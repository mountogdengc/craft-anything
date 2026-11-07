use super::*;
use craft_anything_engine::tools::web_common::{Image, encode_png};
use craft_anything_evidence::{Severity, Unknown};

fn supplied_picture(ws: &Workspace) {
    std::fs::create_dir_all(ws.root.join("reference")).unwrap();
    let rgba = (0..64 * 32).flat_map(|i| if i % 64 < 32 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
    std::fs::write(ws.root.join("reference/picture-1.png"), encode_png(&Image { width: 64, height: 32, rgba }).unwrap()).unwrap();
}

struct ComparedKind;
struct ComparedCheck;

impl Check for ComparedCheck {
    fn id(&self) -> &'static str {
        "app-screenshot"
    }
    fn run(&self, cx: &mut CheckCx) -> Result<CheckResult, ToolError> {
        Ok(CheckResult::new(self.id(), cx.round, true, "The synthetic comparison passes."))
    }
}

impl TargetKind for ComparedKind {
    fn id(&self) -> &'static str {
        "review-comparison"
    }
    fn words(&self) -> KindWords {
        AppKind.words()
    }
    fn detect(&self, _: &DetectCx) -> Option<u32> {
        None
    }
    fn default_stack(&self, _: &DetectCx) -> &'static str {
        "Scripted stack"
    }
    fn goal(&self) -> &'static str {
        "a synthetic app"
    }
    fn checks(&self, _: &Workspace) -> Vec<Box<dyn Check>> {
        vec![Box::new(ComparedCheck)]
    }
}

fn damaged_unknown_ledger(damage: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Question integrity", "Keep verification honest", TargetRef::None).unwrap();
    completed_plan(&ws);
    ws.manifest.checks = vec![CheckResult::new("app-screenshot", 1, true, "The synthetic comparison passes.")];
    ws.save().unwrap();
    // Check-only citations intentionally do not need to open project metadata or evidence files.
    let state = ws.investigation_dir().join("investigation/state.json");
    let mut investigation: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    investigation["build_plan"]["acceptance"][0]["evidence_ids"] = json!([]);
    investigation["build_plan"]["acceptance"][0]["checks"] = json!([{"check":"app-screenshot","round":1}]);
    std::fs::write(&state, serde_json::to_vec_pretty(&investigation).unwrap()).unwrap();
    let project = Project::open(&ws.investigation_dir()).unwrap();
    let unknown = Unknown::new("The required save workflow is unobserved", "rebuild", Severity::High);
    project.store_unknown(&unknown).unwrap();
    let record = std::fs::read_dir(project.root.join("unknowns")).unwrap().next().unwrap().unwrap().path();
    match damage {
        "json" => std::fs::write(&record, b"{broken").unwrap(),
        "directory" => {
            std::fs::remove_file(record).unwrap();
            std::fs::remove_dir(project.root.join("unknowns")).unwrap();
            std::fs::write(project.root.join("unknowns"), b"not a directory").unwrap();
        }
        "metadata" => std::fs::write(project.root.join("project.toml"), b"broken = [").unwrap(),
        _ => panic!("unknown fixture damage"),
    }
    let mut worker = check_worker(&ws, false);
    worker.finish_checks(&mut ws, 1, 90.0).unwrap();
    assert_eq!(ws.manifest.status, Status::Incomplete, "unreadable questions are not an empty question list ({damage})");
    assert!(ws.manifest.note.contains("question") || ws.manifest.note.contains("investigation"), "{}", ws.manifest.note);
    worker.self_check = Some((Arc::new(ComparedKind), 90.0));
    let asks = Inbox::new(self_check::asks_dir(&ws.root));
    let id = asks.post(craft_anything_engine::inbox::RequestKind::Question, "Check", "", Vec::new(), false, None).unwrap();
    worker.answer_self_checks(&mut ws).unwrap();
    let answer = asks.wait(&id, Duration::from_secs(1), &|| false).unwrap().text.unwrap();
    assert!(!answer.contains("This would pass"), "{answer}");
    assert!(answer.contains("question") || answer.contains("investigation"), "{answer}");
}

#[test]
fn corrupt_unknown_records_cannot_disappear_from_readiness() {
    damaged_unknown_ledger("json");
}

#[test]
fn unreadable_unknown_directories_cannot_disappear_from_readiness() {
    damaged_unknown_ledger("directory");
}

#[test]
fn unreadable_project_metadata_cannot_disappear_from_readiness() {
    damaged_unknown_ledger("metadata");
}

