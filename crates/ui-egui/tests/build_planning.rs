use craft_anything_engine::Mode;
use craft_anything_engine::build_plan::BuildPlan;
use craft_anything_engine::workspace::Status;
use craft_anything_ui_egui::state::{FeedItem, PlanStatus, PlanStep};
use craft_anything_ui_egui::{CraftApp, build_plan, demo};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[test]
fn the_saved_plan_replaces_the_legacy_checklist_and_prepares_an_input_reply() {
    let plan: BuildPlan = serde_json::from_value(serde_json::json!({
        "version":1,
        "goal":"Keep unfinished notes",
        "phases":[{"id":"drafts","title":"Save drafts","goal":"Reopen unfinished notes","status":"doing"}],
        "inputs":[{
            "id":"fields","question":"Which fields should a note keep?","why":"The saved record needs those fields.",
            "format":"A short example note","phase_ids":["drafts"],"required":true,"status":"requested"
        }],
        "acceptance":[{"id":"restore","phase_id":"drafts","description":"An unfinished note can be reopened","status":"pending"}]
    }))
    .unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 1050.0)).with_max_steps(32).build_eframe(move |cc| {
        CraftApp::setup_context(&cc.egui_ctx);
        let mut app = demo::app(demo::engine(Mode::Beginner, true));
        demo::fill_project(&mut app);
        app.workspace.as_mut().unwrap().manifest.status = Status::Incomplete;
        app.facts.build_plan = Some(build_plan::View::new(plan, &[], 90.0));
        app.state.feed.push(FeedItem::Plan(vec![PlanStep { text: "Legacy repair list".into(), doing: "Repairing".into(), status: PlanStatus::Todo }]));
        app.state.project_input = "Keep this draft reply.".into();
        app.state.just_ask = true;
        app.state.plan_height = Some(480.0);
        app
    });
    h.run_steps(3);
    h.get_by_label("Project plan");
    h.get_by_label("0 of 1 phases implemented · 0 of 1 criteria verified");
    h.get_by_label_contains("Save drafts · Blocked");
    assert!(h.query_by_label("Legacy repair list").is_none(), "there is one visible source of product progress");
    h.get_by_label("Provide details").scroll_to_me();
    h.run_steps(2);
    h.get_by_label("Provide details").click();
    h.run_steps(3);
    assert!(h.state().state.project_input.starts_with("Keep this draft reply."));
    assert!(h.state().state.project_input.contains("Which fields should a note keep?"));
    assert!(!h.state().state.just_ask, "sending an input answer should reach the builder");
    assert_eq!(h.state().workspace.as_ref().unwrap().manifest.status, Status::Incomplete, "preparing a reply does not silently complete or start work");
}
