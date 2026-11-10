use super::*;
use crate::agent::tests::{Scripted, turn};
use crate::agent::{ApiConversation, EngineHost, LoopConfig, ModelSlot};
use crate::model::{Block, Msg, StopReason, TurnResult};
use craft_anything_engine::workspace::Claim;
use std::collections::{BTreeSet, VecDeque};

struct PlanningScripted {
    inner: Scripted,
    requirements: Arc<Mutex<Vec<Option<String>>>>,
}

impl crate::model::ChatModel for PlanningScripted {
    fn turn(
        &mut self,
        req: &crate::model::TurnRequest<'_>,
        delta: &mut dyn FnMut(crate::model::Delta),
        stop: &crate::model::Stop,
    ) -> Result<TurnResult, ToolError> {
        self.requirements.lock().unwrap().push(req.required_tool.map(str::to_string));
        self.inner.turn(req, delta, stop)
    }
    fn model(&self) -> String {
        "scripted".into()
    }
}

struct PlannedKind;
struct SavedDraftCheck;

impl Check for SavedDraftCheck {
    fn id(&self) -> &'static str {
        "saved-draft"
    }
    fn run(&self, cx: &mut CheckCx) -> Result<CheckResult, ToolError> {
        Ok(saved_draft_result(cx.ws, cx.round))
    }
}

fn saved_draft_result(ws: &Workspace, round: u32) -> CheckResult {
    let saved = std::fs::read_to_string(ws.app_dir().join("draft.txt")).ok().as_deref() == Some("unfinished draft");
    CheckResult {
        similarity: Some(85.0),
        claims: vec![Claim::new("The saved draft can be reopened", saved, "Read the saved draft from the temporary project.")],
        ..CheckResult::new("saved-draft", round, saved, "Checked the synthetic saved draft.")
    }
}

impl TargetKind for PlannedKind {
    fn id(&self) -> &'static str {
        "planned-test"
    }
    fn words(&self) -> KindWords {
        KindWords { button: "Build it", icon: "code", ready: "It's ready", change_hint: "Ask for a change" }
    }
    fn detect(&self, _: &DetectCx) -> Option<u32> {
        Some(1000)
    }
    fn default_stack(&self, _: &DetectCx) -> &'static str {
        "Scripted stack"
    }
    fn goal(&self) -> &'static str {
        "a synthetic notebook"
    }
    fn checks(&self, ws: &Workspace) -> Vec<Box<dyn Check>> {
        if ws.mechanics_path().is_file() { vec![Box::new(kinds::MechanicsCheck)] } else { vec![Box::new(SavedDraftCheck)] }
    }
}

fn plan(phase_status: &str, criterion_status: &str) -> Value {
    let checks = if criterion_status == "verified" { json!([{"check":"saved-draft","round":1,"claim":"The saved draft can be reopened"}]) } else { json!([]) };
    json!({
        "version":1,
        "goal":"Save unfinished work and reopen it",
        "phases":[{"id":"storage","title":"Save drafts","goal":"Keep unfinished work","status":phase_status}],
        "inputs":[],
        "acceptance":[{
            "id":"reopen","phase_id":"storage","description":"The saved draft can be reopened",
            "status":criterion_status,"checks":checks
        }]
    })
}

fn tool(id: &str, name: &str, input: Value) -> Result<TurnResult, ToolError> {
    turn(vec![Block::ToolUse { id: id.into(), name: name.into(), input }], StopReason::ToolUse)
}

fn reply(text: &str) -> Result<TurnResult, ToolError> {
    turn(vec![Block::Text { text: text.into() }], StopReason::EndTurn)
}

fn run(ws: &mut Workspace, turns: VecDeque<Result<TurnResult, ToolError>>, max_rounds: u32) -> (String, Vec<Vec<Msg>>) {
    run_with_extensions(ws, turns, max_rounds, Default::default())
}

fn run_with_extensions(
    ws: &mut Workspace,
    turns: VecDeque<Result<TurnResult, ToolError>>,
    max_rounds: u32,
    extensions: extensions::Extensions,
) -> (String, Vec<Vec<Msg>>) {
    let (summary, requests, _) = run_recording_requirements(ws, turns, max_rounds, extensions);
    (summary, requests)
}

