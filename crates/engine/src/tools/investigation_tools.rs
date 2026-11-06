//! Investigation memory: brief, plan, hypotheses, findings and handoff, plus the journal reader.
//!
//! With a project open these live in `<project>/investigation/` as `state.json` (the structured
//! source of truth, revisioned) and readable Markdown renders (`brief.md`, `plan.md`,
//! `hypotheses.md`, `findings.md`, `handoff.md`). Without a project they live in the session only.
//! Any agent (ours or an external one over MCP) keeps them current so long investigations survive
//! context limits, restarts and handoffs between agents.

use crate::build_plan::{AcceptanceStatus, BuildPlan};
use crate::journal;
use crate::project::write_atomic;
use crate::session::Session;
use crate::tool::{Ctx, Effects, Requires, ToolOutput, ToolSpec, Toolset, empty_schema, parse_params, schema_of};
use craft_anything_evidence::{Confidence, ErrorCode, Evidence, ToolError};
use craft_anything_platform::sync::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

pub fn specs() -> Vec<ToolSpec> {
    let w = Effects::MUTATES_SESSION.union(Effects::WRITES_FILESYSTEM);
    vec![
        ToolSpec {
            id: "core_investigation_show",
            title: "Investigation so far",
            summary: "The working memory of this investigation: brief, plan with step status, hypotheses, findings (with evidence ids) and the latest handoff. Read it first when starting or resuming.",
            beginner: "See what we're trying to find out, the plan, and what we know so far.",
            toolset: Toolset::Core,
            effects: Effects::READ_ONLY,
            requires: Requires::Nothing,
            input_schema: empty_schema,
            run: core_investigation_show,
        },
        ToolSpec {
            id: "core_brief_write",
            title: "Write the brief",
            summary: "Set the investigation brief: the question, purpose, targets, constraints and what 'done' means. Write it before the first analysis call on a new question.",
            beginner: "Write down the question we're answering and what a good answer looks like.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<BriefParams>,
            run: core_brief_write,
        },
        ToolSpec {
            id: "core_plan_update",
            title: "Update the plan",
            summary: "Replace the plan with a numbered list of steps, each todo/doing/done/blocked with a short why. Pass based_on_revision to avoid overwriting someone else's update.",
            beginner: "Update the step-by-step plan.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<PlanParams>,
            run: core_plan_update,
        },
        ToolSpec {
            id: "core_build_plan_update",
            title: "Update the build plan",
            summary: "Save the build's goal, phases and dependencies, required or optional inputs, and acceptance criteria in the investigation. Read core_investigation_show first and pass its revision as based_on_revision. Retain all phases. Required inputs and criteria must remain required; keep required criteria in their original phase. A done phase is implementation progress; verified acceptance must cite real evidence from the built project or current recorded checks. Ask for missing inputs with core_person_ask when needed; recording an input does not send a question.",
            beginner: "Keep the project phases, missing information and checks together in one plan.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<BuildPlanParams>,
            run: core_build_plan_update,
        },
        ToolSpec {
            id: "core_hypothesis_record",
            title: "Record a hypothesis",
            summary: "Record a claim being tested, ideally with the prediction that would confirm or refute it, plus any supporting/contradicting evidence ids.",
            beginner: "Write down a guess we're going to check.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<HypothesisParams>,
            run: core_hypothesis_record,
        },
        ToolSpec {
            id: "core_hypothesis_update",
            title: "Update a hypothesis",
            summary: "Change a hypothesis's status (testing/supported/refuted/abandoned) and attach supporting or contradicting evidence ids and notes.",
            beginner: "Mark a guess as confirmed, wrong, or still being checked.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<UpdateHypothesisParams>,
            run: core_hypothesis_update,
        },
        ToolSpec {
            id: "core_finding_record",
            title: "Record a finding",
            summary: "Record a settled answer. Requires at least one evidence id; every id is checked against the ledger. Findings are the source for reports.",
            beginner: "Save something we now know for sure, with the proof.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<FindingParams>,
            run: core_finding_record,
        },
        ToolSpec {
            id: "core_finding_edit",
            title: "Update a finding",
            summary: "Correct a recorded finding without changing its id. Requires a statement and existing evidence ids; pass based_on_revision to refuse a stale investigation update.",
            beginner: "Correct something we found, keeping its proof and reference.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<EditFindingParams>,
            run: core_finding_edit,
        },
        ToolSpec {
            id: "core_handoff_write",
            title: "Write the handoff",
            summary: "Rewrite the handoff note: where we are, what's next, traps to avoid. Write it at the end of every turn so the next turn (or agent) resumes without re-deriving.",
            beginner: "Leave a note so we can pick up exactly where we left off.",
            toolset: Toolset::Core,
            effects: w,
            requires: Requires::Nothing,
            input_schema: schema_of::<HandoffParams>,
            run: core_handoff_write,
        },
        ToolSpec {
            id: "core_journal_read",
            title: "Read the journal",
            summary: "Page through the project's append-only journal (tool calls, model turns, consents, errors), filtered by day, kind or tool. Needs an open project.",
            beginner: "See a log of everything that happened in this project.",
            toolset: Toolset::Core,
            effects: Effects::READ_ONLY,
            requires: Requires::Nothing,
            input_schema: schema_of::<JournalParams>,
            run: core_journal_read,
        },
    ]
}

