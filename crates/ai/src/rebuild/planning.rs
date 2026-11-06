//! The saved build plan participates in refinement and completion. Read it from the project,
//! since an external builder's MCP server may be its writer; never treat an unreadable record
//! as an empty, successfully completed plan.

use super::*;
use craft_anything_engine::build_plan::{BuildPlan, PlanBlocker};
use craft_anything_engine::tools::investigation_tools;

/// A missing plan is an unfinished obligation, including for projects created by older builds.
pub(super) const MISSING_PLAN: &str = "The project's phases, required inputs and acceptance criteria have not been recorded yet.";

pub(super) const PLAN_ONLY: &str = "For this first turn, study the available evidence and save the build plan with core_build_plan_update. Do not change application files yet. Include the requested workflows and the missing input inventory, then return a short description of the plan. Craft Anything will start implementation after the plan has been saved. Do not wait for approval of routine implementation choices; ask only for material missing information.";

pub(super) const START_PHASE: &str = "The build plan is saved. Read it with core_investigation_show and implement the next unblocked phase. Keep the phase and acceptance status current, preserve the remaining scope, and exercise the relevant behavior before recording verification. Continue independent phases while required inputs for other phases are unavailable.";

pub(super) fn load(ws: &Workspace) -> Result<Option<BuildPlan>, ToolError> {
    Ok(investigation_tools::load_project_state(&ws.investigation_dir())?.build_plan)
}

#[derive(Debug)]
pub(super) struct Review {
    pub missing: bool,
    pub blockers: Vec<PlanBlocker>,
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
            "The automatic checks have finished, but the project plan is not complete: {}\n\n{}\n\nRead craft.json with build_file_read (or your file reader) for the recorded check IDs, rounds and claim titles before updating acceptance criteria. Cite the check and claim that actually demonstrate a criterion; do not treat a build-only check as proof of an interaction or persistence. Keep unavailable required inputs and untested criteria unresolved. A finished checklist alone cannot make this project Ready.",
            self.explanation(),
            super::brief::BUILD_PLANNING,
        )
    }
}

pub(super) fn review(ws: &Workspace, good_enough: f64) -> Result<Review, ToolError> {
    // Evaluate the same snapshot whose evidence was validated. A second read must never let
    // one writer's plan borrow another writer's evidence validity.
    let (plan, verification_issue) = match investigation_tools::load_project_strict(&ws.investigation_dir()) {
        Ok(investigation) => (investigation.build_plan, None),
        Err(error) => (load(ws)?, Some(error.message)),
    };
    Ok(Review {
        missing: plan.is_none(),
        blockers: plan.map(|p| p.completion_blockers(&ws.manifest.checks, good_enough)).unwrap_or_default(),
        verification_issue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
