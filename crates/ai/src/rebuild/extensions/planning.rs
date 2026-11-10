//! Planning's workflow policy. The host only calls the required-extension interface.
use super::*;
use craft_anything_engine::extensions::planning as engine_planning;

#[derive(Debug)]
pub struct PlanningExtension;

pub(crate) const MISSING_PLAN: &str = "The project's phases, required inputs and acceptance criteria have not been recorded yet.";
pub(crate) const PLAN_ONLY: &str = "For this first turn, study the available evidence and save the build plan with core_build_plan_update. Do not change application files yet. Include the requested workflows and the missing input inventory, then return a short description of the plan. Craft Anything will start implementation after the plan has been saved. Do not wait for approval of routine implementation choices; ask only for material missing information.";
pub(crate) const START_PHASE: &str = "The build plan is saved. Read it with core_build_plan_show and implement the next unblocked phase. Keep phase and acceptance status current, preserve the remaining scope, and exercise relevant behavior before recording verification. Continue independent phases while required inputs for other phases are unavailable.";
const INSTRUCTIONS: &str = "Before changing application files, read core_build_plan_show, then create or revise its plan with core_build_plan_update using its planning revision as based_on_revision. Keep the person's goal and requested capabilities, with stable phase and criterion IDs. Record concrete phases, dependencies, required inputs and observable acceptance, including requested subpages, meaningful screen states, navigation, data changes and persistence. List missing inputs with the question, why it matters, a useful format, affected phases and whether each is required. Preserve all phases and required scope across turns. Reuse supplied material before asking with core_person_ask; routine implementation choices do not need approval. Keep unavailable required inputs and untested criteria unresolved, and continue independent work. A completed phase, passing build, screenshot or existing link alone does not verify interactions and persistence: exercise the requested behavior and cite real observed/derived evidence or exact recorded check references. Keep implemented and verified work separate. Before finishing, update the durable plan and explain what remains unverified. The planning extension owns this record; provider-specific todo lists do not replace it.";

pub(crate) fn load(ws: &Workspace) -> Result<Option<craft_anything_engine::build_plan::BuildPlan>, ToolError> {
    Ok(engine_planning::load_project_state(&ws.investigation_dir())?.plan)
}

impl WorkflowExtension for PlanningExtension {
    fn id(&self) -> &'static str {
        "planning"
    }
    fn instructions(&self) -> &'static str {
        INSTRUCTIONS
    }
    fn prepare(&self, ws: &Workspace) -> Result<Option<Preparation>, ToolError> {
        let state = engine_planning::load_project_state(&ws.investigation_dir())?;
        Ok(state.plan.is_none().then(|| Preparation {
            progress: "Planning phases and needed inputs…".into(),
            prompt: PLAN_ONLY.into(),
            next_prompt: START_PHASE.into(),
            missing: MISSING_PLAN.into(),
            correction: Some(format!(
                "The build plan was not saved: Craft Anything checked the project's durable planning state after your reply and it still has no plan. A prose claim, simulated observation or provider checklist does not save it. Call core_build_plan_update with the complete plan and based_on_revision: {}. Wait for its real tool result; if it fails, correct the reported error and call it again. Include phases, inputs and acceptance with unresolved work still pending. Do not change application files or claim success before the tool confirms the save.",
                state.revision
            )),
            correction_tool: Some("core_build_plan_update"),
        }))
    }
    fn assess(&self, ws: &Workspace, good_enough: f64) -> Result<Assessment, ToolError> {
        let review = review(ws, good_enough)?;
        if review.complete() { Ok(Assessment::ready()) } else { Ok(Assessment::blocked(review.explanation(), review.refinement_prompt())) }
    }
    fn assess_refinement(&self, ws: &Workspace, good_enough: f64, unrefinable_checks: &[String]) -> Result<Assessment, ToolError> {
        let review = review(ws, good_enough)?;
        if review.complete() {
            return Ok(Assessment::ready());
        }
        if !unrefinable_checks.is_empty()
            && review.verification_issue.is_none()
            && let Some(plan) = review.plan.as_ref()
            && only_unavailable_checks(plan, &review.blockers, &ws.manifest.checks, good_enough, unrefinable_checks)
        {
            return Ok(Assessment::blocked_without_refinement(review.explanation()));
        }
        Ok(Assessment::blocked(review.explanation(), review.refinement_prompt()))
    }
}

