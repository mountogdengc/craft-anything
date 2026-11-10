//! Engine adapter for the native planning extension: tools, evidence and session state.
use crate::build_plan::{AcceptanceStatus, BuildPlan, RecordedCheck};
use crate::evidence::{Confidence, Evidence, ToolError};
use crate::tool::{Ctx, ToolOutput, parse_params};
use craft_anything_planning::store::{self, State};
use craft_anything_platform::sync::Mutex;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, PoisonError};

type Scratch = Mutex<State>;

mod tools;
pub use craft_anything_planning::store::schema;
pub use tools::specs;
#[cfg(test)]
pub(crate) mod tests;

/// Read-only projection for tools and compatibility views. Structural errors fail; stale
/// evidence is exposed for inspection and remains a blocker in strict completion queries.
pub fn view(session: &crate::Session) -> Result<Value, ToolError> {
    let state = load(session)?;
    let verification_issue = state.plan.as_ref().and_then(|plan| validate_evidence(plan, |id| session.resolve_evidence(id)).err()).map(|e| e.message);
    Ok(json!({"version":state.version,"revision":state.revision,"plan":state.plan,"verification_issue":verification_issue}))
}

pub(crate) fn save_scratch_project(session: &crate::Session) -> Result<(), ToolError> {
    let Some(project) = &session.project else { return Ok(()) };
    let Some(slot) = session.ext.get::<Scratch>() else { return Ok(()) };
    let state = slot.lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(plan) = &state.plan {
        validate_evidence(plan, |id| project.load_evidence(id))?;
    }
    store::import_session_locked(&project.root, &state, &|| false)
}

pub(crate) fn append_investigation_view(session: &crate::Session, body: &mut Value) {
    let (plan, revision, issue) = match view(session) {
        Ok(view) => (
            view.get("plan").cloned().unwrap_or(Value::Null),
            view.get("revision").cloned().unwrap_or(Value::Null),
            view.get("verification_issue").cloned().unwrap_or(Value::Null),
        ),
        Err(error) => (Value::Null, Value::Null, Value::String(error.message)),
    };
    if let Some(body) = body.as_object_mut() {
        body.insert("build_plan".into(), plan);
        body.insert("build_plan_revision".into(), revision);
        body.insert("build_plan_verification_issue".into(), issue);
    }
}

impl RecordedCheck for crate::workspace::CheckResult {
    fn check_id(&self) -> &str {
        &self.check
    }
    fn round(&self) -> u32 {
        self.round
    }
    fn good(&self, threshold: f64) -> bool {
        self.good(threshold)
    }
    fn claim_holds(&self, title: &str) -> Option<bool> {
        let mut claims = self.claims.iter().filter(|claim| claim.title == title).peekable();
        claims.peek()?;
        Some(claims.all(|claim| claim.passed))
    }
}

pub fn load_project_state(root: &Path) -> Result<State, ToolError> {
    store::load(root)
}

pub fn load_project_strict(root: &Path) -> Result<State, ToolError> {
    let state = load_project_state(root)?;
    if let Some(plan) = &state.plan
        && (plan.inputs.iter().any(|input| !input.evidence_ids.is_empty()) || plan.acceptance.iter().any(|criterion| !criterion.evidence_ids.is_empty()))
    {
        let project = crate::project::Project::open(root)?;
        validate_evidence(plan, |id| project.load_evidence(id))?;
    }
    Ok(state)
}

pub fn load(session: &crate::Session) -> Result<State, ToolError> {
    match &session.project {
        Some(project) => load_project_state(&project.root),
        None => Ok(session.ext.get::<Scratch>().map(|slot| slot.lock().unwrap_or_else(PoisonError::into_inner).clone()).unwrap_or_default()),
    }
}

pub(crate) fn load_session_strict(session: &crate::Session) -> Result<State, ToolError> {
    if let Some(project) = &session.project {
        return load_project_strict(&project.root);
    }
    let state = load(session)?;
    if let Some(plan) = &state.plan {
        validate_evidence(plan, |id| session.resolve_evidence(id))?;
    }
    Ok(state)
}

fn validate_evidence(plan: &BuildPlan, mut resolve: impl FnMut(&str) -> Result<Evidence, ToolError>) -> Result<(), ToolError> {
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildPlanParams {
    pub plan: BuildPlan,
    /// Read the independent planning revision with core_build_plan_show.
    pub based_on_revision: u64,
}

pub(crate) fn update(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let params: BuildPlanParams = parse_params(params)?;
    let workspace = crate::tools::interaction_tools::inbox(ctx).as_ref().and_then(crate::tools::interaction_tools::workspace_of);
    let validate = |plan: &BuildPlan| {
        validate_evidence(plan, |id| ctx.session.resolve_evidence(id))?;
        for criterion in &plan.acceptance {
            for reference in &criterion.checks {
                let workspace = workspace.as_ref().ok_or_else(|| ToolError::unavailable("Check citations need the build workspace that recorded them."))?;
                reference.verify(&workspace.manifest.checks, 0.0)?;
            }
        }
        Ok(())
    };
    let state = if let Some(project) = &ctx.session.project {
        store::update(&project.root, params.based_on_revision, params.plan, &|| ctx.cancel.is_cancelled(), validate)?
    } else {
        let previous = load(ctx.session)?;
        if previous.revision != params.based_on_revision {
            return Err(store::stale(previous.revision, params.based_on_revision));
        }
        params.plan.validate()?;
        if let Some(before) = &previous.plan {
            params.plan.validate_scope_update(before)?;
        }
        validate(&params.plan)?;
        let revision = previous
            .revision
            .checked_add(1)
            .ok_or_else(|| ToolError::new(crate::evidence::ErrorCode::OverLimit, "The planning revision cannot advance further."))?;
        let state = State { revision, plan: Some(params.plan), ..State::default() };
        ctx.session.ext.insert(Arc::new(Mutex::new(state.clone())));
        state
    };
    // Keep the old Rust projection current without writing a plan into the analysis record.
    let mut compatibility = crate::tools::investigation_tools::cached(ctx.session);
    compatibility.build_plan = state.plan.clone();
    crate::tools::investigation_tools::cache(ctx.session, compatibility);
    Ok(ToolOutput::Value(json!({"version":state.version,"revision":state.revision,"plan":state.plan})))
}
