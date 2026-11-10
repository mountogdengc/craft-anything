//! Human-readable projection of the canonical planning state.
use crate::BuildPlan;

pub fn markdown(build: &BuildPlan) -> String {
    let mut plan = format!("# Build plan\n\n{}\n\n## Phases\n\n", build.goal);
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
    plan
}
