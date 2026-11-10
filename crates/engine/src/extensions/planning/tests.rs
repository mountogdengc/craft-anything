use crate::project::{Project, Purpose};
use crate::tools::investigation_tools;
use crate::{CallOptions, Caller, Engine};
use serde_json::json;

fn call(e: &Engine, id: &str, p: serde_json::Value) -> Result<serde_json::Value, craft_anything_evidence::ToolError> {
    e.call(id, p, CallOptions::new(Caller::Agent)).map(|o| o.to_json())
}

pub(crate) fn build_plan(goal: &str) -> serde_json::Value {
    json!({
        "goal":goal,
        "phases":[{"id":"catalogue","title":"Catalogue","goal":"Store and reopen entries"}],
        "inputs":[{"id":"sample","question":"Which sample record?","why":"Defines the fields","format":"One record","phase_ids":["catalogue"]}],
        "acceptance":[{"id":"reload","phase_id":"catalogue","description":"Saved entries survive reopening"}]
    })
}

#[test]
fn build_plan_survives_resuming_and_other_investigation_updates() {
    let directory = tempfile::tempdir().unwrap();
    let project = Project::create(&directory.path().join("project.cany"), "Test", Purpose::Education).unwrap();
    let root = project.root.clone();
    let mut session = crate::Session::isolated(&directory.path().join("config"));
    session.settings.deep_mode = false;
    session.open_project(project).unwrap();
    let engine = Engine::new(session);
    let mut plan = build_plan("Build a catalogue");
    plan["inputs"][0]["status"] = json!("provided");
    plan["inputs"][0]["response"] = json!("Use the synthetic record.");
    let update = json!({"plan":plan,"based_on_revision":0});
    let saved = call(&engine, "core_build_plan_update", update.clone()).unwrap();
    assert_eq!(saved["result"]["revision"], 1);
    assert_eq!(call(&engine, "core_build_plan_update", update).unwrap_err().code, craft_anything_evidence::ErrorCode::StaleRevision);
    call(&engine, "core_plan_update", json!({"steps":[{"title":"Study the visible form"}],"based_on_revision":0})).unwrap();
    call(&engine, "core_handoff_write", json!({"text":"Next: implement persistence."})).unwrap();
    let mut resumed = crate::Session::isolated(&directory.path().join("resumed-config"));
    resumed.open_project(Project::open(&root).unwrap()).unwrap();
    let investigation = investigation_tools::load_strict(&resumed).unwrap();
    assert_eq!(investigation.version, investigation_tools::INVESTIGATION_VERSION);
    assert_eq!(investigation.revision, 2);
    let build = investigation.build_plan.as_ref().unwrap();
    assert!(build.inputs[0].provided());
    assert_eq!(build.phases[0].status, crate::build_plan::PhaseStatus::Todo);
    assert_eq!(build.acceptance[0].status, crate::build_plan::AcceptanceStatus::Pending);
    assert_eq!(investigation_tools::cached(&resumed), investigation);
    assert_eq!(investigation_tools::load_project_strict(&root).unwrap(), investigation);
    let rendered = std::fs::read_to_string(root.join("extensions/planning/plan.md")).unwrap();
    assert!(rendered.contains("# Build plan"));
    assert!(rendered.contains("Which sample record?") && rendered.contains("Saved entries survive reopening"));
    let analysis = std::fs::read_to_string(root.join("investigation/plan.md")).unwrap();
    assert!(analysis.contains("Study the visible form"));
    assert!(!analysis.contains("# Build plan"));
}

#[test]
fn build_plan_verification_requires_existing_non_inferred_evidence() {
    use craft_anything_evidence::{Confidence, Environment, EvidenceBuilder, Subject};
    let engine = Engine::new(crate::testing::session());
    let mut plan = build_plan("Build a catalogue");
    plan["acceptance"][0]["status"] = json!("verified");
    let before = call(&engine, "core_investigation_show", json!({})).unwrap();
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_err());
    plan["acceptance"][0]["evidence_ids"] = json!([format!("ev_{}", "0".repeat(64))]);
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_err());
    assert_eq!(call(&engine, "core_investigation_show", json!({})).unwrap(), before);
    for (confidence, accepted) in [(Confidence::Inferred, false), (Confidence::Observed, true)] {
        let evidence = EvidenceBuilder::new("test-reload", Subject { kind: "test".into(), sha256: None, name: None }, Environment::builtin(true))
            .result(json!({"entries_after_reload":1}))
            .summary("Synthetic reload observation")
            .confidence(confidence)
            .build()
            .unwrap();
        let id = evidence.id.clone();
        engine.session().record(evidence).unwrap();
        plan["acceptance"][0]["evidence_ids"] = json!([id]);
        assert_eq!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_ok(), accepted);
    }
    assert_eq!(super::load(&engine.session()).unwrap().revision, 1);
}