fn run_recording_requirements(
    ws: &mut Workspace,
    turns: VecDeque<Result<TurnResult, ToolError>>,
    max_rounds: u32,
    extensions: extensions::Extensions,
) -> (String, Vec<Vec<Msg>>, Vec<Option<String>>) {
    let mut worker = check_worker(ws, false);
    let seen = Arc::default();
    let requirements = Arc::default();
    let model = PlanningScripted { inner: Scripted { turns, seen: Arc::clone(&seen) }, requirements: Arc::clone(&requirements) };
    let slot = ModelSlot { name: "Scripted".into(), model: Box::new(model), local: true, effort: None, max_turns: 12, context_tokens: 0 };
    let enabled = BTreeSet::from([craft_anything_engine::Toolset::Core, craft_anything_engine::Toolset::Build]);
    let api = ApiConversation::start_with(Arc::new(EngineHost(worker.engine.clone())), vec![slot], LoopConfig::default(), enabled).unwrap();
    let entry = crate::BackendEntry {
        config: crate::BackendConfig {
            name: "Scripted".into(),
            kind: BackendKind::OpenaiCompatible,
            model: "scripted".into(),
            base_url: None,
            ssh_host: None,
            enabled: true,
        },
        options: Default::default(),
    };
    let mut kinds = KindRegistry::built_in();
    kinds.register(Arc::new(PlannedKind));
    let opts = RunOptions { message: Some("Keep unfinished drafts".into()), kinds, extensions, max_rounds, good_enough: 80.0, ..RunOptions::default() };
    let summary = worker.run(ws, entry, Some(api), None, KeptHistory::default(), &opts).unwrap();
    let requests = seen.lock().unwrap().clone();
    let requirements = requirements.lock().unwrap().clone();
    (summary, requests, requirements)
}

#[derive(Debug)]
struct UnavailableRequiredExtension;

fn legacy_mechanics_plan(ws: &mut Workspace, independent_work: bool) {
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::write(ws.mechanics_path(), r#"{"values":[{"name":"walk speed","value":4.5,"source":"reference measurement"}]}"#).unwrap();
    ws.manifest.checks = vec![CheckResult::new("mechanics", 1, true, "Legacy source literals matched.")];
    ws.save().unwrap();
    let mut saved = plan("done", "verified");
    saved["acceptance"][0]["checks"] = json!([{"check":"mechanics","round":1}]);
    if independent_work {
        saved["phases"].as_array_mut().unwrap().push(json!({"id":"export","title":"Export drafts","goal":"Save an export","status":"todo"}));
    }
    let worker = check_worker(ws, false);
    worker.attach(ws).unwrap();
    worker.call("core_build_plan_update", json!({"based_on_revision":0,"plan":saved})).unwrap();
}

#[test]
fn an_unavailable_measurement_does_not_retry_a_completed_implementation_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Measured work", "Keep observed behavior honest", TargetRef::None).unwrap();
    legacy_mechanics_plan(&mut ws, false);
    let turns = VecDeque::from([reply("The implementation is present."), reply("The comparison is unavailable."), reply("Still unavailable.")]);
    let (_, requests) = run(&mut ws, turns, 2);
    assert_eq!(requests.len(), 1, "a missing measurement cannot be supplied by repeating planning verification turns");
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(ws.manifest.note.contains("not been measured"), "{}", ws.manifest.note);
    assert!(!planning::review(&ws, 80.0).unwrap().complete());
    assert!(ws.manifest.checks.last().unwrap().unknowns.iter().any(|item| item.contains("not been measured")));
}

