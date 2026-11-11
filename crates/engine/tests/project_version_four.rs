use craft_anything_engine::workspace::{ActionKind, Approval, MANIFEST_VERSION, Status, TargetRef, Workspace};
use serde_json::{Value, json};

#[test]
fn both_version_five_histories_preserve_capture_modes_and_saved_choices() {
    for (capture, expected) in [(None, json!(true)), (Some(Value::Null), Value::Null), (Some(json!(false)), json!(false)), (Some(json!(true)), json!(true))] {
        let dir = tempfile::tempdir().unwrap();
        let original = Workspace::create(dir.path(), "Capture migration", "Keep captured work", TargetRef::None).unwrap();
        let path = original.root.join("craft.json");
        std::fs::create_dir_all(original.capture_dir()).unwrap();
        std::fs::write(original.capture_dir().join("README.md"), "Synthetic capture notes").unwrap();
        let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        document["version"] = json!(5);
        document["status"] = json!("incomplete");
        document["approvals"] = json!({"commands":"refuse"});
        document["agent_session"] = json!("synthetic-session");
        document["agent_session_by"] = json!("synthetic-agent");
        document["agent_session_deep"] = json!(false);
        document.as_object_mut().unwrap().remove("capture_deep");
        if let Some(mode) = capture {
            document["capture_deep"] = mode;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

        let upgraded = Workspace::open(&original.root).unwrap();
        assert_eq!(upgraded.manifest.version, MANIFEST_VERSION);
        assert_eq!(upgraded.manifest.status, Status::Incomplete);
        assert_eq!(upgraded.manifest.approval(ActionKind::Commands, true), Approval::Refuse);
        upgraded.save().unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["capture_deep"], expected, "missing legacy capture modes count as Deep; explicit modes and null remain unchanged");
        assert_eq!(saved["agent_session_deep"], false, "capture migration cannot change the saved builder mode");
        let copy = upgraded.duplicate("Capture copy").unwrap();
        let copied: Value = serde_json::from_slice(&std::fs::read(copy.root.join("craft.json")).unwrap()).unwrap();
        assert_eq!(copied["capture_deep"], expected);
        assert_eq!(std::fs::read_to_string(copy.capture_dir().join("README.md")).unwrap(), "Synthetic capture notes");
        assert_eq!(copied["approvals"]["commands"], "refuse");
    }
}

#[test]
fn both_version_four_histories_keep_session_modes_and_incomplete_work() {
    for mode in [None, Some(Value::Null), Some(json!(false)), Some(json!(true))] {
        let expected_mode = mode.as_ref().and_then(Value::as_bool).unwrap_or(true);
        let dir = tempfile::tempdir().unwrap();
        let original = Workspace::create(dir.path(), "Combined migration", "Keep saved work", TargetRef::None).unwrap();
        let path = original.root.join("craft.json");
        let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        document["version"] = json!(4);
        document["status"] = json!("incomplete");
        document["approvals"] = json!({"commands":"refuse"});
        document["agent_session"] = json!("synthetic-session");
        document["agent_session_by"] = json!("synthetic-agent");
        document.as_object_mut().unwrap().remove("agent_session_deep");
        if let Some(mode) = mode {
            document["agent_session_deep"] = mode;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        let upgraded = Workspace::open(&original.root).unwrap();
        assert_eq!(upgraded.manifest.status, Status::Incomplete);
        assert_eq!(upgraded.manifest.approval(ActionKind::Commands, true), Approval::Refuse);
        upgraded.save().unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["agent_session_deep"], expected_mode, "unlabelled legacy sessions conservatively count as Deep");
        assert_eq!(saved["agent_session"], "synthetic-session");
    }
}

#[test]
fn upstream_version_three_keeps_approval_refusals_after_upgrade_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    let original = Workspace::create(dir.path(), "Approval migration", "Keep my approval choices", TargetRef::None).unwrap();
    let path = original.root.join("craft.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["version"] = json!(3);
    document["status"] = json!("ready");
    document["approvals"] = json!({"commands":"refuse", "publish":"ask"});
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

    let upgraded = Workspace::open(&original.root).unwrap();
    assert_eq!(upgraded.manifest.version, MANIFEST_VERSION, "older version-three apps must refuse to overwrite the combined format");
    assert_eq!(upgraded.manifest.status, Status::Ready, "an old accepted result is not reinterpreted");
    assert_eq!(upgraded.manifest.approval(ActionKind::Commands, true), Approval::Refuse);
    upgraded.save().unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], MANIFEST_VERSION);
    assert_eq!(saved["approvals"], json!({"commands":"refuse", "publish":"ask"}));
    let copied = upgraded.duplicate("Approval copy").unwrap();
    assert_eq!(Workspace::open(&copied.root).unwrap().manifest.approvals, upgraded.manifest.approvals);
}

#[test]
fn fork_version_three_keeps_incomplete_state_after_upgrade_and_copy() {
    let dir = tempfile::tempdir().unwrap();
    let original = Workspace::create(dir.path(), "Incomplete migration", "Keep unfinished work", TargetRef::None).unwrap();
    let path = original.root.join("craft.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["version"] = json!(3);
    document["status"] = json!("incomplete");
    document["note"] = json!("Draft restoration remains unverified.");
    document["awaiting_check"] = json!(true);
    document.as_object_mut().unwrap().remove("approvals");
    std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

    let upgraded = Workspace::open(&original.root).unwrap();
    assert_eq!(upgraded.manifest.version, MANIFEST_VERSION);
    assert_eq!(upgraded.manifest.status, Status::Incomplete);
    assert!(upgraded.manifest.awaiting_check);
    assert!(upgraded.manifest.approvals.is_empty());
    upgraded.save().unwrap();
    let copied = upgraded.duplicate("Incomplete copy").unwrap();
    assert_eq!(copied.manifest.status, Status::Incomplete);
    assert_eq!(copied.manifest.note, "Draft restoration remains unverified.");
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], MANIFEST_VERSION);
    assert_eq!(saved["status"], "incomplete");
}

#[test]
fn a_future_project_is_never_overwritten_by_the_combined_build() {
    let dir = tempfile::tempdir().unwrap();
    let original = Workspace::create(dir.path(), "Future migration", "Keep newer settings", TargetRef::None).unwrap();
    let path = original.root.join("craft.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["version"] = json!(MANIFEST_VERSION + 1);
    document["approvals"] = json!({"publish":"refuse"});
    document["future_scope_policy"] = json!({"keep_required":true});
    let bytes = serde_json::to_vec_pretty(&document).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let future = Workspace::open(&original.root).unwrap();
    assert!(future.is_read_only());
    assert!(future.save().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