#[test]
fn build_plan_updates_cannot_drop_or_demote_required_scope() {
    let engine = Engine::new(crate::testing::session());
    let mut plan = build_plan("Build a catalogue");
    let mut optional_input = plan["inputs"][0].clone();
    optional_input["id"] = json!("art");
    optional_input["required"] = json!(false);
    plan["inputs"].as_array_mut().unwrap().push(optional_input);
    let mut optional_criterion = plan["acceptance"][0].clone();
    optional_criterion["id"] = json!("polish");
    optional_criterion["required"] = json!(false);
    plan["acceptance"].as_array_mut().unwrap().push(optional_criterion.clone());
    call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).unwrap();
    let before = call(&engine, "core_investigation_show", json!({})).unwrap();
    let mut omitted_input = plan.clone();
    omitted_input["inputs"] = json!([]);
    let mut demoted_input = plan.clone();
    demoted_input["inputs"][0]["required"] = json!(false);
    let mut omitted_criterion = plan.clone();
    omitted_criterion["acceptance"] = json!([optional_criterion]);
    let mut demoted_criterion = plan.clone();
    demoted_criterion["acceptance"][0]["required"] = json!(false);
    let mut moved_criterion = plan.clone();
    moved_criterion["acceptance"][0]["phase_id"] = json!("other");
    moved_criterion["phases"].as_array_mut().unwrap().push(json!({"id":"other","title":"Other","goal":"Another phase"}));
    for invalid in [omitted_input, demoted_input, omitted_criterion, demoted_criterion, moved_criterion] {
        assert!(call(&engine, "core_build_plan_update", json!({"plan":invalid,"based_on_revision":1})).is_err());
        assert_eq!(call(&engine, "core_investigation_show", json!({})).unwrap(), before);
    }
    plan["inputs"].as_array_mut().unwrap().retain(|input| input["required"] != false);
    plan["acceptance"].as_array_mut().unwrap().retain(|criterion| criterion["required"] != false);
    plan["inputs"][0]["status"] = json!("provided");
    plan["inputs"][0]["response"] = json!("Use the synthetic record.");
    plan["acceptance"][0]["status"] = json!("implemented");
    plan["acceptance"][0]["description"] = json!("Stored entries remain after reopening the app.");
    let saved = call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":1})).unwrap();
    assert_eq!(saved["result"]["revision"], 2);
    assert_eq!(saved["result"]["plan"]["inputs"].as_array().unwrap().len(), 1);
    assert_eq!(saved["result"]["plan"]["acceptance"].as_array().unwrap().len(), 1);
}

#[test]
fn build_plan_check_citations_are_resolved_against_the_attached_workspace() {
    use crate::workspace::{CheckResult, Claim, TargetRef, Workspace};
    let directory = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::create(directory.path(), "Catalogue", "Store records", TargetRef::None).unwrap();
    let mut session = crate::Session::isolated(&directory.path().join("config"));
    session.ext.insert(std::sync::Arc::new(crate::inbox::InboxDir(workspace.inbox_dir())));
    let engine = Engine::new(session);
    let mut plan = build_plan("Build a catalogue");
    plan["acceptance"][0]["status"] = json!("verified");
    plan["acceptance"][0]["checks"] = json!([{"check":"persistence","round":1,"claim":"Saved entries survive reopening"}]);
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_err());
    let passed = CheckResult {
        claims: vec![Claim::new("Saved entries survive reopening", true, "")],
        ..CheckResult::new("persistence", 1, true, "One stored record returned.")
    };
    workspace.manifest.checks.push(passed.clone());
    workspace.save().unwrap();
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":0})).is_ok());
    workspace.manifest.checks.push(CheckResult { round: 2, ..passed.clone() });
    workspace.save().unwrap();
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":1})).is_ok(), "same check passing again revalidates its citation");
    workspace.manifest.checks.push(CheckResult { round: 3, passed: false, ..passed });
    workspace.save().unwrap();
    assert!(call(&engine, "core_build_plan_update", json!({"plan":plan,"based_on_revision":2})).is_err());
    assert_eq!(super::load(&engine.session()).unwrap().revision, 2);
}
