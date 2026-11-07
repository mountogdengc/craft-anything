use craft_anything_engine::build_plan::CheckReference;
use craft_anything_engine::workspace::{CheckBlocker, CheckResult, CheckVerdict};
use serde_json::json;

fn stored_missing_comparison(round: u32) -> CheckResult {
    let check = CheckResult::new("app-screenshot", round, true, "The program draws, but the original was not compared.");
    let mut document = serde_json::to_value(check).unwrap();
    document["not_compared"] = json!(true);
    serde_json::from_value(document).unwrap()
}

#[test]
fn an_uncompared_original_cannot_make_a_passing_build_ready() {
    let check = stored_missing_comparison(2);
    assert!(!check.good(90.0), "drawing a window cannot verify comparison with the original");
    let verdict = CheckVerdict::latest(&[check], 90.0);
    assert!(matches!(verdict, CheckVerdict::Incomplete { reasons, .. } if reasons.iter().any(|reason| matches!(reason, CheckBlocker::MissingEvidence { .. }))));
}

#[test]
fn a_plan_cannot_cite_a_check_that_never_compared_the_original() {
    let citation = CheckReference { check: "app-screenshot".into(), round: 1, claim: None };
    assert!(citation.verify(&[stored_missing_comparison(1)], 90.0).is_err());
    let earlier = CheckResult::new("app-screenshot", 1, true, "Compared the original.");
    assert!(citation.verify(&[earlier.clone(), stored_missing_comparison(2)], 90.0).is_err(), "an older comparison cannot mask missing current evidence");
    let repaired = CheckResult::new("app-screenshot", 3, true, "Compared the new reference.");
    assert!(citation.verify(&[earlier, stored_missing_comparison(2), repaired], 90.0).is_ok(), "a real passing repeat restores the citation");
}

#[test]
fn missing_comparison_survives_storage_and_legacy_checks_keep_their_policy() {
    let check = stored_missing_comparison(1);
    let document = serde_json::to_value(&check).unwrap();
    assert_eq!(document["not_compared"], true, "saving cannot silently remove the missing-evidence marker");
    let reread: CheckResult = serde_json::from_value(document).unwrap();
    assert!(!reread.good(90.0));
    let legacy = CheckResult::new("build", 1, true, "Build succeeded.");
    assert!(legacy.good(90.0), "a build-only result does not acquire an invented comparison requirement");
}
