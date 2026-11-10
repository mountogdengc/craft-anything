use craft_anything_evidence::ErrorCode;
use craft_anything_planning::{BuildPlan, store};
use serde_json::json;

fn plan() -> BuildPlan {
    serde_json::from_value(json!({
        "goal":"Reopen saved entries",
        "phases":[{"id":"data","title":"Data","goal":"Persist entries"}],
        "acceptance":[{"id":"reload","phase_id":"data","description":"Entries remain after reopening"}]
    }))
    .unwrap()
}

#[test]
fn updates_render_a_readable_plan_beside_the_extension_record() {
    let directory = tempfile::tempdir().unwrap();
    store::update(directory.path(), 0, plan(), &|| false, |_| Ok(())).unwrap();
    let readable = std::fs::read_to_string(directory.path().join("extensions/planning/plan.md")).unwrap();
    assert!(readable.contains("Reopen saved entries") && readable.contains("Entries remain after reopening"));
    assert!(!directory.path().join("investigation/plan.md").exists());
}

#[test]
fn loading_a_legacy_view_is_read_only_and_updates_keep_scope() {
    let directory = tempfile::tempdir().unwrap();
    let legacy = directory.path().join("investigation/state.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    let bytes = serde_json::to_vec(&json!({"revision":7,"build_plan":plan()})).unwrap();
    std::fs::write(&legacy, &bytes).unwrap();
    let state = store::load(directory.path()).unwrap();
    assert_eq!(state.revision, 7);
    assert_eq!(state.plan.as_ref().unwrap().acceptance[0].id, "reload");
    assert!(!directory.path().join("extensions").exists());
    let saved = store::update(directory.path(), 7, plan(), &|| false, |_| Ok(())).unwrap();
    assert_eq!(saved.revision, 8);
    assert_eq!(std::fs::read(&legacy).unwrap(), bytes);
    let mut reduced = plan();
    reduced.acceptance[0].required = false;
    assert!(store::update(directory.path(), 8, reduced, &|| false, |_| Ok(())).is_err());
    assert_eq!(store::load(directory.path()).unwrap(), saved);
    assert_eq!(store::update(directory.path(), 7, plan(), &|| false, |_| Ok(())).unwrap_err().code, ErrorCode::StaleRevision);
}

#[test]
fn a_null_legacy_version_has_the_same_meaning_as_an_absent_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("investigation/state.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = serde_json::to_vec(&json!({"version":null,"revision":7,"build_plan":plan()})).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let loaded = store::load(directory.path()).unwrap();
    assert_eq!(loaded.revision, 7);
    assert_eq!(loaded.plan, Some(plan()));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn revision_overflow_and_oversized_records_are_refused_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(store::STATE_PATH);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = serde_json::to_vec(&store::State { revision: u64::MAX, plan: Some(plan()), ..Default::default() }).unwrap();
    std::fs::write(&path, &before).unwrap();
    assert_eq!(store::update(directory.path(), u64::MAX, plan(), &|| false, |_| Ok(())).unwrap_err().code, ErrorCode::OverLimit);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let file = std::fs::OpenOptions::new().write(true).truncate(true).open(&path).unwrap();
    file.set_len(store::MAX_STATE_BYTES + 1).unwrap();
    drop(file);
    assert_eq!(store::load(directory.path()).unwrap_err().code, ErrorCode::OverLimit);
}

#[test]
fn a_cancelled_lock_wait_does_not_wait_for_the_whole_deadline() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let lock = directory.path().join("investigation/update.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lock).unwrap();
    file.try_lock().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        signal.store(true, Ordering::SeqCst);
    });
    let started = std::time::Instant::now();
    let result = store::update(directory.path(), 0, plan(), &|| stop.load(Ordering::SeqCst), |_| Ok(()));
    thread.join().unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(!directory.path().join(store::STATE_PATH).exists());
}

#[test]
fn preserving_legacy_data_does_not_reacquire_the_hosts_lock() {
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("investigation");
    std::fs::create_dir_all(&folder).unwrap();
    let bytes = serde_json::to_vec(&json!({"version":1,"revision":3,"build_plan":plan()})).unwrap();
    std::fs::write(folder.join("state.json"), &bytes).unwrap();
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(folder.join("update.lock")).unwrap();
    lock.try_lock().unwrap();
    store::preserve_before_legacy_write(directory.path(), &|| false).unwrap();
    assert_eq!(store::load(directory.path()).unwrap().revision, 3);
    assert_eq!(std::fs::read(folder.join("state.json")).unwrap(), bytes);
}