#[test]
fn an_unavailable_measurement_keeps_independent_unfinished_phases_actionable() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Independent work", "Keep building independent features", TargetRef::None).unwrap();
    legacy_mechanics_plan(&mut ws, true);
    let turns = VecDeque::from([
        reply("The current implementation still needs an export."),
        tool("export", "build_file_write", json!({"path":"app/export.txt","content":"an independent export"})),
        reply("The export is implemented; measurement remains unavailable."),
    ]);
    let (_, requests) = run(&mut ws, turns, 1);
    assert_eq!(requests.len(), 3);
    assert_eq!(std::fs::read_to_string(ws.app_dir().join("export.txt")).unwrap(), "an independent export");
    assert_eq!(ws.manifest.status, Status::Incomplete);
}

#[test]
fn an_unavailable_measurement_keeps_other_required_extensions_actionable() {
    #[derive(Debug)]
    struct ExportExtension;
    impl extensions::WorkflowExtension for ExportExtension {
        fn id(&self) -> &'static str {
            "export-fixture"
        }
        fn assess(&self, ws: &Workspace, _: f64) -> Result<extensions::Assessment, ToolError> {
            Ok(if ws.app_dir().join("export.txt").is_file() {
                extensions::Assessment::ready()
            } else {
                extensions::Assessment::blocked("The export is missing.".into(), "Write the independent export.".into())
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Extension work", "Keep all required work", TargetRef::None).unwrap();
    legacy_mechanics_plan(&mut ws, false);
    let mut extensions = extensions::Extensions::default();
    extensions.register(Arc::new(ExportExtension)).unwrap();
    let turns = VecDeque::from([
        reply("The implementation is present."),
        tool("export", "build_file_write", json!({"path":"app/export.txt","content":"an independent export"})),
        reply("The export is implemented; measurement remains unavailable."),
    ]);
    let (_, requests) = run_with_extensions(&mut ws, turns, 1, extensions);
    assert_eq!(requests.len(), 3);
    assert_eq!(std::fs::read_to_string(ws.app_dir().join("export.txt")).unwrap(), "an independent export");
    assert_eq!(ws.manifest.status, Status::Incomplete);
}

#[test]
fn a_saved_legacy_mechanics_round_is_rechecked_before_readiness() {
    use craft_anything_engine::workspace::progress::BuildProgress;
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Legacy check", "Retain honest verification", TargetRef::None).unwrap();
    legacy_mechanics_plan(&mut ws, false);
    let mut worker = check_worker(&ws, false);
    worker.self_check = Some((Arc::new(PlannedKind), 80.0));
    worker.checks_unless_unchanged(&ws, &PlannedKind, 1, || Ok(ws.manifest.checks.clone())).unwrap();
    let progress = BuildProgress { checked: worker.kept_checks.lock().unwrap().saved_for(1), ..Default::default() };
    assert!(progress.checked.is_some(), "the fixture has a saved legacy passing round");
    *worker.kept_checks.lock().unwrap() = kept_check::KeptChecks::from_saved(&progress, &ws);
    let ran = std::cell::Cell::new(false);
    let checks = worker
        .checks_unless_unchanged(&ws, &PlannedKind, 2, || {
            ran.set(true);
            worker.run_checks(&ws, &PlannedKind, 2)
        })
        .unwrap();
    ws.manifest.checks.extend(checks);
    worker.finish_checks(&mut ws, 2, 80.0, &Default::default()).unwrap();
    assert!(ran.get(), "a saved legacy source-literal check cannot stand in for current mechanics verification");
    assert_eq!(ws.manifest.status, Status::Incomplete);
}

#[test]
fn cached_checks_still_require_the_current_extension_obligations() {
    use std::cell::Cell;
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Cached verification", "Retain a saved draft", TargetRef::None).unwrap();
    completed_plan(&ws);
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::write(ws.app_dir().join("draft.txt"), "unfinished draft").unwrap();
    let mut worker = check_worker(&ws, false);
    worker.self_check = Some((Arc::new(PlannedKind), 80.0));
    let runs = Cell::new(0);
    for round in [1, 2] {
        let results = worker
            .checks_unless_unchanged(&ws, &PlannedKind, round, || {
                runs.set(runs.get() + 1);
                Ok(vec![saved_draft_result(&ws, round)])
            })
            .unwrap();
        assert_eq!(results[0].round, round);
        assert!(results.iter().all(|check| check.good(80.0)));
        ws.manifest.checks.extend(results);
        ws.save().unwrap();
    }
    assert_eq!(runs.get(), 1, "the second round really reused a passing observation");
    let mut extensions = extensions::Extensions::default();
    extensions.register(Arc::new(UnavailableRequiredExtension)).unwrap();
    let summary = worker.finish_checks(&mut ws, 2, 80.0, &extensions).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(summary.contains("required fixture verification"), "{summary}");

    let state = craft_anything_engine::extensions::planning::load_project_strict(&ws.investigation_dir()).unwrap();
    let mut plan = state.plan.unwrap();
    plan.phases[0].status = craft_anything_engine::build_plan::PhaseStatus::Todo;
    let mut session = craft_anything_engine::Session::isolated(&dir.path().join("config"));
    session.open_project(Project::open(&ws.investigation_dir()).unwrap()).unwrap();
    Engine::new(session).call("core_build_plan_update", json!({"plan":plan,"based_on_revision":state.revision}), CallOptions::new(Caller::Agent)).unwrap();
    let results = worker
        .checks_unless_unchanged(&ws, &PlannedKind, 3, || {
            runs.set(runs.get() + 1);
            Ok(vec![saved_draft_result(&ws, 3)])
        })
        .unwrap();
    assert_eq!(runs.get(), 1, "planning metadata does not rebuild an unchanged app");
    ws.manifest.checks.extend(results);
    let summary = worker.finish_checks(&mut ws, 3, 80.0, &extensions::Extensions::default()).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete, "cached checks cannot finish a reopened phase");
    assert!(!summary.starts_with("Ready:"));
}

#[test]
fn a_higher_quality_target_cannot_reuse_a_lower_targets_recorded_round() {
    use craft_anything_engine::workspace::progress::BuildProgress;
    use std::cell::Cell;
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Changed quality", "Retain a saved draft", TargetRef::None).unwrap();
    completed_plan(&ws);
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::write(ws.app_dir().join("draft.txt"), "unfinished draft").unwrap();
    let mut first = check_worker(&ws, false);
    first.self_check = Some((Arc::new(PlannedKind), 80.0));
    let runs = Cell::new(0);
    let mut observed = first
        .checks_unless_unchanged(&ws, &PlannedKind, 1, || {
            runs.set(runs.get() + 1);
            Ok(vec![saved_draft_result(&ws, 1)])
        })
        .unwrap();
    observed[0].good_enough = Some(80.0);
    assert!(observed[0].good(80.0));
    ws.manifest.checks.extend(observed);
    ws.save().unwrap();
    let progress = BuildProgress { checked: first.kept_checks.lock().unwrap().saved_for(1), ..BuildProgress::default() };

    let mut next = check_worker(&ws, false);
    next.self_check = Some((Arc::new(PlannedKind), 90.0));
    *next.kept_checks.lock().unwrap() = kept_check::KeptChecks::from_saved(&progress, &ws);
    let mut again = next
        .checks_unless_unchanged(&ws, &PlannedKind, 2, || {
            runs.set(runs.get() + 1);
            Ok(vec![saved_draft_result(&ws, 2)])
        })
        .unwrap();
    for check in &mut again {
        if check.good_enough.is_none() {
            check.good_enough = Some(90.0);
        }
    }
    assert!(!again[0].good(90.0), "a result measured at 85% cannot meet the new 90% target by retaining an old 80% bar");
    assert_eq!(runs.get(), 2, "a changed quality target checks again");
    ws.manifest.checks.extend(again);
    next.finish_checks(&mut ws, 2, 90.0, &extensions::Extensions::default()).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete);
}

#[test]
fn changing_the_picture_threshold_rechecks_an_unchanged_app() {
    use std::cell::Cell;
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Changed picture policy", "Retain a saved draft", TargetRef::None).unwrap();
    completed_plan(&ws);
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::write(ws.app_dir().join("draft.txt"), "unfinished draft").unwrap();
    let mut worker = check_worker(&ws, false);
    worker.self_check = Some((Arc::new(PlannedKind), 90.0));
    worker.engine.session().settings.build.picture_good_enough = Some(80.0);
    let runs = Cell::new(0);
    let check = |ws: &Workspace, round| {
        worker
            .checks_unless_unchanged(ws, &PlannedKind, round, || {
                runs.set(runs.get() + 1);
                let mut observed = saved_draft_result(ws, round);
                observed.good_enough = worker.engine.session().settings.build.picture_good_enough;
                Ok(vec![observed])
            })
            .unwrap()
    };
    let first = check(&ws, 1);
    assert!(first[0].good(90.0), "the explicit picture target was 80%");
    ws.manifest.checks.extend(first);
    ws.save().unwrap();
    worker.engine.session().settings.build.picture_good_enough = Some(90.0);
    let again = check(&ws, 2);
    assert!(!again[0].good(90.0), "a changed picture target must not borrow an old explicit bar");
    assert_eq!(runs.get(), 2);
    ws.manifest.checks.extend(again);
    worker.finish_checks(&mut ws, 2, 90.0, &extensions::Extensions::default()).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete);
}

