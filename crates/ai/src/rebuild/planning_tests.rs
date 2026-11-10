use super::*;
use crate::agent::tests::{Scripted, turn};
use crate::agent::{ApiConversation, EngineHost, LoopConfig, ModelSlot};
use crate::model::{Block, Msg, StopReason, TurnResult};
use craft_anything_engine::workspace::Claim;
use std::collections::{BTreeSet, VecDeque};

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
    fn checks(&self, _: &Workspace) -> Vec<Box<dyn Check>> {
        vec![Box::new(SavedDraftCheck)]
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
    let mut worker = check_worker(ws, false);
    let seen = Arc::default();
    let model = Scripted { turns, seen: Arc::clone(&seen) };
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
    (summary, requests)
}

#[derive(Debug)]
struct UnavailableRequiredExtension;

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
    let (summary, requests) = run(&mut ws, VecDeque::from([reply("I have a plan.")]), 2);
    assert_eq!(requests.len(), 1, "an unsaved plan cannot advance to implementation");
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(ws.manifest.checks.is_empty());
    assert!(summary.contains("not been recorded"), "{summary}");
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
