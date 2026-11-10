//! Versioned planning state, independent of the host's investigation and workspace formats.
use crate::BuildPlan;
use craft_anything_evidence::{ErrorCode, ToolError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const STATE_VERSION: u32 = 1;
pub const STATE_PATH: &str = "extensions/planning/state.json";
pub const MAX_STATE_BYTES: u64 = 32 * 1024 * 1024;
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u32,
    pub revision: u64,
    #[serde(default)]
    pub plan: Option<BuildPlan>,
}

pub fn schema() -> String {
    let schema = schemars::generate::SchemaSettings::draft2020_12().into_generator().into_root_schema_for::<State>();
    serde_json::to_string_pretty(&schema).map(|s| s + "\n").unwrap_or_default()
}

impl Default for State {
    fn default() -> Self {
        Self { version: STATE_VERSION, revision: 0, plan: None }
    }
}

impl State {
    fn validate(&self) -> Result<(), ToolError> {
        if self.version != STATE_VERSION {
            return Err(ToolError::unsupported(format!("Planning state version {} is not supported; this build reads {STATE_VERSION}.", self.version))
                .with_remediation("Update Craft Anything before changing this planning record."));
        }
        if let Some(plan) = &self.plan {
            plan.validate()?;
        }
        Ok(())
    }
}

fn bad(error: impl std::fmt::Display) -> ToolError {
    ToolError::new(ErrorCode::BadSettings, format!("The planning record could not be read: {error}"))
        .with_remediation("Keep a backup of the planning record, repair it or use a compatible Craft Anything, then try again.")
}

fn too_large() -> ToolError {
    ToolError::new(ErrorCode::OverLimit, "The planning record exceeds the 32 MiB limit; it was not changed.")
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, ToolError> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ToolError::io(path.display(), error)),
    };
    if file.metadata().map_err(|e| ToolError::io(path.display(), e))?.len() > MAX_STATE_BYTES {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| ToolError::io(path.display(), e))?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(too_large());
    }
    Ok(Some(bytes))
}

fn sidecar(root: &Path) -> Result<Option<State>, ToolError> {
    let Some(bytes) = read_bytes(&root.join(STATE_PATH))? else { return Ok(None) };
    let state: State = serde_json::from_slice(&bytes).map_err(bad)?;
    state.validate()?;
    Ok(Some(state))
}

fn legacy(root: &Path) -> Result<State, ToolError> {
    let Some(bytes) = read_bytes(&root.join("investigation/state.json"))? else { return Ok(State::default()) };
    let document: Value = serde_json::from_slice(&bytes).map_err(bad)?;
    let object = document.as_object().ok_or_else(|| bad("the legacy investigation is not an object"))?;
    let Some(plan) = object.get("build_plan").filter(|v| !v.is_null()) else { return Ok(State::default()) };
    if let Some(version) = object.get("version").filter(|version| !version.is_null()) {
        let version = version.as_u64().ok_or_else(|| bad("the legacy investigation version is not a whole number"))?;
        if version > 1 {
            return Err(ToolError::unsupported("The legacy investigation has a newer format; its embedded plan was not imported."));
        }
    }
    let revision = object.get("revision").and_then(Value::as_u64).ok_or_else(|| bad("the legacy investigation has no valid revision"))?;
    let plan: BuildPlan = serde_json::from_value(plan.clone()).map_err(bad)?;
    plan.validate()?;
    Ok(State { version: STATE_VERSION, revision, plan: Some(plan) })
}

/// Read-only: a present sidecar is authoritative, including when it is invalid. Importing a
/// legacy view here does not create a directory or rewrite either record.
pub fn load(root: &Path) -> Result<State, ToolError> {
    if let Some(state) = sidecar(root)? {
        return Ok(state);
    }
    let original = legacy(root);
    // A writer may have preserved the legacy record immediately before replacing it.
    match sidecar(root)? {
        Some(state) => Ok(state),
        None => original,
    }
}

struct Lock(std::fs::File);

