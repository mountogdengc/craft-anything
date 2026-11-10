use craft_anything_engine::project::{Project, Purpose};
use craft_anything_engine::{CallOptions, Caller, Engine, Session};
use serde_json::{Value, json};

fn plan() -> Value {
    json!({
        "goal":"Retain a scratch plan",
        "phases":[{"id":"notes","title":"Notes","goal":"Keep and reopen notes"}],
        "acceptance":[{"id":"reopen","phase_id":"notes","description":"Notes remain after reopening"}]
    })
}

fn call(engine: &Engine, id: &str, params: Value) -> Value {
    engine.call(id, params, CallOptions::new(Caller::Agent)).unwrap().to_json()["result"].clone()
}

#[test]
fn scratch_planning_has_its_own_query_and_revision_before_a_project_is_opened() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::new(Session::isolated(directory.path()));
    call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":0}));
    let shown = call(&engine, "core_build_plan_show", json!({}));
    assert_eq!(shown["revision"], 1);
    assert_eq!(shown["plan"]["acceptance"][0]["id"], "reopen");
    call(&engine, "core_plan_update", json!({"steps":[{"title":"Observe reopen"}]}));
    assert_eq!(call(&engine, "core_build_plan_show", json!({}))["revision"], 1, "analysis updates do not advance the planning revision");
}

#[test]
fn creating_a_shared_project_transfers_scratch_planning_and_its_evidence() {
    use craft_anything_engine::evidence::{Confidence, Environment, EvidenceBuilder, Subject};
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::new(Session::isolated(directory.path()));
    let evidence = EvidenceBuilder::new("test-reopen", Subject { kind: "test".into(), sha256: None, name: None }, Environment::builtin(true))
        .result(json!({"notes_after_reopen":1}))
        .summary("Synthetic reopen observation")
        .confidence(Confidence::Observed)
        .build()
        .unwrap();
    let evidence_id = evidence.id.clone();
    engine.session().record(evidence).unwrap();
    let mut verified = plan();
    verified["acceptance"][0]["status"] = json!("verified");
    verified["acceptance"][0]["evidence_ids"] = json!([evidence_id]);
    call(&engine, "core_build_plan_update", json!({"plan":verified,"based_on_revision":0}));
    let root = engine.session().ensure_shared_project().unwrap();
    assert!(root.join("extensions/planning/state.json").is_file());
    let shown = call(&engine, "core_build_plan_show", json!({}));
    assert_eq!(shown["revision"], 1);
    assert_eq!(shown["plan"]["acceptance"][0]["evidence_ids"][0], evidence_id);
    assert!(Project::open(&root).unwrap().load_evidence(&evidence_id).is_ok());
}

#[test]
fn opening_an_existing_project_does_not_import_an_unrelated_scratch_plan() {
    let directory = tempfile::tempdir().unwrap();
    let engine = Engine::new(Session::isolated(&directory.path().join("config")));
    call(&engine, "core_build_plan_update", json!({"plan":plan(),"based_on_revision":0}));
    let project = Project::create(&directory.path().join("existing.cany"), "Existing", Purpose::Education).unwrap();
    let root = project.root.clone();
    engine.session().open_project(project).unwrap();
    let shown = call(&engine, "core_build_plan_show", json!({}));
    assert_eq!(shown["revision"], 0);
    assert!(shown["plan"].is_null());
    assert!(!root.join("extensions/planning/state.json").exists(), "read-only inspection does not persist or import scratch state");
}