impl extensions::WorkflowExtension for UnavailableRequiredExtension {
    fn id(&self) -> &'static str {
        "required-fixture"
    }
    fn assess(&self, _: &Workspace, _: f64) -> Result<extensions::Assessment, ToolError> {
        Err(ToolError::unavailable("The required fixture verification is unavailable."))
    }
}

#[test]
fn a_required_extension_error_cannot_be_replaced_by_passing_builtin_checks() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Required extension", "Retain a saved draft", TargetRef::None).unwrap();
    completed_plan(&ws);
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::write(ws.app_dir().join("draft.txt"), "unfinished draft").unwrap();
    let mut extensions = extensions::Extensions::default();
    extensions.register(Arc::new(UnavailableRequiredExtension)).unwrap();
    let (summary, requests) = run_with_extensions(&mut ws, VecDeque::from([reply("The saved draft can be reopened.")]), 2, extensions);
    assert_eq!(requests.len(), 1);
    assert!(ws.manifest.checks.iter().all(|check| check.good(80.0)));
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(summary.contains("required fixture verification"), "{summary}");
}

#[test]
fn a_missing_plan_stops_before_an_implementation_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Notebook", "Build a notebook", TargetRef::None).unwrap();
    let turns = VecDeque::from([reply("I have a plan."), reply("The plan is saved."), reply("No tool result returned.")]);
    let (summary, requests) = run(&mut ws, turns, 2);
    assert_eq!(requests.len(), 3, "unsaved planning corrections cannot advance to implementation");
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(ws.manifest.checks.is_empty());
    assert!(summary.contains("not been recorded"), "{summary}");
    assert!(!ws.app_dir().join("draft.txt").exists());
}