/// A previously verified criterion may become unverifiable when its checker loses an oracle.
/// Only that case is non-actionable: unfinished phases, pending criteria, missing history and
/// other extension obligations still get their own refinement opportunity.
fn only_unavailable_checks(
    plan: &craft_anything_engine::build_plan::BuildPlan,
    blockers: &[craft_anything_engine::build_plan::PlanBlocker],
    checks: &[CheckResult],
    good_enough: f64,
    unrefinable: &[String],
) -> bool {
    use craft_anything_engine::build_plan::{AcceptanceStatus, BlockerKind};
    let Some(latest) = checks.iter().map(|check| check.round).max() else { return false };
    !blockers.is_empty()
        && blockers.iter().all(|blocker| {
            if blocker.kind != BlockerKind::AcceptanceUnverified {
                return false;
            }
            let Some(criterion) = plan.acceptance.iter().find(|criterion| criterion.id == blocker.item_id) else { return false };
            criterion.status == AcceptanceStatus::Verified
                && !criterion.checks.is_empty()
                && criterion.checks.iter().all(|reference| {
                    if reference.verify(checks, good_enough).is_ok() {
                        return true;
                    }
                    let original = checks.iter().filter(|check| check.round == reference.round).cloned().collect::<Vec<_>>();
                    reference.verify(&original, 0.0).is_ok()
                        && unrefinable.contains(&reference.check)
                        && checks.iter().any(|check| check.round == latest && check.check == reference.check && !check.good(good_enough))
                })
        })
}

#[derive(Debug)]
pub(crate) struct Review {
    pub missing: bool,
    plan: Option<craft_anything_engine::build_plan::BuildPlan>,
    pub blockers: Vec<craft_anything_engine::build_plan::PlanBlocker>,
    pub verification_issue: Option<String>,
}

impl Review {
    pub fn complete(&self) -> bool {
        !self.missing && self.blockers.is_empty() && self.verification_issue.is_none()
    }
    pub fn explanation(&self) -> String {
        if self.missing {
            return MISSING_PLAN.into();
        }
        let mut reasons: Vec<&str> = self.verification_issue.as_deref().into_iter().chain(self.blockers.iter().take(8).map(|b| b.message.as_str())).collect();
        if self.blockers.len() > 8 {
            reasons.push("More unfinished requirements are listed in the project plan.");
        }
        reasons.join(" ")
    }
    pub fn refinement_prompt(&self) -> String {
        format!(
            "The automatic checks have finished, but required extension obligations remain: {}\n\n{}\n\nRead craft.json for actual check IDs, rounds and claim titles before updating acceptance. A finished checklist alone cannot make this project Ready.",
            self.explanation(),
            INSTRUCTIONS
        )
    }
}

pub(crate) fn review(ws: &Workspace, good_enough: f64) -> Result<Review, ToolError> {
    let (plan, verification_issue) = match engine_planning::load_project_strict(&ws.investigation_dir()) {
        Ok(state) => (state.plan, None),
        Err(error) => (load(ws)?, Some(error.message)),
    };
    Ok(Review {
        missing: plan.is_none(),
        blockers: plan.as_ref().map(|plan| plan.completion_blockers(&ws.manifest.checks, good_enough)).unwrap_or_default(),
        plan,
        verification_issue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_anything_engine::workspace::{TargetRef, Workspace};

    #[test]
    fn missing_and_corrupt_plans_cannot_count_as_complete() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::create(dir.path(), "Notebook", "Build a notebook", TargetRef::None).unwrap();
        let result = review(&ws, 90.0).unwrap();
        assert!(result.missing && !result.complete());
        assert!(result.refinement_prompt().contains("core_build_plan_update"));
        let state = ws.investigation_dir().join("investigation/state.json");
        std::fs::create_dir_all(state.parent().unwrap()).unwrap();
        std::fs::write(state, b"{broken").unwrap();
        assert!(review(&ws, 90.0).is_err(), "an unreadable plan is never treated as completed");
    }
}
