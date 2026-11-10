//! A durable build plan, kept in the investigation beside its evidence.
//!
//! Implementation progress and recorded verification are separate. A completed phase does not
//! verify its acceptance criteria, and supplying an input never completes unrelated work.

/// The host's recorded check, without coupling planning policy to an engine implementation.
pub trait RecordedCheck {
    fn check_id(&self) -> &str;
    fn round(&self) -> u32;
    fn good(&self, good_enough: f64) -> bool;
    /// None means this result has no such claim; false includes any failed duplicate claim.
    fn claim_holds(&self, title: &str) -> Option<bool>;
}

use craft_anything_evidence::{ToolError, is_evidence_id};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub const BUILD_PLAN_VERSION: u32 = 1;
const MAX_PHASES: usize = 50;
const MAX_INPUTS: usize = 100;
const MAX_ACCEPTANCE: usize = 200;
const MAX_CITATIONS: usize = 32;

fn version() -> u32 {
    BUILD_PLAN_VERSION
}

fn required() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildPlan {
    #[serde(default = "version")]
    pub version: u32,
    /// The requested outcome. Keep the person's scope when revising the plan.
    pub goal: String,
    pub phases: Vec<BuildPhase>,
    #[serde(default)]
    pub inputs: Vec<BuildInput>,
    /// Concrete outcomes to check, including interactions and persistence where requested.
    pub acceptance: Vec<AcceptanceCriterion>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    #[default]
    Todo,
    Doing,
    Done,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildPhase {
    pub id: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub status: PhaseStatus,
    /// Stable ids of phases whose implementation this phase needs.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputStatus {
    #[default]
    Needed,
    Requested,
    Provided,
    /// Still unavailable. Deferring a required input does not resolve its blocker.
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildInput {
    pub id: String,
    pub question: String,
    pub why: String,
    /// What the person can supply, such as a choice, a screenshot or a sample file.
    pub format: String,
    pub phase_ids: Vec<String>,
    #[serde(default = "required")]
    pub required: bool,
    #[serde(default)]
    pub status: InputStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

impl BuildInput {
    pub fn provided(&self) -> bool {
        self.status == InputStatus::Provided && (self.response.as_ref().is_some_and(|s| !s.trim().is_empty()) || !self.evidence_ids.is_empty())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceStatus {
    #[default]
    Pending,
    Implemented,
    /// The agent has recorded supporting evidence or check results, validated when saved.
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub phase_id: String,
    pub description: String,
    #[serde(default = "required")]
    pub required: bool,
    #[serde(default)]
    pub status: AcceptanceStatus,
    /// Observed or derived evidence that demonstrates this outcome in the built project.
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub checks: Vec<CheckReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckReference {
    /// A check id actually recorded in craft.json, not a command or a proposed test.
    pub check: String,
    pub round: u32,
    /// If present, the exact title of a passing claim within that check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<String>,
}

impl CheckReference {
    /// A citation must name a real passing result, and the latest round must still support the
    /// same check and claim. A passing repeat revalidates it without rewriting the plan; a later
    /// missing or failed result cannot retain the earlier verification.
    pub fn verify<C: RecordedCheck>(&self, checks: &[C], good_enough: f64) -> Result<(), ToolError> {
        // A changed fallback threshold applies to the latest result. The citation's original
        // result must have succeeded on its own terms, not satisfy a later run's higher bar.
        self.verify_round(checks, self.round, 0.0)?;
        let Some(latest) = checks.iter().map(|check| check.round()).max() else {
            return Err(ToolError::invalid("No checks have been recorded."));
        };
        self.verify_round(checks, latest, good_enough)?;
        Ok(())
    }

    fn verify_round<C: RecordedCheck>(&self, checks: &[C], round: u32, good_enough: f64) -> Result<(), ToolError> {
        let matches: Vec<_> = checks.iter().filter(|check| check.round() == round && check.check_id() == self.check).collect();
        if matches.is_empty() || matches.iter().any(|check| !check.good(good_enough)) {
            return Err(ToolError::invalid(format!("Check {} round {round} has no passing result that meets its checks.", self.check)));
        }
        if let Some(title) = &self.claim {
            let claims: Vec<_> = matches.iter().filter_map(|check| check.claim_holds(title)).collect();
            if claims.is_empty() || claims.iter().any(|holds| !holds) {
                return Err(ToolError::invalid(format!("Check {} round {round} has no passing claim named {title:?}.", self.check)));
            }
        }
        Ok(())
    }
}

impl AcceptanceCriterion {
    /// Evidence ids are resolved by the update tool. This pure view also rechecks citations
    /// against current results, so a later failure cannot retain an earlier verification.
    pub fn verified<C: RecordedCheck>(&self, checks: &[C], good_enough: f64) -> bool {
        self.status == AcceptanceStatus::Verified
            && (!self.evidence_ids.is_empty() || !self.checks.is_empty())
            && self.checks.iter().all(|reference| reference.verify(checks, good_enough).is_ok())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockerKind {
    InvalidPlan,
    PhaseIncomplete,
    DependencyIncomplete,
    MissingInput,
    AcceptanceUnverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanBlocker {
    pub kind: BlockerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    pub item_id: String,
    pub message: String,
}

impl PlanBlocker {
    pub fn message(&self) -> &str {
        &self.message
    }

    fn invalid(error: impl fmt::Display) -> Self {
        Self { kind: BlockerKind::InvalidPlan, phase_id: None, item_id: String::new(), message: error.to_string() }
    }
}

impl fmt::Display for PlanBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl BuildPlan {
    pub fn validate(&self) -> Result<(), ToolError> {
        if self.version != BUILD_PLAN_VERSION {
            return Err(ToolError::unsupported(format!(
                "Build plan version {} is not supported; this build reads version {BUILD_PLAN_VERSION}.",
                self.version
            ))
            .with_remediation("Update Craft Anything before changing this plan."));
        }
        text(&self.goal, "The build goal", 8_192)?;
        count(self.phases.len(), "phases", 1, MAX_PHASES)?;
        count(self.inputs.len(), "inputs", 0, MAX_INPUTS)?;
        count(self.acceptance.len(), "acceptance criteria", 1, MAX_ACCEPTANCE)?;
        let phase_ids = ids(self.phases.iter().map(|phase| phase.id.as_str()), "phase")?;
        ids(self.inputs.iter().map(|input| input.id.as_str()), "input")?;
        ids(self.acceptance.iter().map(|criterion| criterion.id.as_str()), "acceptance criterion")?;
        for phase in &self.phases {
            text(&phase.title, "A phase title", 256)?;
            text(&phase.goal, "A phase goal", 2_048)?;
            count(phase.depends_on.len(), "phase dependencies", 0, MAX_PHASES)?;
            let dependencies = ids(phase.depends_on.iter().map(String::as_str), "dependency")?;
            if dependencies.contains(phase.id.as_str()) || !dependencies.is_subset(&phase_ids) {
                return Err(ToolError::invalid(format!("Phase {} has an unknown dependency or depends on itself.", phase.id)));
            }
        }
        self.validate_dependencies()?;
        for input in &self.inputs {
            text(&input.question, "An input question", 2_048)?;
            text(&input.why, "An input's purpose", 2_048)?;
            text(&input.format, "An input's expected format", 512)?;
            count(input.phase_ids.len(), "input phases", 1, MAX_PHASES)?;
            if !ids(input.phase_ids.iter().map(String::as_str), "input phase")?.is_subset(&phase_ids) {
                return Err(ToolError::invalid(format!("Input {} names an unknown phase.", input.id)));
            }
            if let Some(response) = &input.response {
                text(response, "An input response", 8_192)?;
            }
            evidence_ids(&input.evidence_ids)?;
            if input.status == InputStatus::Provided && !input.provided() {
                return Err(ToolError::invalid(format!("Provided input {} needs a response or an evidence id.", input.id)));
            }
        }
        for criterion in &self.acceptance {
            text(&criterion.description, "An acceptance criterion", 4_096)?;
            if !phase_ids.contains(criterion.phase_id.as_str()) {
                return Err(ToolError::invalid(format!("Acceptance criterion {} names an unknown phase.", criterion.id)));
            }
            evidence_ids(&criterion.evidence_ids)?;
            count(criterion.checks.len(), "check citations", 0, MAX_CITATIONS)?;
            let mut references = BTreeSet::new();
            for reference in &criterion.checks {
                text(&reference.check, "A check id", 256)?;
                if let Some(claim) = &reference.claim {
                    text(claim, "A check claim", 4_096)?;
                }
                if !references.insert(reference) {
                    return Err(ToolError::invalid(format!("Acceptance criterion {} repeats a check citation.", criterion.id)));
                }
            }
            if criterion.status == AcceptanceStatus::Verified && criterion.evidence_ids.is_empty() && criterion.checks.is_empty() {
                return Err(ToolError::invalid(format!("Verified acceptance criterion {} needs recorded evidence or a check citation.", criterion.id)));
            }
        }
        Ok(())
    }

    /// Progress updates may refine wording and attach proof, but cannot silently shrink the
    /// required scope. Keep resolved inputs and criteria in the plan. Explicit acceptance of a
    /// partial build is a separate project action, not a way for the builder to rewrite scope.
    pub fn validate_scope_update(&self, previous: &BuildPlan) -> Result<(), ToolError> {
        for phase in &previous.phases {
            if !self.phases.iter().any(|next| next.id == phase.id) {
                return Err(ToolError::invalid(format!("Phase {} must remain in the build plan.", phase.id))
                    .with_remediation("Keep the phase and update its progress instead of removing unfinished work."));
            }
        }
        for criterion in previous.acceptance.iter().filter(|criterion| criterion.required) {
            let retained = self.acceptance.iter().any(|next| next.id == criterion.id && next.required && next.phase_id == criterion.phase_id);
            if !retained {
                return Err(ToolError::invalid(format!(
                    "Required acceptance criterion {} must remain required in phase {}.",
                    criterion.id, criterion.phase_id
                ))
                .with_remediation("Keep the criterion and its phase; update its progress and evidence instead of removing or moving it."));
            }
        }
        for input in previous.inputs.iter().filter(|input| input.required) {
            if !self.inputs.iter().any(|next| next.id == input.id && next.required) {
                return Err(ToolError::invalid(format!("Required input {} must remain in the build plan.", input.id))
                    .with_remediation("Keep the input and record its response or evidence; deferring it does not remove the requirement."));
            }
        }
        Ok(())
    }

    fn validate_dependencies(&self) -> Result<(), ToolError> {
        let mut reached = BTreeSet::new();
        while reached.len() < self.phases.len() {
            let before = reached.len();
            for phase in &self.phases {
                if phase.depends_on.iter().all(|id| reached.contains(id.as_str())) {
                    reached.insert(phase.id.as_str());
                }
            }
            if reached.len() == before {
                return Err(ToolError::invalid("Build phases contain a dependency cycle."));
            }
        }
        Ok(())
    }

    /// Work may proceed in an independent phase while another awaits information. Dependencies
    /// are traversed without recursion so a missing upstream input cannot disappear behind a
    /// phase manually marked done.
    pub fn phase_blockers(&self, phase_id: &str) -> Vec<PlanBlocker> {
        if let Err(error) = self.validate() {
            return vec![PlanBlocker::invalid(error.message)];
        }
        if !self.phases.iter().any(|phase| phase.id == phase_id) {
            return vec![PlanBlocker::invalid(format!("Unknown build phase {phase_id}."))];
        }
        let mut blockers = Vec::new();
        let mut visited = BTreeSet::new();
        let mut queue = vec![phase_id];
        let mut missing = BTreeSet::new();
        while let Some(id) = queue.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(phase) = self.phases.iter().find(|phase| phase.id == id) else { continue };
            if (id != phase_id && phase.status != PhaseStatus::Done) || (id == phase_id && phase.status == PhaseStatus::Blocked) {
                blockers.push(PlanBlocker {
                    kind: BlockerKind::DependencyIncomplete,
                    phase_id: Some(phase_id.into()),
                    item_id: phase.id.clone(),
                    message: format!("Phase {} is {}.", phase.title, if id == phase_id { "marked blocked" } else { "a dependency that is not done" }),
                });
            }
            for input in self.inputs.iter().filter(|input| input.required && !input.provided() && input.phase_ids.iter().any(|phase| phase == id)) {
                if missing.insert(input.id.as_str()) {
                    blockers.push(PlanBlocker {
                        kind: BlockerKind::MissingInput,
                        phase_id: Some(phase_id.into()),
                        item_id: input.id.clone(),
                        message: format!("Required input is missing: {}", input.question),
                    });
                }
            }
            queue.extend(phase.depends_on.iter().map(String::as_str));
        }
        blockers
    }

    /// Required scope remains unfinished until phases, inputs and acceptance are all resolved.
    /// This evaluates recorded verification; it does not run a workflow or invent a test result.
    pub fn completion_blockers<C: RecordedCheck>(&self, checks: &[C], good_enough: f64) -> Vec<PlanBlocker> {
        if let Err(error) = self.validate() {
            return vec![PlanBlocker::invalid(error.message)];
        }
        let mut blockers = Vec::new();
        for phase in self.phases.iter().filter(|phase| phase.status != PhaseStatus::Done) {
            blockers.push(PlanBlocker {
                kind: BlockerKind::PhaseIncomplete,
                phase_id: Some(phase.id.clone()),
                item_id: phase.id.clone(),
                message: format!("Phase is not done: {}", phase.title),
            });
        }
        for input in self.inputs.iter().filter(|input| input.required && !input.provided()) {
            blockers.push(PlanBlocker {
                kind: BlockerKind::MissingInput,
                phase_id: input.phase_ids.first().cloned(),
                item_id: input.id.clone(),
                message: format!("Required input is missing: {}", input.question),
            });
        }
        for criterion in self.acceptance.iter().filter(|criterion| criterion.required && !criterion.verified(checks, good_enough)) {
            let reason = criterion.checks.iter().find_map(|reference| reference.verify(checks, good_enough).err()).map(|error| format!(" {}", error.message));
            blockers.push(PlanBlocker {
                kind: BlockerKind::AcceptanceUnverified,
                phase_id: Some(criterion.phase_id.clone()),
                item_id: criterion.id.clone(),
                message: format!("Acceptance is not verified: {}{}", criterion.description, reason.unwrap_or_default()),
            });
        }
        blockers
    }
}

fn count(value: usize, label: &str, min: usize, max: usize) -> Result<(), ToolError> {
    if !(min..=max).contains(&value) {
        return Err(ToolError::invalid(format!("A build plan needs {min} to {max} {label}.")));
    }
    Ok(())
}

fn text(value: &str, label: &str, max: usize) -> Result<(), ToolError> {
    if value.trim().is_empty() || value.len() > max {
        return Err(ToolError::invalid(format!("{label} must be nonempty and at most {max} bytes.")));
    }
    Ok(())
}

fn ids<'a>(values: impl Iterator<Item = &'a str>, label: &str) -> Result<BTreeSet<&'a str>, ToolError> {
    let mut seen = BTreeSet::new();
    for id in values {
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-')) {
            return Err(ToolError::invalid(format!("A {label} id must contain 1 to 64 ASCII letters, digits, underscores or hyphens.")));
        }
        if !seen.insert(id) {
            return Err(ToolError::invalid(format!("Duplicate {label} id {id}.")));
        }
    }
    Ok(seen)
}

fn evidence_ids(values: &[String]) -> Result<(), ToolError> {
    count(values.len(), "evidence citations", 0, MAX_CITATIONS)?;
    let mut seen = BTreeSet::new();
    for id in values {
        if !is_evidence_id(id) || !seen.insert(id) {
            return Err(ToolError::invalid("Evidence citations must be distinct evidence ids returned by tools."));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use self::test_support::{CheckResult, Claim};
    use super::*;

    fn plan() -> BuildPlan {
        serde_json::from_value(serde_json::json!({
            "goal":"Build a persistent catalogue",
            "phases":[
                {"id":"data","title":"Catalogue data","goal":"Choose and store the catalogue"},
                {"id":"navigation","title":"Navigation","goal":"Implement page navigation"},
                {"id":"detail","title":"Detail pages","goal":"Show stored entries","depends_on":["data"]}
            ],
            "inputs":[{"id":"source","question":"Which sample data?","why":"Determines fields","format":"A sample record","phase_ids":["data"]}],
            "acceptance":[{"id":"reload","phase_id":"detail","description":"Entries survive a reload"}]
        }))
        .unwrap()
    }

    #[test]
    fn required_input_blocks_only_its_phase_and_dependants() {
        let mut plan = plan();
        assert!(plan.validate().is_ok());
        assert_eq!(plan.phase_blockers("data").len(), 1);
        assert!(plan.phase_blockers("navigation").is_empty());
        assert!(plan.phase_blockers("detail").iter().any(|blocker| blocker.item_id == "source"));
        plan.phases[0].status = PhaseStatus::Done;
        assert!(plan.phase_blockers("detail").iter().any(|blocker| blocker.item_id == "source"), "a done checkbox cannot hide missing input");
        plan.inputs[0].required = false;
        assert!(plan.phase_blockers("data").is_empty());
        assert!(plan.phase_blockers("detail").is_empty());
        assert!(!plan.completion_blockers(&[] as &[CheckResult], 90.0).iter().any(|blocker| blocker.kind == BlockerKind::MissingInput));
    }

    #[test]
    fn supplied_input_does_not_complete_or_verify_work() {
        let mut plan = plan();
        plan.inputs[0].status = InputStatus::Provided;
        assert!(plan.validate().is_err(), "provided needs an answer or evidence");
        plan.inputs[0].response = Some("Use the synthetic sample.".into());
        assert!(plan.validate().is_ok());
        assert!(plan.phase_blockers("data").is_empty());
        assert!(plan.completion_blockers(&[] as &[CheckResult], 90.0).iter().any(|blocker| blocker.kind == BlockerKind::AcceptanceUnverified));
        assert_eq!(plan.phases[0].status, PhaseStatus::Todo);
        plan.inputs[0].status = InputStatus::Deferred;
        assert!(!plan.phase_blockers("data").is_empty(), "deferral does not satisfy a required input");
    }

    #[test]
    fn invalid_links_cycles_and_unbacked_verification_are_refused() {
        let mut invalid = plan();
        invalid.phases[0].depends_on = vec!["detail".into()];
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.phases[1].id = "data".into();
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.inputs[0].phase_ids = vec!["missing".into()];
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.acceptance[0].phase_id = "missing".into();
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.acceptance[0].status = AcceptanceStatus::Verified;
        assert!(invalid.validate().is_err());
        assert_eq!(invalid.completion_blockers(&[] as &[CheckResult], 90.0)[0].kind, BlockerKind::InvalidPlan);
        let mut invalid = plan();
        invalid.phases.clear();
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.acceptance.clear();
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.goal = " ".into();
        assert!(invalid.validate().is_err());
        let mut invalid = plan();
        invalid.version = BUILD_PLAN_VERSION + 1;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn recorded_checks_must_still_pass_in_the_latest_round() {
        let mut plan = plan();
        for phase in &mut plan.phases {
            phase.status = PhaseStatus::Done;
        }
        plan.inputs.clear();
        let criterion = &mut plan.acceptance[0];
        criterion.status = AcceptanceStatus::Verified;
        criterion.checks.push(CheckReference { check: "persistence".into(), round: 2, claim: Some("Entries survive reload".into()) });
        assert!(plan.validate().is_ok());
        assert!(!plan.completion_blockers(&[] as &[CheckResult], 90.0).is_empty());
        let passed = CheckResult {
            claims: vec![Claim::new("Entries survive reload", true, "")],
            ..CheckResult::new("persistence", 2, true, "Stored entries found after reload.")
        };
        assert!(plan.completion_blockers(std::slice::from_ref(&passed), 90.0).is_empty());
        let failed = CheckResult { claims: vec![Claim::new("Entries survive reload", false, "The list is empty")], ..passed.clone() };
        assert!(!plan.completion_blockers(&[passed.clone(), failed], 90.0).is_empty());
        let newer = CheckResult { round: 3, ..passed.clone() };
        assert!(plan.completion_blockers(&[passed.clone(), newer.clone()], 90.0).is_empty(), "a passing repeat revalidates the citation");
        let missing_claim = CheckResult { claims: vec![], ..newer.clone() };
        assert!(!plan.completion_blockers(&[passed.clone(), missing_claim], 90.0).is_empty());
        let newer_failed = CheckResult { passed: false, ..newer };
        assert!(!plan.completion_blockers(&[passed.clone(), newer_failed], 90.0).is_empty());
        let missing_check = CheckResult::new("build", 3, true, "New round without the cited check.");
        assert!(!plan.completion_blockers(&[passed.clone(), missing_check], 90.0).is_empty());
        let wrong_claim = CheckResult { claims: vec![Claim::new("The page has a heading", true, "")], ..passed };
        assert!(!plan.completion_blockers(&[wrong_claim], 90.0).is_empty());
        plan.acceptance[0].required = false;
        assert!(plan.completion_blockers(&[] as &[CheckResult], 90.0).is_empty(), "optional acceptance must not block completion");
    }

    #[test]
    fn plan_round_trips_and_bounds_are_enforced() {
        let plan = plan();
        assert_eq!(serde_json::from_slice::<BuildPlan>(&serde_json::to_vec(&plan).unwrap()).unwrap(), plan);
        let mut large = plan.clone();
        large.goal = "x".repeat(8_193);
        assert!(large.validate().is_err());
        let mut large = plan.clone();
        large.phases = (0..=MAX_PHASES).map(|i| BuildPhase { id: format!("phase_{i}"), ..plan.phases[0].clone() }).collect();
        assert!(large.validate().is_err());
        let mut duplicate = plan;
        duplicate.inputs[0].phase_ids.push("data".into());
        assert!(duplicate.validate().is_err());
    }

    #[test]
    fn progress_updates_preserve_required_scope_but_can_remove_optional_records() {
        let mut previous = plan();
        previous.acceptance.push(AcceptanceCriterion { id: "optional-polish".into(), required: false, ..previous.acceptance[0].clone() });
        previous.inputs.push(BuildInput { id: "optional-art".into(), required: false, ..previous.inputs[0].clone() });
        let mut next = previous.clone();
        next.acceptance.retain(|criterion| criterion.required);
        next.inputs.retain(|input| input.required);
        next.inputs[0].status = InputStatus::Provided;
        next.inputs[0].response = Some("Use the synthetic record.".into());
        next.acceptance[0].status = AcceptanceStatus::Implemented;
        next.acceptance[0].description = "Saved entries remain after reopening the app.".into();
        assert!(next.validate().is_ok());
        assert!(next.validate_scope_update(&previous).is_ok());
        let mut omitted_phase = next.clone();
        omitted_phase.phases.retain(|phase| phase.id != "navigation");
        assert!(omitted_phase.validate_scope_update(&previous).is_err(), "a phase without a required criterion still belongs to the requested scope");
        let mut omitted = next.clone();
        omitted.inputs.clear();
        assert!(omitted.validate_scope_update(&previous).is_err());
        let mut omitted = next.clone();
        omitted.acceptance.clear();
        assert!(omitted.validate_scope_update(&previous).is_err());
        let mut optional = next.clone();
        optional.inputs[0].required = false;
        assert!(optional.validate_scope_update(&previous).is_err());
        let mut optional = next.clone();
        optional.acceptance[0].required = false;
        assert!(optional.validate_scope_update(&previous).is_err());
        let mut moved = next;
        moved.acceptance[0].phase_id = "navigation".into();
        assert!(moved.validate_scope_update(&previous).is_err());
    }

    mod test_support {
        use super::super::RecordedCheck;
        #[derive(Clone)]
        pub struct Claim {
            pub title: String,
            pub passed: bool,
        }
        impl Claim {
            pub fn new(title: &str, passed: bool, _: &str) -> Self {
                Self { title: title.into(), passed }
            }
        }
        #[derive(Clone)]
        pub struct CheckResult {
            pub check: String,
            pub round: u32,
            pub passed: bool,
            pub claims: Vec<Claim>,
        }
        impl CheckResult {
            pub fn new(check: &str, round: u32, passed: bool, _: &str) -> Self {
                Self { check: check.into(), round, passed, claims: Vec::new() }
            }
        }
        impl RecordedCheck for CheckResult {
            fn check_id(&self) -> &str {
                &self.check
            }
            fn round(&self) -> u32 {
                self.round
            }
            fn good(&self, _: f64) -> bool {
                self.passed && self.claims.iter().all(|c| c.passed)
            }
            fn claim_holds(&self, title: &str) -> Option<bool> {
                let mut claims = self.claims.iter().filter(|c| c.title == title).peekable();
                claims.peek()?;
                Some(claims.all(|c| c.passed))
            }
        }
    }
}