#[test]
fn an_unsaved_planning_reply_is_corrected_before_implementation() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Notebook", "Build a notebook", TargetRef::None).unwrap();
    let turns = VecDeque::from([
        reply("I saved the plan. No application files were changed."),
        tool("plan", "core_build_plan_update", json!({"based_on_revision":0,"plan":plan("todo","pending")})),
        reply("The plan is now saved."),
        tool("write", "build_file_write", json!({"path":"app/draft.txt","content":"unfinished draft"})),
        tool("implement", "core_build_plan_update", json!({"based_on_revision":1,"plan":plan("done","implemented")})),
        reply("The draft is implemented, and needs verification."),
        tool("verify", "core_build_plan_update", json!({"based_on_revision":2,"plan":plan("done","verified")})),
        reply("The recorded check confirms the saved draft."),
    ]);
    let (summary, requests, requirements) = run_recording_requirements(&mut ws, turns, 2, Default::default());
    assert_eq!(ws.manifest.status, Status::Ready, "{summary}");
    assert_eq!(std::fs::read_to_string(ws.app_dir().join("draft.txt")).unwrap(), "unfinished draft");
    let saved = craft_anything_engine::extensions::planning::load_project_strict(&ws.investigation_dir()).unwrap();
    assert_eq!(saved.revision, 3);
    assert_eq!(saved.plan.unwrap().goal, "Save unfinished work and reopen it");
    let correction = requests[1].last().unwrap().text();
    assert!(correction.contains("core_build_plan_update") && correction.contains("not saved"), "{correction}");
    assert_eq!(requests.len(), 8, "a successful correction must release the implementation turn");
    assert_eq!(requirements, vec![None, Some("core_build_plan_update".into()), None, None, None, None, None, None]);
}

