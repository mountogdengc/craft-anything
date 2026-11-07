use craft_anything_engine::workspace::{ActionKind, Approval, Status, TargetRef, Workspace};
use serde_json::{Value, json};

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
    assert_eq!(upgraded.manifest.version, 4, "older version-three apps must refuse to overwrite the combined format");
    assert_eq!(upgraded.manifest.status, Status::Ready, "an old accepted result is not reinterpreted");
    assert_eq!(upgraded.manifest.approval(ActionKind::Commands, true), Approval::Refuse);
    upgraded.save().unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], 4);
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
    assert_eq!(upgraded.manifest.version, 4);
    assert_eq!(upgraded.manifest.status, Status::Incomplete);
    assert!(upgraded.manifest.awaiting_check);
    assert!(upgraded.manifest.approvals.is_empty());
    upgraded.save().unwrap();
    let copied = upgraded.duplicate("Incomplete copy").unwrap();
    assert_eq!(copied.manifest.status, Status::Incomplete);
    assert_eq!(copied.manifest.note, "Draft restoration remains unverified.");
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["version"], 4);
    assert_eq!(saved["status"], "incomplete");
}

#[test]
fn a_future_project_is_never_overwritten_by_a_version_four_build() {
    let dir = tempfile::tempdir().unwrap();
    let original = Workspace::create(dir.path(), "Future migration", "Keep newer settings", TargetRef::None).unwrap();
    let path = original.root.join("craft.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["version"] = json!(5);
    document["approvals"] = json!({"publish":"refuse"});
    document["future_scope_policy"] = json!({"keep_required":true});
    let bytes = serde_json::to_vec_pretty(&document).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let future = Workspace::open(&original.root).unwrap();
    assert!(future.is_read_only());
    assert!(future.save().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