#[test]
fn supplied_pictures_survive_a_failed_resumed_runtime_capture() {
    let dir = tempfile::tempdir().unwrap();
    let mut ws = Workspace::create(dir.path(), "Reference fallback", "Recreate an editor", TargetRef::Local { path: dir.path().join("missing.exe") }).unwrap();
    supplied_picture(&ws);
    let mut worker = check_worker(&ws, false);
    worker.engine.session().settings.runtime_study = true;
    worker.ensure_native_observation(&mut ws, &AppKind).unwrap();
    assert!(!ws.capture_dir().join("window.png").exists(), "fallback must not invent a runtime capture");
    assert!(!ws.capture_dir().join("ui-tree.json").exists());
    let notes = std::fs::read_to_string(ws.capture_dir().join("README.md")).unwrap();
    assert!(notes.contains("supplied reference"), "the limitation and fallback remain visible: {notes}");
    worker.stop.cancel();
    assert_eq!(worker.ensure_native_observation(&mut ws, &AppKind).unwrap_err().code, ErrorCode::Cancelled);
}

#[test]
fn supplied_pictures_survive_a_failed_first_runtime_study() {
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("invalid.exe");
    std::fs::write(&program, b"not an executable").unwrap();
    let mut ws = Workspace::create(dir.path(), "First reference", "Recreate an editor", TargetRef::Local { path: program }).unwrap();
    supplied_picture(&ws);
    let mut worker = check_worker(&ws, false);
    worker.engine.session().settings.runtime_study = true;
    worker.study(&mut ws, &AppKind).unwrap();
    assert!(!ws.capture_dir().join("window.png").exists());
    let notes = std::fs::read_to_string(ws.capture_dir().join("README.md")).unwrap();
    assert!(notes.contains("supplied reference"), "{notes}");
}

#[cfg(windows)]
#[test]
fn enabling_console_study_recovers_runs_without_gui_capture() {
    let dir = tempfile::tempdir().unwrap();
    let test_exe = std::env::current_exe().unwrap();
    let program = test_exe.parent().unwrap().parent().unwrap().join(craft_anything_platform::cli_file_name());
    assert!(program.is_file(), "build the CLI helper before AI tests");
    assert!(kinds::runs::is_console(&program));
    let mut ws = Workspace::create(dir.path(), "Console recovery", "Recreate a console tool", TargetRef::Local { path: program }).unwrap();
    std::fs::create_dir_all(ws.capture_dir()).unwrap();
    std::fs::write(ws.capture_dir().join("README.md"), "An old static study.").unwrap();
    let mut worker = check_worker(&ws, true);
    worker.engine.session().settings.runtime_study = true;
    worker.ensure_native_observation(&mut ws, &AppKind).unwrap();
    let runs = kinds::runs::captured(&ws);
    assert_eq!(runs.len(), 3);
    assert!(kinds::runs::ran_like_console(&runs));
    assert!(!ws.capture_dir().join("window.png").exists());
}

#[test]
fn static_package_evidence_allows_work_without_claiming_a_comparison() {
    use crate::agent::tests::turn;
    use crate::model::{Block, StopReason};
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("package");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("package.json"), r#"{"name":"fixture","main":"main.js"}"#).unwrap();
    std::fs::write(package.join("main.js"), "export const title = 'Fixture';").unwrap();
    let mut ws = Workspace::create(dir.path(), "Static package", "Recreate this app", TargetRef::Local { path: package }).unwrap();
    completed_plan(&ws);
    let mut worker = check_worker(&ws, true);
    worker.study(&mut ws, &AppKind).unwrap();
    let turns = std::collections::VecDeque::from([
        turn(
            vec![Block::ToolUse {
                id: "write".into(),
                name: "build_file_write".into(),
                input: json!({"path":"app/draft.txt","content":"Implemented from the static package"}),
            }],
            StopReason::ToolUse,
        ),
        turn(vec![Block::Text { text: "The implementation remains unverified against the original.".into() }], StopReason::EndTurn),
        // Existing Deep-mode runs send one decompilation reminder before their first checks.
        turn(vec![Block::Text { text: "Only static JavaScript evidence is available; native decompilation is unavailable.".into() }], StopReason::EndTurn),
    ]);
    let (result, requests) = super::readiness_integration::run_with(&mut ws, true, turns);
    assert!(result.is_ok(), "static evidence can reach implementation: {result:?}");
    assert_eq!(requests.len(), 3, "implementation and the existing Deep-mode reminder finish before the missing-comparison stop");
    assert_eq!(std::fs::read_to_string(ws.app_dir().join("draft.txt")).unwrap(), "Implemented from the static package");
    assert_eq!(ws.manifest.status, Status::Incomplete, "required comparison evidence is still missing");
}