#[test]
fn repeated_unsaved_planning_claims_stop_after_bounded_corrections() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Notebook", "Build a notebook", TargetRef::None).unwrap();
    let turns = VecDeque::from([
        reply("The plan is saved."),
        reply("The plan is definitely saved."),
        reply("The tool did not return a result."),
        tool("must-not-write", "build_file_write", json!({"path":"app/draft.txt","content":"unexpected implementation"})),
    ]);
    let (summary, requests) = run(&mut ws, turns, 2);
    assert_eq!(requests.len(), 3, "initial planning plus two corrective turns, then stop");
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(ws.manifest.checks.is_empty());
    assert!(summary.contains("not been recorded"), "{summary}");
    assert!(planning::load(&ws).unwrap().is_none());
    assert!(!ws.app_dir().join("draft.txt").exists());
}

#[test]
fn a_rejected_plan_save_can_be_corrected_without_losing_the_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Notebook", "Build a notebook", TargetRef::None).unwrap();
    let turns = VecDeque::from([
        tool("stale", "core_build_plan_update", json!({"based_on_revision":99,"plan":plan("todo","pending")})),
        reply("The plan was saved."),
        tool("corrected", "core_build_plan_update", json!({"based_on_revision":0,"plan":plan("todo","pending")})),
        reply("The corrected plan was saved."),
        reply("Implementation remains pending."),
    ]);
    let (_, requests) = run(&mut ws, turns, 0);
    let state = craft_anything_engine::extensions::planning::load_project_strict(&ws.investigation_dir()).unwrap();
    assert_eq!(state.revision, 1);
    assert_eq!(state.plan.unwrap().phases[0].id, "storage");
    assert!(
        requests[1].last().unwrap().blocks.iter().any(|block| matches!(block, Block::ToolResult { tool_use_id, is_error: true, .. } if tool_use_id == "stale"))
    );
    assert!(requests[2].last().unwrap().text().contains("based_on_revision"));
    assert_eq!(ws.manifest.status, Status::Incomplete, "saving a plan does not complete the implementation");
    assert!(!ws.app_dir().join("draft.txt").exists());
}

#[test]
fn planning_then_implementation_then_recorded_verification_can_finish() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Notebook", "Build a notebook", TargetRef::None).unwrap();
    let turns = VecDeque::from([
        tool("plan", "core_build_plan_update", json!({"based_on_revision":0,"plan":plan("todo","pending")})),
        reply("The plan is saved."),
        tool("write", "build_file_write", json!({"path":"app/draft.txt","content":"unfinished draft"})),
        tool("implement", "core_build_plan_update", json!({"based_on_revision":1,"plan":plan("done","implemented")})),
        reply("The draft is implemented, and needs verification."),
        tool("verify", "core_build_plan_update", json!({"based_on_revision":2,"plan":plan("done","verified")})),
        reply("The recorded check confirms the saved draft."),
    ]);
    let (_, requests) = run(&mut ws, turns, 2);
    assert_eq!(ws.manifest.status, Status::Ready);
    assert_eq!(ws.manifest.checks.len(), 2, "a passing rerun revalidates the cited check rather than making an endless stale-plan loop");
    assert!(
        ws.manifest.checks.iter().all(|check| check.good_enough == Some(80.0) && check.good(90.0)),
        "recorded checks retain the run's target when a later view uses default settings"
    );
    assert_eq!(std::fs::read_to_string(ws.app_dir().join("draft.txt")).unwrap(), "unfinished draft");
    let saved = planning::load(&ws).unwrap().unwrap();
    assert!(saved.completion_blockers(&ws.manifest.checks, 90.0).is_empty());
    assert_eq!(requests.len(), 7, "all scripted calls finish without provider retries");
}