// ---- state ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Brief {
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_when: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Todo,
    Doing,
    Done,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanStep {
    pub title: String,
    #[serde(default)]
    pub status: StepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    #[default]
    Testing,
    Supported,
    Refuted,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Hypothesis {
    pub id: String,
    pub claim: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prediction: Option<String>,
    pub status: HypothesisStatus,
    #[serde(default)]
    pub supporting: Vec<String>,
    #[serde(default)]
    pub contradicting: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Finding {
    pub id: String,
    pub statement: String,
    pub evidence_ids: Vec<String>,
    #[schemars(with = "ConfidenceParam")]
    pub confidence: Confidence,
}

pub const INVESTIGATION_VERSION: u32 = 1;
const MAX_STATE_BYTES: usize = 32 * 1024 * 1024;
const INVESTIGATION_STEPS: &[crate::versioned::Step<Value>] = &[investigation_v0_to_v1];
const _: () = assert!(INVESTIGATION_STEPS.len() == INVESTIGATION_VERSION as usize);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Investigation {
    #[serde(default = "investigation_version")]
    pub version: u32,
    pub revision: u64,
    #[serde(default)]
    pub brief: Option<Brief>,
    #[serde(default)]
    pub plan: Vec<PlanStep>,
    /// Canonical project scope and progress. The older plan remains an analysis checklist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_plan: Option<BuildPlan>,
    #[serde(default)]
    pub hypotheses: Vec<Hypothesis>,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub handoff: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

fn investigation_version() -> u32 {
    INVESTIGATION_VERSION
}

impl Default for Investigation {
    fn default() -> Self {
        Self {
            version: INVESTIGATION_VERSION,
            revision: 0,
            brief: None,
            plan: Vec::new(),
            build_plan: None,
            hypotheses: Vec::new(),
            findings: Vec::new(),
            handoff: None,
            updated: None,
        }
    }
}

fn investigation_v0_to_v1(document: &mut Value) -> Result<(), String> {
    if document.is_object() { Ok(()) } else { Err("the investigation is not a JSON object".into()) }
}

fn state_error(reason: impl std::fmt::Display) -> ToolError {
    ToolError::new(ErrorCode::GarbledOutput, format!("The investigation state could not be read; it was not changed. {reason}"))
        .with_remediation("Restore a valid investigation/state.json backup before editing this investigation.")
}

fn state_too_large() -> ToolError {
    ToolError::new(ErrorCode::OverLimit, "The investigation state exceeds the 32 MiB limit; it was not changed.")
        .with_remediation("Keep a backup, then shorten the investigation notes before retrying.")
}

fn validate_state(investigation: &Investigation) -> Result<(), ToolError> {
    if investigation.version != INVESTIGATION_VERSION {
        return Err(ToolError::unsupported(format!(
            "Investigation version {} is not supported; this build reads version {INVESTIGATION_VERSION}.",
            investigation.version
        ))
        .with_remediation("Update Craft Anything before reading or changing this investigation."));
    }
    if let Some(plan) = &investigation.build_plan {
        plan.validate()?;
    }
    Ok(())
}

/// Parse legacy unversioned state in memory. Saving writes the current version; unreadable or
/// future state is never replaced by an empty investigation on this strict path.
pub fn read_investigation(bytes: &[u8]) -> Result<Investigation, ToolError> {
    if bytes.len() > MAX_STATE_BYTES {
        return Err(state_too_large());
    }
    let mut document: Value = serde_json::from_slice(bytes).map_err(state_error)?;
    let found = match document.get("version") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| state_error("The version must be a whole number."))?,
    };
    if found > INVESTIGATION_VERSION {
        return Err(ToolError::unsupported(format!(
            "This investigation needs a newer Craft Anything (version {found}; this build reads {INVESTIGATION_VERSION})."
        ))
        .with_remediation("Update Craft Anything before reading or changing this investigation."));
    }
    let reached = crate::versioned::upgrade(&mut document, found, 0, INVESTIGATION_STEPS).map_err(state_error)?;
    if let Some(object) = document.as_object_mut() {
        object.insert("version".into(), Value::from(reached));
    }
    let investigation: Investigation = serde_json::from_value(document).map_err(state_error)?;
    validate_state(&investigation)?;
    Ok(investigation)
}

/// The schema of the canonical investigation state, including its optional build plan.
pub fn investigation_schema() -> String {
    let schema = schemars::generate::SchemaSettings::draft2020_12().into_generator().into_root_schema_for::<Investigation>();
    serde_json::to_string_pretty(&schema).map(|s| s + "\n").unwrap_or_default()
}

type Slot = Mutex<Investigation>;

/// The last loaded state, for clients that cannot read the project while drawing. Writers and
/// project refreshes update this snapshot; an external writer is picked up by open_project.
pub fn cached(session: &Session) -> Investigation {
    session.ext.get::<Slot>().map(|slot| slot.lock().unwrap_or_else(PoisonError::into_inner).clone()).unwrap_or_default()
}

pub(crate) fn cache(session: &mut Session, investigation: Investigation) {
    match session.ext.get::<Slot>() {
        Some(slot) => *slot.lock().unwrap_or_else(PoisonError::into_inner) = investigation,
        None => session.ext.insert(Arc::new(Mutex::new(investigation))),
    }
}

fn dir(session: &Session) -> Option<PathBuf> {
    session.project.as_ref().map(|p| p.root.join("investigation"))
}

/// The current investigation state (from the project when one is open, else the session).
pub fn load(session: &Session) -> Investigation {
    load_strict(session).unwrap_or_default()
}

/// Use for completion decisions: missing state is a new investigation, while invalid
/// state is an error. The lenient legacy `load` is only suitable for optional display snapshots.
pub fn load_strict(session: &Session) -> Result<Investigation, ToolError> {
    if let Some(project) = &session.project {
        return load_project_strict(&project.root);
    }
    let investigation = load_state(session)?;
    if let Some(plan) = &investigation.build_plan {
        validate_plan_evidence(plan, |id| session.resolve_evidence(id))?;
    }
    Ok(investigation)
}

/// Read from a `.cany` project root, not a rebuild workspace root. Call off the UI thread.
pub fn load_project_strict(project_root: &Path) -> Result<Investigation, ToolError> {
    let investigation = load_project_state(project_root)?;
    if let Some(plan) = &investigation.build_plan
        && (plan.inputs.iter().any(|input| !input.evidence_ids.is_empty()) || plan.acceptance.iter().any(|criterion| !criterion.evidence_ids.is_empty()))
    {
        let project = crate::project::Project::open(project_root)?;
        validate_plan_evidence(plan, |id| project.load_evidence(id))?;
    }
    Ok(investigation)
}

/// Structurally valid state for inspection and repair. Completion must use `load_strict`, which
/// additionally resolves recorded evidence; an inspection can show a stale citation to fix it.
pub fn load_state(session: &Session) -> Result<Investigation, ToolError> {
    if let Some(project) = &session.project {
        return load_project_state(&project.root);
    }
    let investigation = cached(session);
    validate_state(&investigation)?;
    Ok(investigation)
}

/// Read structurally valid state from a `.cany` root without treating citations as verified.
/// Use `load_project_strict` before deciding completion. Call off the UI thread.
pub fn load_project_state(project_root: &Path) -> Result<Investigation, ToolError> {
    load_directory_strict(&project_root.join("investigation"))
}

fn load_directory_strict(directory: &Path) -> Result<Investigation, ToolError> {
    let path = directory.join("state.json");
    match std::fs::File::open(&path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_STATE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|error| ToolError::io(path.display(), error))?;
            read_investigation(&bytes)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Investigation::default()),
        Err(error) => Err(ToolError::io(path.display(), error)),
    }
}

struct UpdateLock {
    directory: Option<PathBuf>,
    _file: Option<std::fs::File>,
}