impl Lock {
    fn acquire(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self, ToolError> {
        if cancelled() {
            return Err(ToolError::cancelled());
        }
        let parent = path.parent().ok_or_else(|| ToolError::invalid("The planning lock has no parent folder."))?;
        std::fs::create_dir_all(parent).map_err(|e| ToolError::io(parent.display(), e))?;
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| ToolError::io(path.display(), e))?;
        let started = Instant::now();
        loop {
            if cancelled() {
                return Err(ToolError::cancelled());
            }
            match file.try_lock() {
                Ok(()) => return Ok(Self(file)),
                Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < LOCK_TIMEOUT => std::thread::park_timeout(Duration::from_millis(5)),
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(ToolError::new(ErrorCode::TimedOut, "Another process is updating the planning record.")
                        .with_remediation("Wait for the other update, reload with core_build_plan_show and retry."));
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(ToolError::io(path.display(), error)),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn extension_lock(root: &Path) -> PathBuf {
    root.join("extensions/planning/update.lock")
}

fn save(root: &Path, state: &State, cancelled: &dyn Fn() -> bool) -> Result<(), ToolError> {
    state.validate()?;
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| ToolError::internal(e.to_string()))?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(too_large());
    }
    if cancelled() {
        return Err(ToolError::cancelled());
    }
    craft_anything_platform::write_atomic(&root.join(STATE_PATH), &bytes)?;
    if let Some(plan) = &state.plan {
        craft_anything_platform::write_atomic(&root.join("extensions/planning/plan.md"), crate::render::markdown(plan).as_bytes()).map_err(|error| {
            ToolError::new(error.code, format!("The planning state was saved, but its readable plan could not be written: {}", error.message))
                .with_remediation("Reload core_build_plan_show before retrying; the canonical planning state has already been saved.")
        })?;
    }
    Ok(())
}

/// Validate and update under locks. Always take the legacy lock first: the host's analysis
/// writer takes that same lock before preserving its old extension data.
pub fn update(
    root: &Path,
    expected: u64,
    plan: BuildPlan,
    cancelled: &dyn Fn() -> bool,
    validate: impl FnOnce(&BuildPlan) -> Result<(), ToolError>,
) -> Result<State, ToolError> {
    let _legacy = Lock::acquire(&root.join("investigation/update.lock"), cancelled)?;
    let _extension = Lock::acquire(&extension_lock(root), cancelled)?;
    let previous = load(root)?;
    if previous.revision != expected {
        return Err(stale(previous.revision, expected));
    }
    plan.validate()?;
    if let Some(before) = &previous.plan {
        plan.validate_scope_update(before)?;
    }
    validate(&plan)?;
    let revision = expected.checked_add(1).ok_or_else(|| ToolError::new(ErrorCode::OverLimit, "The planning revision cannot advance further."))?;
    let state = State { version: STATE_VERSION, revision, plan: Some(plan) };
    save(root, &state, cancelled)?;
    Ok(state)
}

pub fn stale(current: u64, expected: u64) -> ToolError {
    ToolError::new(ErrorCode::StaleRevision, format!("The build plan is at revision {current}, not {expected}."))
        .with_remediation("Call core_build_plan_show, merge your changes and retry with its planning revision.")
}

/// Called by the host while its legacy investigation update lock is already held. Do not
/// reacquire that lock here. The legacy file is left intact; a present sidecar is never replaced.
pub fn preserve_before_legacy_write(root: &Path, cancelled: &dyn Fn() -> bool) -> Result<(), ToolError> {
    let _extension = Lock::acquire(&extension_lock(root), cancelled)?;
    if sidecar(root)?.is_some() {
        return Ok(());
    }
    let state = legacy(root)?;
    if state.plan.is_some() {
        save(root, &state, cancelled)?;
    }
    Ok(())
}

/// Transfer scratch state into a newly created project, with the host's legacy lock held.
/// Existing planning data wins; the caller must not use this when opening an existing project.
pub fn import_session_locked(root: &Path, state: &State, cancelled: &dyn Fn() -> bool) -> Result<(), ToolError> {
    state.validate()?;
    let _extension = Lock::acquire(&extension_lock(root), cancelled)?;
    if sidecar(root)?.is_some() || legacy(root)?.plan.is_some() {
        return Ok(());
    }
    if state.plan.is_some() {
        save(root, state, cancelled)?;
    }
    Ok(())
}
