use craft_anything_engine::project::{Project, Purpose};
use craft_anything_engine::{CallOptions, Caller, Engine, Session};
use serde_json::{Value, json};

fn plan() -> Value {
    json!({
        "version":1,"goal":"Reopen saved catalogue entries",
        "phases":[{"id":"catalogue","title":"Catalogue","goal":"Store and reopen entries"}],
        "inputs":[],
        "acceptance":[{"id":"reload","phase_id":"catalogue","description":"Entries survive reopening","required":true}]
    })
}

fn fixture(directory: &std::path::Path) -> (Engine, std::path::PathBuf, Vec<u8>) {
    let project = Project::create(&directory.join("fixture.cany"), "Catalogue", Purpose::Education).unwrap();
    let root = project.root.clone();
    let legacy = root.join("investigation/state.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(&json!({"version":1,"revision":7,"build_plan":plan()})).unwrap();
    std::fs::write(&legacy, &bytes).unwrap();
    let mut session = Session::isolated(&directory.join("config"));
    session.open_project(project).unwrap();
    (Engine::new(session), root, bytes)
}

fn call(engine: &Engine, id: &str, params: Value) -> Result<Value, craft_anything_engine::evidence::ToolError> {
    engine.call(id, params, CallOptions::new(Caller::Agent)).map(|out| out.to_json())
}

#[test]
fn updating_a_legacy_plan_uses_independent_state_without_rewriting_its_source() {
    let directory = tempfile::tempdir().unwrap();
    let (engine, root, legacy) = fixture(directory.path());
    let saved = call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":7})).unwrap();
    assert_eq!(saved["result"]["revision"], 8);
    let sidecar = root.join("extensions/planning/state.json");
    assert!(sidecar.is_file(), "planning writes belong to the extension, not the analysis record");
    let state: Value = serde_json::from_slice(&std::fs::read(sidecar).unwrap()).unwrap();
    assert_eq!(state["version"], 1);
    assert_eq!(state["revision"], 8);
    assert_eq!(state["plan"]["acceptance"][0]["id"], "reload");
    assert_eq!(state["plan"]["acceptance"][0]["required"], true);
    assert_eq!(std::fs::read(root.join("investigation/state.json")).unwrap(), legacy, "import does not overwrite the legacy record");
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":7})).is_err(), "stale planning revisions are refused");
}

#[test]
fn an_analysis_write_preserves_the_embedded_plan_before_it_can_be_discarded() {
    let directory = tempfile::tempdir().unwrap();
    let (engine, root, _) = fixture(directory.path());
    call(&engine, "core_plan_update", json!({"steps":[{"title":"Observe the save workflow"}],"based_on_revision":7})).unwrap();
    let sidecar = root.join("extensions/planning/state.json");
    assert!(sidecar.is_file(), "an analysis update must first preserve extension-owned legacy data");
    let state: Value = serde_json::from_slice(&std::fs::read(sidecar).unwrap()).unwrap();
    assert_eq!(state["revision"], 7, "analysis revisions do not advance planning revisions");
    assert_eq!(state["plan"]["goal"], "Reopen saved catalogue entries");
    assert_eq!(state["plan"]["acceptance"][0]["id"], "reload");
    call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":7})).unwrap();
}

#[test]
fn an_unreadable_or_future_sidecar_never_reactivates_the_legacy_plan() {
    for bytes in [b"{broken".to_vec(), serde_json::to_vec(&json!({"version":99,"revision":8,"plan":plan()})).unwrap()] {
        let directory = tempfile::tempdir().unwrap();
        let (engine, root, legacy) = fixture(directory.path());
        let sidecar = root.join("extensions/planning/state.json");
        std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        std::fs::write(&sidecar, &bytes).unwrap();
        assert!(
            call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":7})).is_err(),
            "a present invalid sidecar is authoritative, not an excuse to fall back"
        );
        assert_eq!(std::fs::read(&sidecar).unwrap(), bytes);
        assert_eq!(std::fs::read(root.join("investigation/state.json")).unwrap(), legacy);
    }
}

#[test]
fn two_writers_cannot_both_accept_the_same_legacy_planning_revision() {
    let directory = tempfile::tempdir().unwrap();
    let (first, root, _) = fixture(directory.path());
    let mut session = Session::isolated(&directory.path().join("other-config"));
    session.open_project(Project::open(&root).unwrap()).unwrap();
    let second = Engine::new(session);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = [first, second]
        .into_iter()
        .map(|engine| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":7}))
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let rejected = results.into_iter().find_map(Result::err).unwrap();
    assert_eq!(rejected.code, craft_anything_engine::evidence::ErrorCode::StaleRevision);
}
