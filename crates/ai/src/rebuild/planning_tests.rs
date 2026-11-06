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
        let saved = std::fs::read_to_string(cx.ws.app_dir().join("draft.txt")).ok().as_deref() == Some("unfinished draft");
        Ok(CheckResult {
            similarity: Some(85.0),
            claims: vec![Claim::new("The saved draft can be reopened", saved, "Read the saved draft from the temporary project.")],
            ..CheckResult::new(self.id(), cx.round, saved, "Checked the synthetic saved draft.")
        })
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
    let opts = RunOptions { message: Some("Keep unfinished drafts".into()), kinds, max_rounds, good_enough: 80.0, ..RunOptions::default() };
    let summary = worker.run(ws, entry, Some(api), None, &opts).unwrap();
    let requests = seen.lock().unwrap().clone();
    (summary, requests)
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