impl UpdateLock {
    fn acquire(session: &Session, cancel: &crate::CancelToken, timeout: Duration) -> Result<Self, ToolError> {
        cancel.check()?;
        let directory = dir(session);
        let Some(path) = directory.as_ref() else { return Ok(Self { directory, _file: None }) };
        std::fs::create_dir_all(path).map_err(|error| ToolError::io(path.display(), error))?;
        let lock_path = path.join("update.lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| ToolError::io(lock_path.display(), error))?;
        let started = Instant::now();
        loop {
            cancel.check()?;
            match file.try_lock() {
                Ok(()) => return Ok(Self { directory, _file: Some(file) }),
                Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < timeout => std::thread::park_timeout(Duration::from_millis(5)),
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(ToolError::new(ErrorCode::TimedOut, "Another process is updating this investigation.")
                        .with_remediation("Wait for the other update, reload the investigation and retry with its current revision."));
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(ToolError::io(lock_path.display(), error)),
            }
        }
    }
}

fn lock_update(session: &Session, cancel: &crate::CancelToken) -> Result<UpdateLock, ToolError> {
    UpdateLock::acquire(session, cancel, Duration::from_secs(10))
}

fn load_for_update(session: &Session, guard: &UpdateLock) -> Result<Investigation, ToolError> {
    let Some(directory) = &guard.directory else { return load_state(session) };
    load_directory_strict(directory)
}

pub(crate) fn save_new_project(session: &mut Session, inv: Investigation) -> Result<Investigation, ToolError> {
    let guard = lock_update(session, &crate::CancelToken::default())?;
    if let Some(directory) = &guard.directory {
        let path = directory.join("state.json");
        if path.try_exists().map_err(|error| ToolError::io(path.display(), error))? {
            return Err(ToolError::new(ErrorCode::StaleRevision, "This project already has an investigation.")
                .with_remediation("Reload the existing investigation instead of replacing it."));
        }
    }
    save(session, inv, &guard)
}

fn save(session: &mut Session, mut inv: Investigation, guard: &UpdateLock) -> Result<Investigation, ToolError> {
    if guard.directory != dir(session) {
        return Err(ToolError::internal("The investigation changed while its update lock was held."));
    }
    validate_state(&inv)?;
    inv.revision = inv.revision.saturating_add(1);
    let (_, date, time) = journal::now_utc();
    inv.updated = Some(format!("{date}T{time}Z"));
    if let Some(d) = dir(session) {
        let text = serde_json::to_vec_pretty(&inv).map_err(|e| ToolError::internal(e.to_string()))?;
        if text.len() > MAX_STATE_BYTES {
            return Err(state_too_large());
        }
        write_atomic(&d.join("state.json"), &text)?;
        for (name, body) in render(&inv) {
            write_atomic(&d.join(name), body.as_bytes())?;
        }
    }
    cache(session, inv.clone());
    Ok(inv)
}

/// The Markdown files, rendered from the structured state.
pub fn render(inv: &Investigation) -> Vec<(&'static str, String)> {
    let mut brief = String::from("# Brief\n\n");
    match &inv.brief {
        Some(b) => {
            brief.push_str(&format!("**Question:** {}\n\n", b.question));
            if let Some(p) = &b.purpose {
                brief.push_str(&format!("**Purpose:** {p}\n\n"));
            }
            if !b.targets.is_empty() {
                brief.push_str("**Targets:**\n");
                for t in &b.targets {
                    brief.push_str(&format!("- {t}\n"));
                }
                brief.push('\n');
            }
            if !b.constraints.is_empty() {
                brief.push_str("**Constraints:**\n");
                for c in &b.constraints {
                    brief.push_str(&format!("- {c}\n"));
                }
                brief.push('\n');
            }
            if let Some(d) = &b.done_when {
                brief.push_str(&format!("**Done when:** {d}\n"));
            }
        }
        None => brief.push_str("_No brief yet._\n"),
    }
    let mut plan = String::from("# Plan\n\n");
    if inv.plan.is_empty() {
        plan.push_str("_No plan yet._\n");
    }
    for (i, s) in inv.plan.iter().enumerate() {
        let st = match s.status {
            StepStatus::Todo => "todo",
            StepStatus::Doing => "doing",
            StepStatus::Done => "done",
            StepStatus::Blocked => "blocked",
        };
        plan.push_str(&format!("{}. [{st}] {}", i + 1, s.title));
        if let Some(w) = &s.why {
            plan.push_str(&format!(" — {w}"));
        }
        plan.push('\n');
    }
    if let Some(build) = &inv.build_plan {
        let analysis_plan = plan;
        plan = format!("# Build plan\n\n{}\n\n## Phases\n\n", build.goal);
        for phase in &build.phases {
            plan.push_str(&format!("### {} — {} ({:?})\n\n{}\n\n", phase.id, phase.title, phase.status, phase.goal));
            if !phase.depends_on.is_empty() {
                plan.push_str(&format!("Depends on: {}\n\n", phase.depends_on.join(", ")));
            }
        }
        plan.push_str("## Inputs\n\n");
        if build.inputs.is_empty() {
            plan.push_str("_No missing inputs recorded._\n\n");
        }
        for input in &build.inputs {
            plan.push_str(&format!(
                "### {} ({}, {:?})\n\n{}\n\nWhy: {}\n\nExpected format: {}\n\nPhases: {}\n\n",
                input.id,
                if input.required { "required" } else { "optional" },
                input.status,
                input.question,
                input.why,
                input.format,
                input.phase_ids.join(", ")
            ));
            if let Some(response) = &input.response {
                plan.push_str(&format!("Response: {response}\n\n"));
            }
            if !input.evidence_ids.is_empty() {
                plan.push_str(&format!("Evidence: {}\n\n", input.evidence_ids.join(", ")));
            }
        }
        plan.push_str("## Acceptance criteria\n\nRecorded verification cites evidence or check results; a done phase alone is not verification.\n\n");
        for criterion in &build.acceptance {
            plan.push_str(&format!(
                "- **{}** ({}, {:?}; phase {}) {}\n",
                criterion.id,
                if criterion.required { "required" } else { "optional" },
                criterion.status,
                criterion.phase_id,
                criterion.description
            ));
            if !criterion.evidence_ids.is_empty() {
                plan.push_str(&format!("  Evidence: {}\n", criterion.evidence_ids.join(", ")));
            }
            for check in &criterion.checks {
                plan.push_str(&format!(
                    "  Check: {}, round {}{}\n",
                    check.check,
                    check.round,
                    check.claim.as_ref().map(|claim| format!(", claim {claim}")).unwrap_or_default()
                ));
            }
        }
        if !inv.plan.is_empty() {
            plan.push_str("\n## Investigation checklist\n\n");
            plan.push_str(analysis_plan.strip_prefix("# Plan\n\n").unwrap_or(&analysis_plan));
        }
    }
    let mut hyp = String::from("# Hypotheses\n\n");
    if inv.hypotheses.is_empty() {
        hyp.push_str("_None yet._\n");
    }
    for h in &inv.hypotheses {
        hyp.push_str(&format!("## {} ({:?})\n\n{}\n\n", h.id, h.status, h.claim));
        if let Some(p) = &h.prediction {
            hyp.push_str(&format!("Prediction: {p}\n\n"));
        }
        if !h.supporting.is_empty() {
            hyp.push_str(&format!("Supporting: {}\n\n", h.supporting.join(", ")));
        }
        if !h.contradicting.is_empty() {
            hyp.push_str(&format!("Contradicting: {}\n\n", h.contradicting.join(", ")));
        }
        for n in &h.notes {
            hyp.push_str(&format!("- {n}\n"));
        }
    }
    let mut fnd = String::from("# Findings\n\n");
    if inv.findings.is_empty() {
        fnd.push_str("_None yet._\n");
    }
    for f in &inv.findings {
        fnd.push_str(&format!("- **{}** {} ({}; evidence: {})\n", f.id, f.statement, f.confidence.plain_label(), f.evidence_ids.join(", ")));
    }
    let handoff = format!("# Handoff\n\n{}\n", inv.handoff.clone().unwrap_or_else(|| "_No handoff yet._".into()));
    vec![("brief.md", brief), ("plan.md", plan), ("hypotheses.md", hyp), ("findings.md", fnd), ("handoff.md", handoff)]
}

fn out(inv: &Investigation, session: &Session) -> ToolOutput {
    let verification_issue = inv.build_plan.as_ref().and_then(|plan| {
        validate_plan_evidence(plan, |id| match &session.project {
            Some(project) => project.load_evidence(id),
            None => session.resolve_evidence(id),
        })
        .err()
        .map(|error| error.message)
    });
    ToolOutput::Value(json!({
        "version": inv.version,
        "revision": inv.revision,
        "updated": inv.updated,
        "brief": inv.brief,
        "plan": inv.plan,
        "build_plan": inv.build_plan,
        "build_plan_verification_issue": verification_issue,
        "hypotheses": inv.hypotheses,
        "findings": inv.findings,
        "handoff": inv.handoff,
        "stored_in": dir(session).map(|d| d.display().to_string()).unwrap_or_else(|| "session (no project open)".into()),
    }))
}

fn check_ids(session: &Session, ids: &[String]) -> Result<(), ToolError> {
    for id in ids {
        session.resolve_evidence(id).map_err(|_| {
            ToolError::invalid(format!("Unknown evidence id {id}")).with_remediation("Cite ids returned by analysis tools (see core_evidence_show).")
        })?;
    }
    Ok(())
}

fn validate_plan_evidence(plan: &BuildPlan, mut resolve: impl FnMut(&str) -> Result<Evidence, ToolError>) -> Result<(), ToolError> {
    for input in &plan.inputs {
        for id in &input.evidence_ids {
            resolve(id)?;
        }
    }
    for criterion in &plan.acceptance {
        for id in &criterion.evidence_ids {
            let evidence = resolve(id)?;
            if criterion.status == AcceptanceStatus::Verified && evidence.confidence == Confidence::Inferred {
                return Err(ToolError::invalid(format!("Acceptance criterion {} needs observed or derived evidence; {id} is inferred.", criterion.id)));
            }
        }
    }
    Ok(())
}

fn check_revision(inv: &Investigation, expected: Option<u64>) -> Result<(), ToolError> {
    match expected {
        Some(r) if r != inv.revision => Err(ToolError::new(ErrorCode::StaleRevision, format!("The investigation is at revision {}, not {r}.", inv.revision))
            .with_remediation("Call core_investigation_show, merge your change, and retry with the current revision.")),
        _ => Ok(()),
    }
}

// ---- tools ----

fn core_investigation_show(ctx: &mut Ctx<'_>, _p: Value) -> Result<ToolOutput, ToolError> {
    let inv = load_state(ctx.session)?;
    Ok(out(&inv, ctx.session))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BriefParams {
    /// The question being answered, in one or two sentences.
    pub question: String,
    /// Why the user is asking (interoperability, security review, learning…).
    #[serde(default)]
    pub purpose: Option<String>,
    /// Targets involved (paths, packages, URLs).
    #[serde(default)]
    pub targets: Vec<String>,
    /// Constraints (clean room, no running the target, time budget…).
    #[serde(default)]
    pub constraints: Vec<String>,
    /// What a finished answer looks like.
    #[serde(default)]
    pub done_when: Option<String>,
}

fn core_brief_write(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: BriefParams = parse_params(p)?;
    if p.question.trim().is_empty() {
        return Err(ToolError::invalid("The brief needs a question."));
    }
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut inv = load_for_update(ctx.session, &guard)?;
    inv.brief = Some(Brief { question: p.question, purpose: p.purpose, targets: p.targets, constraints: p.constraints, done_when: p.done_when });
    let inv = save(ctx.session, inv, &guard)?;
    Ok(out(&inv, ctx.session))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanParams {
    /// The whole plan, in order (replaces the previous one).
    pub steps: Vec<PlanStep>,
    /// The revision your change is based on (from core_investigation_show); omit to overwrite.
    #[serde(default)]
    pub based_on_revision: Option<u64>,
}

fn core_plan_update(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: PlanParams = parse_params(p)?;
    if p.steps.len() > 200 {
        return Err(ToolError::invalid("A plan can have at most 200 steps."));
    }
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut inv = load_for_update(ctx.session, &guard)?;
    check_revision(&inv, p.based_on_revision)?;
    inv.plan = p.steps;
    let inv = save(ctx.session, inv, &guard)?;
    Ok(out(&inv, ctx.session))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildPlanParams {
    /// The whole build plan. Preserve existing ids and requirements while updating progress.
    pub plan: BuildPlan,
    /// Required revision from core_investigation_show; stale updates are refused.
    pub based_on_revision: u64,
}

fn core_build_plan_update(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let params: BuildPlanParams = parse_params(params)?;
    params.plan.validate()?;
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut investigation = load_for_update(ctx.session, &guard)?;
    check_revision(&investigation, Some(params.based_on_revision))?;
    if let Some(previous) = &investigation.build_plan {
        params.plan.validate_scope_update(previous)?;
    }
    validate_plan_evidence(&params.plan, |id| ctx.session.resolve_evidence(id))?;
    let workspace = super::interaction_tools::inbox(ctx).as_ref().and_then(super::interaction_tools::workspace_of);
    for criterion in &params.plan.acceptance {
        for reference in &criterion.checks {
            let workspace = workspace.as_ref().ok_or_else(|| {
                ToolError::unavailable("Check citations need the build workspace that recorded them.")
                    .with_remediation("Use the build plan tool from the project's attached builder, or cite recorded evidence instead.")
            })?;
            // The run's fallback likeness threshold is not stored here. Validate the recorded
            // success and its own threshold now; completion also applies the run's threshold.
            reference.verify(&workspace.manifest.checks, 0.0)?;
        }
    }
    investigation.build_plan = Some(params.plan);
    let investigation = save(ctx.session, investigation, &guard)?;
    Ok(out(&investigation, ctx.session))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HypothesisParams {
    /// The claim being tested.
    pub claim: String,
    /// What we expect to observe if the claim is true.
    #[serde(default)]
    pub prediction: Option<String>,
    #[serde(default)]
    pub evidence_for: Vec<String>,
    #[serde(default)]
    pub evidence_against: Vec<String>,
}

fn core_hypothesis_record(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: HypothesisParams = parse_params(p)?;
    if p.claim.trim().is_empty() {
        return Err(ToolError::invalid("A hypothesis needs a claim."));
    }
    check_ids(ctx.session, &p.evidence_for)?;
    check_ids(ctx.session, &p.evidence_against)?;
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut inv = load_for_update(ctx.session, &guard)?;
    let id = format!("hyp_{}", inv.hypotheses.len() + 1);
    inv.hypotheses.push(Hypothesis {
        id: id.clone(),
        claim: p.claim,
        prediction: p.prediction,
        status: HypothesisStatus::Testing,
        supporting: p.evidence_for,
        contradicting: p.evidence_against,
        notes: Vec::new(),
    });
    let inv = save(ctx.session, inv, &guard)?;
    Ok(ToolOutput::Value(json!({"hypothesis_id": id, "revision": inv.revision})))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateHypothesisParams {
    pub hypothesis_id: String,
    #[serde(default)]
    pub status: Option<HypothesisStatus>,
    #[serde(default)]
    pub add_supporting_evidence_ids: Vec<String>,
    #[serde(default)]
    pub add_contradicting_evidence_ids: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

fn core_hypothesis_update(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: UpdateHypothesisParams = parse_params(p)?;
    check_ids(ctx.session, &p.add_supporting_evidence_ids)?;
    check_ids(ctx.session, &p.add_contradicting_evidence_ids)?;
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut inv = load_for_update(ctx.session, &guard)?;
    let h = inv
        .hypotheses
        .iter_mut()
        .find(|h| h.id == p.hypothesis_id)
        .ok_or_else(|| ToolError::invalid(format!("No hypothesis {}", p.hypothesis_id)).with_remediation("See core_investigation_show for the ids."))?;
    if let Some(s) = p.status {
        h.status = s;
    }
    for id in p.add_supporting_evidence_ids {
        if !h.supporting.contains(&id) {
            h.supporting.push(id);
        }
    }
    for id in p.add_contradicting_evidence_ids {
        if !h.contradicting.contains(&id) {
            h.contradicting.push(id);
        }
    }
    if let Some(n) = p.note.filter(|n| !n.trim().is_empty()) {
        h.notes.push(n);
    }
    let snapshot = h.clone();
    let inv = save(ctx.session, inv, &guard)?;
    Ok(ToolOutput::Value(json!({"hypothesis": snapshot, "revision": inv.revision})))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindingParams {
    /// The settled answer, in one or two sentences.
    pub statement: String,
    /// Evidence ids that establish it (at least one).
    pub evidence_ids: Vec<String>,
    /// observed (read directly), derived (computed from observations) or inferred (best guess).
    #[serde(default = "inferred")]
    pub confidence: ConfidenceParam,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceParam {
    Observed,
    Derived,
    Inferred,
}

fn inferred() -> ConfidenceParam {
    ConfidenceParam::Inferred
}

fn core_finding_record(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: FindingParams = parse_params(p)?;
    if p.statement.trim().is_empty() {
        return Err(ToolError::invalid("A finding needs a statement."));
    }
    if p.evidence_ids.is_empty() {
        return Err(ToolError::invalid("A finding must cite at least one evidence id.")
            .with_remediation("Run the analysis that shows it, then cite the evidence id it returns."));
    }
    check_ids(ctx.session, &p.evidence_ids)?;
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut inv = load_for_update(ctx.session, &guard)?;
    let id = format!("fnd_{}", inv.findings.len() + 1);
    let confidence = match p.confidence {
        ConfidenceParam::Observed => Confidence::Observed,
        ConfidenceParam::Derived => Confidence::Derived,
        ConfidenceParam::Inferred => Confidence::Inferred,
    };
    inv.findings.push(Finding { id: id.clone(), statement: p.statement, evidence_ids: p.evidence_ids, confidence });
    let inv = save(ctx.session, inv, &guard)?;
    Ok(ToolOutput::Value(json!({"finding_id": id, "revision": inv.revision})))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditFindingParams {
    /// An existing finding id from core_investigation_show.
    pub finding_id: String,
    /// The corrected settled answer.
    pub statement: String,
    /// Evidence ids that establish the corrected answer (at least one).
    pub evidence_ids: Vec<String>,
    /// Omit to keep the finding's current confidence.
    #[serde(default)]
    pub confidence: Option<ConfidenceParam>,
    /// The investigation revision read before editing; a stale revision is refused.
    #[serde(default)]
    pub based_on_revision: Option<u64>,
}

fn core_finding_edit(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let params: EditFindingParams = parse_params(params)?;
    if params.statement.trim().is_empty() || params.evidence_ids.is_empty() {
        return Err(ToolError::invalid("A finding needs a statement and at least one evidence id."));
    }
    check_ids(ctx.session, &params.evidence_ids)?;
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut investigation = load_for_update(ctx.session, &guard)?;
    check_revision(&investigation, params.based_on_revision)?;
    let finding = investigation
        .findings
        .iter_mut()
        .find(|finding| finding.id == params.finding_id)
        .ok_or_else(|| ToolError::invalid(format!("Unknown finding {}", params.finding_id)))?;
    finding.statement = params.statement;
    finding.evidence_ids = params.evidence_ids;
    if let Some(confidence) = params.confidence {
        finding.confidence = match confidence {
            ConfidenceParam::Observed => Confidence::Observed,
            ConfidenceParam::Derived => Confidence::Derived,
            ConfidenceParam::Inferred => Confidence::Inferred,
        };
    }
    let investigation = save(ctx.session, investigation, &guard)?;
    Ok(ToolOutput::Value(json!({"finding_id": params.finding_id, "revision": investigation.revision})))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HandoffParams {
    /// Where we are, what's next, and traps to avoid. Replaces the previous handoff.
    pub text: String,
}

fn core_handoff_write(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: HandoffParams = parse_params(p)?;
    let guard = lock_update(ctx.session, &ctx.cancel)?;
    let mut inv = load_for_update(ctx.session, &guard)?;
    inv.handoff = Some(p.text);
    let inv = save(ctx.session, inv, &guard)?;
    Ok(ToolOutput::Value(json!({"revision": inv.revision})))
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalParams {
    /// Day to read (`YYYY-MM-DD`); the latest day when omitted.
    #[serde(default)]
    pub day: Option<String>,
    /// Only entries of this kind (`tool_call`, `model_turn`, `consent`, `user_message`, `error`…).
    #[serde(default)]
    pub kind: Option<String>,
    /// Only calls of this tool.
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub offset: usize,
    /// At most 500; default 50.
    #[serde(default)]
    pub limit: Option<usize>,
}

fn core_journal_read(ctx: &mut Ctx<'_>, p: Value) -> Result<ToolOutput, ToolError> {
    let p: JournalParams = parse_params(p)?;
    let root = ctx.session.project.as_ref().map(|pr| pr.root.clone()).ok_or_else(|| {
        ToolError::unavailable("The journal is kept per project, and no project is open.").with_remediation("Open or create a project (.cany) first.")
    })?;
    let days = journal::days(&root);
    let day = p.day.or_else(|| days.last().cloned()).unwrap_or_default();
    let entries: Vec<Value> = journal::read_day(&root, &day)
        .into_iter()
        .filter(|e| p.kind.as_ref().is_none_or(|k| e["kind"] == *k))
        .filter(|e| p.tool.as_ref().is_none_or(|t| e["tool"] == *t))
        .collect();
    let limit = p.limit.unwrap_or(50).clamp(1, 500);
    let total = entries.len();
    let page: Vec<Value> = entries.into_iter().skip(p.offset).take(limit).collect();
    let next = p.offset.saturating_add(page.len());
    Ok(ToolOutput::Value(json!({
        "day": day,
        "days": days,
        "total": total,
        "entries": page,
        "next_offset": if next < total { Some(next) } else { None },
    })))
}

#[cfg(test)]
mod tests {
    use crate::project::{Project, Purpose};
    use crate::{CallOptions, Caller, Engine};
    use serde_json::json;

    fn call(e: &Engine, id: &str, p: serde_json::Value) -> Result<serde_json::Value, craft_anything_evidence::ToolError> {
        e.call(id, p, CallOptions::new(Caller::Agent)).map(|o| o.to_json())
    }

    fn build_plan(goal: &str) -> serde_json::Value {
        json!({
            "goal":goal,
            "phases":[{"id":"catalogue","title":"Catalogue","goal":"Store and reopen entries"}],
            "inputs":[{"id":"sample","question":"Which sample record?","why":"Defines the fields","format":"One record","phase_ids":["catalogue"]}],
            "acceptance":[{"id":"reload","phase_id":"catalogue","description":"Saved entries survive reopening"}]
        })
    }

    #[test]
    fn build_plan_survives_resuming_and_other_investigation_updates() {
        let directory = tempfile::tempdir().unwrap();
        let project = Project::create(&directory.path().join("project.cany"), "Test", Purpose::Education).unwrap();
        let root = project.root.clone();
        let mut session = crate::Session::isolated(&directory.path().join("config"));
        session.settings.deep_mode = false;
        session.open_project(project).unwrap();
        let engine = Engine::new(session);
        let mut plan = build_plan("Build a catalogue");
        plan["inputs"][0]["status"] = json!("provided");
        plan["inputs"][0]["response"] = json!("Use the synthetic record.");
        let update = json!({"plan":plan,"based_on_revision":0});
        let saved = call(&engine, "core_build_plan_update", update.clone()).unwrap();
        assert_eq!(saved["result"]["revision"], 1);
        assert_eq!(call(&engine, "core_build_plan_update", update).unwrap_err().code, craft_anything_evidence::ErrorCode::StaleRevision);
        call(&engine, "core_plan_update", json!({"steps":[{"title":"Study the visible form"}],"based_on_revision":1})).unwrap();
        call(&engine, "core_handoff_write", json!({"text":"Next: implement persistence."})).unwrap();
        let mut resumed = crate::Session::isolated(&directory.path().join("resumed-config"));
        resumed.open_project(Project::open(&root).unwrap()).unwrap();
        let investigation = super::load_strict(&resumed).unwrap();
        assert_eq!(investigation.version, super::INVESTIGATION_VERSION);
        assert_eq!(investigation.revision, 3);
        let build = investigation.build_plan.as_ref().unwrap();
        assert!(build.inputs[0].provided());
        assert_eq!(build.phases[0].status, crate::build_plan::PhaseStatus::Todo);
        assert_eq!(build.acceptance[0].status, crate::build_plan::AcceptanceStatus::Pending);
        assert_eq!(super::cached(&resumed), investigation);
        assert_eq!(super::load_project_strict(&root).unwrap(), investigation);
        let rendered = std::fs::read_to_string(root.join("investigation/plan.md")).unwrap();
        assert!(rendered.contains("# Build plan"));
        assert!(rendered.contains("Which sample record?") && rendered.contains("Saved entries survive reopening"));
        assert!(rendered.contains("Study the visible form"));
        assert_eq!(super::render(&investigation).into_iter().find(|(name, _)| *name == "plan.md").unwrap().1, rendered);
    }

    #[test]
    fn build_plan_verification_requires_existing_non_inferred_evidence() {
        use craft_anything_evidence::{Confidence, Environment, EvidenceBuilder, Subject};
        let engine = Engine::new(crate::testing::session());
        let mut plan = build_plan("Build a catalogue");
        plan["acceptance"][0]["status"] = json!("verified");
        let before = call(&engine, "core_investigation_show", json!({})).unwrap();
        assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_err());
        plan["acceptance"][0]["evidence_ids"] = json!([format!("ev_{}", "0".repeat(64))]);
        assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_err());
        assert_eq!(call(&engine, "core_investigation_show", json!({})).unwrap(), before);
        for (confidence, accepted) in [(Confidence::Inferred, false), (Confidence::Observed, true)] {
            let evidence = EvidenceBuilder::new("test-reload", Subject { kind: "test".into(), sha256: None, name: None }, Environment::builtin(true))
                .result(json!({"entries_after_reload":1}))
                .summary("Synthetic reload observation")
                .confidence(confidence)
                .build()
                .unwrap();
            let id = evidence.id.clone();
            engine.session().record(evidence).unwrap();
            plan["acceptance"][0]["evidence_ids"] = json!([id]);
            assert_eq!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_ok(), accepted);
        }
        assert_eq!(super::cached(&engine.session()).revision, 1);
    }

    #[test]
    fn build_plan_updates_cannot_drop_or_demote_required_scope() {
        let engine = Engine::new(crate::testing::session());
        let mut plan = build_plan("Build a catalogue");
        let mut optional_input = plan["inputs"][0].clone();
        optional_input["id"] = json!("art");
        optional_input["required"] = json!(false);
        plan["inputs"].as_array_mut().unwrap().push(optional_input);
        let mut optional_criterion = plan["acceptance"][0].clone();
        optional_criterion["id"] = json!("polish");
        optional_criterion["required"] = json!(false);
        plan["acceptance"].as_array_mut().unwrap().push(optional_criterion.clone());
        call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).unwrap();
        let before = call(&engine, "core_investigation_show", json!({})).unwrap();
        let mut omitted_input = plan.clone();
        omitted_input["inputs"] = json!([]);
        let mut demoted_input = plan.clone();
        demoted_input["inputs"][0]["required"] = json!(false);
        let mut omitted_criterion = plan.clone();
        omitted_criterion["acceptance"] = json!([optional_criterion]);
        let mut demoted_criterion = plan.clone();
        demoted_criterion["acceptance"][0]["required"] = json!(false);
        let mut moved_criterion = plan.clone();
        moved_criterion["acceptance"][0]["phase_id"] = json!("other");
        moved_criterion["phases"].as_array_mut().unwrap().push(json!({"id":"other","title":"Other","goal":"Another phase"}));
        for invalid in [omitted_input, demoted_input, omitted_criterion, demoted_criterion, moved_criterion] {
            assert!(call(&engine, "core_build_plan_update", json!({"plan":invalid,"based_on_revision":1})).is_err());
            assert_eq!(call(&engine, "core_investigation_show", json!({})).unwrap(), before);
        }
        plan["inputs"].as_array_mut().unwrap().retain(|input| input["required"] != false);
        plan["acceptance"].as_array_mut().unwrap().retain(|criterion| criterion["required"] != false);
        plan["inputs"][0]["status"] = json!("provided");
        plan["inputs"][0]["response"] = json!("Use the synthetic record.");
        plan["acceptance"][0]["status"] = json!("implemented");
        plan["acceptance"][0]["description"] = json!("Stored entries remain after reopening the app.");
        let saved = call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":1})).unwrap();
        assert_eq!(saved["result"]["revision"], 2);
        assert_eq!(saved["result"]["build_plan"]["inputs"].as_array().unwrap().len(), 1);
        assert_eq!(saved["result"]["build_plan"]["acceptance"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn build_plan_check_citations_are_resolved_against_the_attached_workspace() {
        use crate::workspace::{CheckResult, Claim, TargetRef, Workspace};
        let directory = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::create(directory.path(), "Catalogue", "Store records", TargetRef::None).unwrap();
        let mut session = crate::Session::isolated(&directory.path().join("config"));
        session.ext.insert(std::sync::Arc::new(crate::inbox::InboxDir(workspace.inbox_dir())));
        let engine = Engine::new(session);
        let mut plan = build_plan("Build a catalogue");
        plan["acceptance"][0]["status"] = json!("verified");
        plan["acceptance"][0]["checks"] = json!([{"check":"persistence","round":1,"claim":"Saved entries survive reopening"}]);
        assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_err());
        let passed = CheckResult {
            claims: vec![Claim::new("Saved entries survive reopening", true, "")],
            ..CheckResult::new("persistence", 1, true, "One stored record returned.")
        };
        workspace.manifest.checks.push(passed.clone());
        workspace.save().unwrap();
        assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_ok());
        workspace.manifest.checks.push(CheckResult { round: 2, ..passed.clone() });
        workspace.save().unwrap();
        assert!(
            call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":1})).is_ok(),
            "same check passing again revalidates its citation"
        );
        workspace.manifest.checks.push(CheckResult { round: 3, passed: false, ..passed });
        workspace.save().unwrap();
        assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":2})).is_err());
        assert_eq!(super::cached(&engine.session()).revision, 2);
    }

    #[test]
    fn legacy_investigation_migrates_and_invalid_or_future_state_never_becomes_empty() {
        let legacy = br#"{"revision":7,"plan":[{"title":"Keep the old plan","status":"doing"}],"handoff":"Keep going"}"#;
        let migrated = super::read_investigation(legacy).unwrap();
        assert_eq!(migrated.version, super::INVESTIGATION_VERSION);
        assert_eq!(migrated.revision, 7);
        assert_eq!(migrated.plan[0].title, "Keep the old plan");
        assert!(migrated.build_plan.is_none());
        let directory = tempfile::tempdir().unwrap();
        let project = Project::create(&directory.path().join("project.cany"), "Test", Purpose::Education).unwrap();
        let root = project.root.clone();
        let mut session = crate::Session::isolated(&directory.path().join("config"));
        session.open_project(project).unwrap();
        let engine = Engine::new(session);
        let path = root.join("investigation/state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut invalid = json!({"version":1,"revision":7,"build_plan":build_plan("Catalogue")});
        invalid["build_plan"]["inputs"][0]["phase_ids"] = json!(["missing"]);
        for bytes in [b"not-json".to_vec(), br#"{"version":999,"revision":7}"#.to_vec(), serde_json::to_vec(&invalid).unwrap()] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(super::load_project_strict(&root).is_err());
            assert!(call(&engine, "core_investigation_show", json!({})).is_err());
            assert!(call(&engine, "core_build_plan_update", json!({"plan":build_plan("Replacement"),"based_on_revision":7})).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let mut missing_evidence = json!({"version":1,"revision":7,"build_plan":build_plan("Catalogue")});
        missing_evidence["build_plan"]["acceptance"][0]["status"] = json!("verified");
        missing_evidence["build_plan"]["acceptance"][0]["evidence_ids"] = json!([format!("ev_{}", "0".repeat(64))]);
        std::fs::write(&path, serde_json::to_vec(&missing_evidence).unwrap()).unwrap();
        assert!(super::load_project_strict(&root).is_err(), "valid-looking citations must also exist");
        assert!(super::load_project_state(&root).is_ok(), "inspection must retain the plan so its citation can be repaired");
        let inspection = call(&engine, "core_investigation_show", json!({})).unwrap();
        assert_eq!(inspection["result"]["revision"], 7);
        assert!(inspection["result"]["build_plan_verification_issue"].as_str().is_some());
        call(&engine, "core_build_plan_update", json!({"plan":build_plan("Catalogue"),"based_on_revision":7})).unwrap();
        assert!(super::load_project_strict(&root).is_ok(), "a revision-aware update can remove an invalid citation without discarding the plan");
        std::fs::File::create(&path).unwrap().set_len(super::MAX_STATE_BYTES as u64 + 1).unwrap();
        assert_eq!(super::load_project_strict(&root).unwrap_err().code, craft_anything_evidence::ErrorCode::OverLimit);
        assert!(call(&engine, "core_build_plan_update", json!({"plan":build_plan("Replacement"),"based_on_revision":8})).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), super::MAX_STATE_BYTES as u64 + 1);
    }

    #[test]
    fn finding_edits_refuse_unbacked_corrections_without_changing_memory() {
        let engine = Engine::new(crate::testing::session());
        assert!(engine.registry().get("core_finding_edit").is_some());
        let before = call(&engine, "core_investigation_show", json!({})).unwrap();
        for evidence_ids in [json!([]), json!(["ev_missing"])] {
            assert!(call(&engine, "core_finding_edit", json!({"finding_id":"fnd_1", "statement":"corrected", "evidence_ids":evidence_ids})).is_err());
        }
        assert_eq!(call(&engine, "core_investigation_show", json!({})).unwrap(), before);
    }

    #[test]
    fn investigation_locks_are_bounded_cancelled_and_released_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = crate::Session::isolated(directory.path());
        session.project = Some(Project::create(&directory.path().join("project.cany"), "Test", Purpose::Education).unwrap());
        let cancel = crate::CancelToken::default();
        let guard = super::UpdateLock::acquire(&session, &cancel, std::time::Duration::ZERO).unwrap();
        assert_eq!(super::UpdateLock::acquire(&session, &cancel, std::time::Duration::ZERO).err().unwrap().code, craft_anything_evidence::ErrorCode::TimedOut);
        cancel.cancel();
        assert_eq!(
            super::UpdateLock::acquire(&session, &cancel, std::time::Duration::from_secs(10)).err().unwrap().code,
            craft_anything_evidence::ErrorCode::Cancelled
        );
        drop(guard);
        assert!(super::UpdateLock::acquire(&session, &crate::CancelToken::default(), std::time::Duration::ZERO).is_ok());
    }

    #[test]
    fn investigation_updates_never_replace_unreadable_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = crate::Session::isolated(directory.path());
        let project = Project::create(&directory.path().join("project.cany"), "Test", Purpose::Education).unwrap();
        let path = project.root.join("investigation/state.json");
        session.open_project(project).unwrap();
        let engine = Engine::new(session);
        call(&engine, "core_brief_write", json!({"question":"Keep this"})).unwrap();
        let rendered = std::fs::read(path.with_file_name("brief.md")).unwrap();
        std::fs::write(&path, b"not-json").unwrap();
        assert_eq!(
            call(&engine, "core_brief_write", json!({"question":"Do not replace"})).unwrap_err().code,
            craft_anything_evidence::ErrorCode::GarbledOutput
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"not-json");
        assert_eq!(std::fs::read(path.with_file_name("brief.md")).unwrap(), rendered);
    }

    #[test]
    fn project_writer_child() {
        let Ok(root) = std::env::var("CRAFT_TEST_INV_PROJECT") else { return };
        let label = std::env::var("CRAFT_TEST_INV_LABEL").unwrap();
        let mode = std::env::var("CRAFT_TEST_INV_MODE").unwrap();
        let root = std::path::PathBuf::from(root);
        let mut session = crate::Session::isolated(&root.join(format!("test-config-{label}")));
        session.open_project(Project::open(&root).unwrap()).unwrap();
        let engine = Engine::new(session);
        if mode == "append" {
            for index in 0..24 {
                call(&engine, "core_hypothesis_record", json!({"claim":format!("{label}-{index}")})).unwrap();
            }
        } else {
            let result = if mode == "build_plan" {
                call(&engine, "core_build_plan_update", json!({"plan":build_plan(&label),"based_on_revision":0}))
            } else {
                call(&engine, "core_plan_update", json!({"steps":[{"title":label,"status":"todo"}],"based_on_revision":0}))
            };
            let outcome = match result {
                Ok(_) => "ok",
                Err(error) if error.code == craft_anything_evidence::ErrorCode::StaleRevision => "stale",
                Err(error) => panic!("{error:?}"),
            };
            std::fs::write(root.join(format!("{label}.result")), outcome).unwrap();
        }
    }

    #[test]
    fn project_writers_serialize_across_processes() {
        let directory = tempfile::tempdir().unwrap();
        for mode in ["append", "revision", "build_plan"] {
            let project = Project::create(&directory.path().join(format!("{mode}.cany")), "Test", Purpose::Education).unwrap();
            let children: Vec<_> = ["first", "second"]
                .into_iter()
                .map(|label| {
                    let root = project.root.clone();
                    std::thread::spawn(move || {
                        craft_anything_platform::Process::new(std::env::current_exe().unwrap())
                            .args(["--exact", "tools::investigation_tools::tests::project_writer_child", "--test-threads=1"])
                            .env("CRAFT_TEST_INV_PROJECT", &root)
                            .env("CRAFT_TEST_INV_LABEL", label)
                            .env("CRAFT_TEST_INV_MODE", mode)
                            .timeout(std::time::Duration::from_secs(30))
                            .stdout_cap(64 * 1024)
                            .stderr_cap(64 * 1024)
                            .run_checked()
                    })
                })
                .collect();
            for child in children {
                child.join().unwrap().unwrap();
            }
            let state: super::Investigation = serde_json::from_slice(&std::fs::read(project.root.join("investigation/state.json")).unwrap()).unwrap();
            if mode == "append" {
                assert_eq!(state.revision, 48);
                assert_eq!(state.hypotheses.len(), 48);
                assert_eq!(state.hypotheses.iter().map(|hypothesis| &hypothesis.id).collect::<std::collections::HashSet<_>>().len(), 48);
            } else {
                assert_eq!(state.revision, 1);
                let mut outcomes: Vec<_> =
                    ["first", "second"].into_iter().map(|label| std::fs::read_to_string(project.root.join(format!("{label}.result"))).unwrap()).collect();
                outcomes.sort();
                assert_eq!(outcomes, ["ok", "stale"]);
                if mode == "build_plan" {
                    assert!(state.build_plan.is_some());
                }
            }
            for (name, body) in super::render(&state) {
                assert_eq!(std::fs::read_to_string(project.root.join("investigation").join(name)).unwrap(), body);
            }
        }
    }

    #[test]
    fn finding_corrections_keep_their_id_and_reject_stale_revisions() {
        use craft_anything_evidence::{Confidence, Environment, EvidenceBuilder, Subject};
        let engine = Engine::new(crate::testing::session());
        let evidence = EvidenceBuilder::new("test-observation", Subject { kind: "test".into(), sha256: None, name: None }, Environment::builtin(true))
            .result(json!({"value": 1}))
            .summary("Synthetic observation")
            .confidence(Confidence::Observed)
            .build()
            .unwrap();
        let evidence_id = evidence.id.clone();
        engine.session().record(evidence).unwrap();
        call(&engine, "core_finding_record", json!({"statement":"initial", "evidence_ids":[evidence_id]})).unwrap();
        let edit = json!({"finding_id":"fnd_1", "statement":"corrected", "evidence_ids":[evidence_id], "confidence":"observed", "based_on_revision":1});
        let result = call(&engine, "core_finding_edit", edit.clone()).unwrap();
        assert_eq!(result["result"]["finding_id"], "fnd_1");
        let investigation = call(&engine, "core_investigation_show", json!({})).unwrap();
        assert_eq!(investigation["result"]["findings"].as_array().unwrap().len(), 1);
        assert_eq!(investigation["result"]["findings"][0]["statement"], "corrected");
        assert_eq!(call(&engine, "core_finding_edit", edit).unwrap_err().code, craft_anything_evidence::ErrorCode::StaleRevision);
        for (confidence, expected) in [("observed", Confidence::Observed), ("derived", Confidence::Derived), ("inferred", Confidence::Inferred)] {
            call(
                &engine,
                "core_finding_edit",
                json!({"finding_id":"fnd_1", "statement":"reclassified", "evidence_ids":[evidence_id], "confidence":confidence}),
            )
            .unwrap();
            assert_eq!(super::cached(&engine.session()).findings[0].confidence, expected);
            call(&engine, "core_finding_edit", json!({"finding_id":"fnd_1", "statement":"clarified", "evidence_ids":[evidence_id]})).unwrap();
            assert_eq!(super::cached(&engine.session()).findings[0].confidence, expected);
        }
    }

    #[test]
    fn shared_projects_preserve_memory_and_refresh_an_external_writers_findings() {
        use craft_anything_evidence::{Confidence, Environment, EvidenceBuilder, Subject};
        let directory = tempfile::tempdir().unwrap();
        let engine = Engine::new(crate::session::Session::isolated(directory.path()));
        let evidence = EvidenceBuilder::new("test-shared", Subject { kind: "test".into(), sha256: None, name: None }, Environment::builtin(true))
            .result(json!({"value": 1}))
            .summary("Synthetic observation")
            .confidence(Confidence::Observed)
            .build()
            .unwrap();
        let id = evidence.id.clone();
        engine.session().record(evidence).unwrap();
        call(&engine, "core_brief_write", json!({"question":"Shared question"})).unwrap();
        let root = engine.session().ensure_shared_project().unwrap();
        assert_eq!(engine.session().ensure_shared_project().unwrap(), root);
        let mut child = crate::session::Session::isolated(&directory.path().join("child"));
        child.open_project(Project::open(&root).unwrap()).unwrap();
        let child = Engine::new(child);
        assert_eq!(call(&child, "core_investigation_show", json!({})).unwrap()["result"]["brief"]["question"], "Shared question");
        call(&child, "core_finding_record", json!({"statement":"Shared finding", "evidence_ids":[id]})).unwrap();
        engine.session().open_project(Project::open(&root).unwrap()).unwrap();
        assert_eq!(super::cached(&engine.session()).findings[0].id, "fnd_1");
        assert_eq!(super::cached(&engine.session()).findings[0].statement, "Shared finding");
        let mut reopened = crate::session::Session::isolated(&directory.path().join("reopened"));
        reopened.open_project(Project::open(&root).unwrap()).unwrap();
        assert_eq!(super::cached(&reopened).findings[0].id, "fnd_1");
    }

    #[test]
    fn shared_targets_are_mode_aware_and_changed_files_are_refused() {
        use crate::target::{Target, TargetContext};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.bin");
        std::fs::write(&path, b"sample").unwrap();
        let target = Target::load(&path).unwrap();
        let context = TargetContext::from_target(&target);
        let mut deep = crate::session::Session::isolated(&directory.path().join("deep"));
        deep.deep_for_run = Some(true);
        deep.restore_targets(std::slice::from_ref(&context)).unwrap();
        assert_eq!(deep.active().unwrap().target.id, target.id);
        assert_eq!(deep.active().unwrap().target.bytes.as_ref(), b"sample");
        std::fs::write(&path, b"changed").unwrap();
        let mut other = crate::session::Session::isolated(&directory.path().join("other"));
        other.deep_for_run = Some(true);
        assert!(other.restore_targets(std::slice::from_ref(&context)).is_err());
        assert!(other.active().is_err());
        std::fs::remove_file(&path).unwrap();
        let mut clean = crate::session::Session::isolated(&directory.path().join("clean"));
        clean.restore_targets(&[context]).unwrap();
        assert_eq!(clean.active().unwrap().target.id, target.id);
        assert!(clean.active().unwrap().target.bytes.is_empty());
    }

    #[test]
    fn memory_round_trip_without_project() {
        let e = Engine::new(crate::testing::session());
        call(&e, "core_brief_write", json!({"question": "How does save work?"})).unwrap();
        call(&e, "core_plan_update", json!({"steps": [{"title": "find save", "status": "doing"}]})).unwrap();
        let h = call(&e, "core_hypothesis_record", json!({"claim": "writes JSON", "prediction": "a .json string near the save path"})).unwrap();
        assert_eq!(h["result"]["hypothesis_id"], "hyp_1");
        call(&e, "core_hypothesis_update", json!({"hypothesis_id": "hyp_1", "status": "supported", "note": "seen"})).unwrap();
        // findings need real evidence
        assert!(call(&e, "core_finding_record", json!({"statement": "x", "evidence_ids": []})).is_err());
        assert!(call(&e, "core_finding_record", json!({"statement": "x", "evidence_ids": ["ev_00"]})).is_err());
        call(&e, "core_handoff_write", json!({"text": "next: check the loader"})).unwrap();
        let inv = call(&e, "core_investigation_show", json!({})).unwrap();
        assert_eq!(inv["result"]["brief"]["question"], "How does save work?");
        assert_eq!(inv["result"]["hypotheses"][0]["status"], "supported");
        assert_eq!(inv["result"]["handoff"], "next: check the loader");
        let rev = inv["result"]["revision"].as_u64().unwrap();
        let err = call(&e, "core_plan_update", json!({"steps": [], "based_on_revision": rev + 7})).unwrap_err();
        assert_eq!(err.code, craft_anything_evidence::ErrorCode::StaleRevision);
        assert!(call(&e, "core_journal_read", json!({})).is_err());
    }

    #[test]
    fn project_files_and_journal() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(&dir.path().join("p.cany"), "p", Purpose::Education).unwrap();
        let mut s = crate::testing::session();
        s.open_project(project).unwrap();
        let e = Engine::new(s);
        call(&e, "core_brief_write", json!({"question": "Q?", "targets": ["/bin/ls"]})).unwrap();
        call(&e, "core_handoff_write", json!({"text": "resume here"})).unwrap();
        let inv_dir = dir.path().join("p.cany").join("investigation");
        for f in ["state.json", "brief.md", "plan.md", "hypotheses.md", "findings.md", "handoff.md"] {
            assert!(inv_dir.join(f).exists(), "{f}");
        }
        assert!(std::fs::read_to_string(inv_dir.join("handoff.md")).unwrap().contains("resume here"));
        let _ = call(&e, "no_such_tool", json!({}));
        let j = call(&e, "core_journal_read", json!({"kind": "tool_call"})).unwrap();
        let entries = j["result"]["entries"].as_array().unwrap();
        assert!(entries.len() >= 3);
        assert_eq!(entries[0]["tool"], "core_brief_write");
        assert_eq!(entries[0]["ok"], true);
        assert!(entries.iter().any(|e| e["tool"] == "no_such_tool" && e["ok"] == false));
        let only = call(&e, "core_journal_read", json!({"tool": "core_handoff_write"})).unwrap();
        assert_eq!(only["result"]["total"], 1);
    }
}
