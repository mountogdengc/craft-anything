use super::*;
use crate::agent::tests::{Scripted, turn};
use crate::agent::{ApiConversation, EngineHost, LoopConfig, ModelSlot};
use crate::model::{Block, Msg, StopReason};
use craft_anything_evidence::{Severity, Unknown};

fn missing_comparison(round: u32) -> CheckResult {
    let mut stored = serde_json::to_value(CheckResult::new("app-screenshot", round, true, "The build draws, but the original was not compared.")).unwrap();
    stored["not_compared"] = json!(true);
    serde_json::from_value(stored).unwrap()
}

#[test]
fn missing_comparison_retains_the_preview_and_cannot_bypass_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Comparison", "Recreate an editor", TargetRef::None).unwrap();
    completed_plan(&ws);
    std::fs::create_dir_all(ws.app_dir()).unwrap();
    std::fs::write(ws.app_dir().join("index.html"), "<h1>Preview</h1>").unwrap();
    ws.manifest.checks = vec![missing_comparison(1)];
    let mut worker = check_worker(&ws, false);
    let preview = serve_dir(&ws.app_dir()).unwrap();
    let url = preview.url();
    *worker.preview.lock().unwrap() = Some(preview);
    let summary = worker.finish_checks(&mut ws, 1, 90.0).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(summary.contains("original") && summary.contains("compar"), "{summary}");
    assert_eq!(worker.preview.lock().unwrap().as_ref().map(StaticServer::url), Some(url));

    ws.manifest.checks = vec![CheckResult::new("app-screenshot", 2, true, "The original was compared.")];
    worker.finish_checks(&mut ws, 2, 90.0).unwrap();
    assert_eq!(ws.manifest.status, Status::Ready, "a complete plan and a repaired comparison may finish");
    let state = ws.investigation_dir().join("investigation/state.json");
    let mut investigation: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    investigation["build_plan"]["phases"][0]["status"] = json!("todo");
    std::fs::write(&state, serde_json::to_vec_pretty(&investigation).unwrap()).unwrap();
    worker.finish_checks(&mut ws, 2, 90.0).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete, "a good comparison cannot finish an unfinished phase");
}

#[test]
fn essential_unknowns_block_final_readiness_and_the_builders_pass_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Unknown workflow", "Recreate an editor", TargetRef::None).unwrap();
    completed_plan(&ws);
    let project = Project::open(&ws.investigation_dir()).unwrap();
    let question = Unknown::new("The save and reopen workflow has not been observed", "rebuild", Severity::High);
    project.store_unknown(&question).unwrap();
    ws.manifest.checks = vec![CheckResult::new("app-screenshot", 1, true, "The window was compared.")];
    let mut worker = check_worker(&ws, false);
    worker.finish_checks(&mut ws, 1, 90.0).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert!(ws.manifest.note.contains("save and reopen"));
    assert!(!self_check::report(&ws.manifest.checks, 90.0, &self_check::open_questions(&ws).unwrap()).contains("This would pass"));
}

struct UncomparedKind;
struct UncomparedCheck;

impl Check for UncomparedCheck {
    fn id(&self) -> &'static str {
        "app-screenshot"
    }
    fn run(&self, cx: &mut CheckCx) -> Result<CheckResult, ToolError> {
        Ok(missing_comparison(cx.round))
    }
}

impl TargetKind for UncomparedKind {
    fn id(&self) -> &'static str {
        "uncompared-fixture"
    }
    fn words(&self) -> KindWords {
        KindWords { button: "Build it", icon: "code", ready: "Ready", change_hint: "Continue" }
    }
    fn detect(&self, _: &DetectCx) -> Option<u32> {
        Some(1000)
    }
    fn default_stack(&self, _: &DetectCx) -> &'static str {
        "Scripted stack"
    }
    fn goal(&self) -> &'static str {
        "a scripted app"
    }
    fn checks(&self, _: &Workspace) -> Vec<Box<dyn Check>> {
        vec![Box::new(UncomparedCheck)]
    }
}

fn run_once(ws: &mut Workspace) -> (Result<String, ToolError>, Vec<Vec<Msg>>) {
    run_with(
        ws,
        false,
        std::collections::VecDeque::from([turn(vec![Block::Text { text: "The build draws; reference evidence is missing.".into() }], StopReason::EndTurn)]),
    )
}

pub(super) fn run_with(
    ws: &mut Workspace,
    deep: bool,
    turns: std::collections::VecDeque<Result<crate::model::TurnResult, ToolError>>,
) -> (Result<String, ToolError>, Vec<Vec<Msg>>) {
    let mut worker = check_worker(ws, deep);
    let seen = Arc::default();
    let model = Scripted { turns, seen: Arc::clone(&seen) };
    let slot = ModelSlot { name: "Scripted".into(), model: Box::new(model), local: true, effort: None, max_turns: 8, context_tokens: 0 };
    let enabled = std::collections::BTreeSet::from([craft_anything_engine::Toolset::Core, craft_anything_engine::Toolset::Build]);
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
    kinds.register(Arc::new(UncomparedKind));
    let opts = RunOptions { message: Some("Continue this build".into()), max_rounds: 5, kinds, ..RunOptions::default() };
    let result = worker.run(ws, entry, Some(api), None, &opts);
    let requests = seen.lock().unwrap().clone();
    (result, requests)
}

#[test]
fn missing_comparison_stops_without_spending_more_builder_turns() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Missing reference", "Recreate an editor", TargetRef::None).unwrap();
    completed_plan(&ws);
    let (result, requests) = run_once(&mut ws);
    assert!(result.is_ok(), "a missing comparison retains a usable incomplete result: {result:?}");
    assert_eq!(requests.len(), 1, "a missing reference is not repaired by more paid builder turns");
    assert_eq!(ws.manifest.status, Status::Incomplete);
    assert_eq!(ws.manifest.checks.len(), 1);
}

#[test]
fn a_legacy_native_study_without_observation_stops_before_a_builder_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Legacy editor", "Recreate an editor", TargetRef::Local { path: dir.path().join("missing.exe") }).unwrap();
    completed_plan(&ws);
    std::fs::create_dir_all(ws.capture_dir()).unwrap();
    std::fs::write(ws.capture_dir().join("README.md"), "An old study without a working-interface capture.").unwrap();
    let (result, requests) = run_once(&mut ws);
    assert!(result.is_err(), "missing native observation must be surfaced before implementation");
    assert!(requests.is_empty(), "no provider call is needed to identify missing local evidence");
}
