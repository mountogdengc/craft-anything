//! The project plan is a read-only view of the engine's canonical investigation record.
//! Answers use the normal composer; drawing never reads files or changes plan verification.

use crate::i18n::{tr, trf};
use crate::theme::{self, Tokens};
use crate::widgets;
use craft_anything_engine::build_plan::{AcceptanceStatus, BuildPlan, InputStatus, PhaseStatus};
use craft_anything_engine::workspace::CheckResult;
use egui::{Margin, RichText, Ui};
use std::collections::{BTreeMap, BTreeSet};

/// Computed on the project-facts worker. Drawing does not repeatedly validate a large plan or
/// scan check history for each criterion on every frame.
#[derive(Debug, Clone)]
pub struct View {
    pub plan: BuildPlan,
    phase_blockers: BTreeMap<String, Vec<String>>,
    verified: BTreeSet<String>,
    implemented: usize,
}

impl View {
    pub fn new(plan: BuildPlan, checks: &[CheckResult], good_enough: f64) -> Self {
        let phase_blockers =
            plan.phases.iter().map(|phase| (phase.id.clone(), plan.phase_blockers(&phase.id).into_iter().map(|blocker| blocker.message).collect())).collect();
        let verified = plan.acceptance.iter().filter(|criterion| criterion.verified(checks, good_enough)).map(|criterion| criterion.id.clone()).collect();
        let implemented = plan.phases.iter().filter(|phase| phase.status == PhaseStatus::Done).count();
        Self { plan, phase_blockers, verified, implemented }
    }
}

/// Returns an input question to place in the composer when the person chooses to answer it.
pub(crate) fn show(ui: &mut Ui, view: &View, t: &Tokens, height: f32, shown_id: egui::Id) -> Option<String> {
    let plan = &view.plan;
    let mut answer = None;
    widgets::card(t).inner_margin(Margin::symmetric(16, 12)).show(ui, |ui| {
        egui::CollapsingHeader::new(tr("Project plan")).id_salt("durable-build-plan").default_open(true).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&plan.goal).color(t.text)).wrap());
            let implemented = view.implemented;
            let verified = view.verified.len();
            ui.label(
                RichText::new(trf(
                    "{implemented} of {phases} phases implemented · {verified} of {criteria} criteria verified",
                    &[
                        ("implemented", &implemented.to_string()),
                        ("phases", &plan.phases.len().to_string()),
                        ("verified", &verified.to_string()),
                        ("criteria", &plan.acceptance.len().to_string()),
                    ],
                ))
                .size(theme::SMALL)
                .color(t.text_dim),
            );
            ui.add_space(6.0);
            let list =
                egui::ScrollArea::vertical().id_salt("durable-plan-details").max_height(height).min_scrolled_height(height).auto_shrink([false, true]).show(
                    ui,
                    |ui| {
                        for phase in &plan.phases {
                            let blockers = view.phase_blockers.get(&phase.id).map_or(&[][..], Vec::as_slice);
                            let label = if !blockers.is_empty() {
                                "Blocked"
                            } else {
                                match phase.status {
                                    PhaseStatus::Todo => "Pending",
                                    PhaseStatus::Doing => "In progress",
                                    PhaseStatus::Done => "Implemented",
                                    PhaseStatus::Blocked => "Blocked",
                                }
                            };
                            egui::CollapsingHeader::new(format!("{} · {}", phase.title, tr(label)))
                                .id_salt(("build-phase", &phase.id))
                                .default_open(phase.status == PhaseStatus::Doing || !blockers.is_empty())
                                .show(ui, |ui| {
                                    ui.add(egui::Label::new(&phase.goal).wrap());
                                    for blocker in blockers {
                                        ui.add(egui::Label::new(RichText::new(blocker).color(t.text_dim).size(theme::SMALL)).wrap());
                                    }
                                    for criterion in plan.acceptance.iter().filter(|criterion| criterion.phase_id == phase.id) {
                                        let verified = view.verified.contains(&criterion.id);
                                        let status = if verified {
                                            "Verified"
                                        } else if criterion.status == AcceptanceStatus::Pending {
                                            "Pending"
                                        } else {
                                            "Needs verification"
                                        };
                                        let requirement = if criterion.required { "Required" } else { "Optional" };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!("{} · {} — {}", tr(status), tr(requirement), criterion.description))
                                                    .color(if verified { t.success } else { t.text_dim })
                                                    .size(theme::SMALL),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                        }
                        if !plan.inputs.is_empty() {
                            ui.add_space(6.0);
                            egui::CollapsingHeader::new(tr("Inputs and answers"))
                                .id_salt("build-plan-inputs")
                                .default_open(plan.inputs.iter().any(|input| !input.provided()))
                                .show(ui, |ui| {
                                    for input in &plan.inputs {
                                        ui.push_id(&input.id, |ui| {
                                            let status = match input.status {
                                                InputStatus::Needed => "Needed",
                                                InputStatus::Requested => "Requested",
                                                InputStatus::Provided => "Provided",
                                                InputStatus::Deferred => "Deferred",
                                            };
                                            let requirement = if input.required { "Required" } else { "Optional" };
                                            ui.add(egui::Label::new(RichText::new(&input.question).strong()).wrap());
                                            ui.label(RichText::new(format!("{} · {}", tr(requirement), tr(status))).size(theme::SMALL).color(t.text_dim));
                                            ui.add(egui::Label::new(RichText::new(&input.why).size(theme::SMALL).color(t.text_dim)).wrap());
                                            let phases = plan
                                                .phases
                                                .iter()
                                                .filter(|phase| input.phase_ids.contains(&phase.id))
                                                .map(|phase| phase.title.as_str())
                                                .collect::<Vec<_>>()
                                                .join(", ");
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(trf("Needed for: {phases}", &[("phases", &phases)])).size(theme::SMALL).color(t.text_dim),
                                                )
                                                .wrap(),
                                            );
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(trf("Useful format: {format}", &[("format", &input.format)]))
                                                        .size(theme::SMALL)
                                                        .color(t.text_dim),
                                                )
                                                .wrap(),
                                            );
                                            if let Some(response) = &input.response {
                                                ui.add(egui::Label::new(RichText::new(response).size(theme::SMALL)).wrap());
                                            }
                                            if !input.provided() && widgets::link(ui, "Provide details").clicked() {
                                                answer = Some(input.question.clone());
                                            }
                                            ui.add_space(8.0);
                                        });
                                    }
                                });
                        }
                    },
                );
            ui.data_mut(|data| data.insert_temp(shown_id, list.inner_rect.height()));
        });
    });
    answer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Facts;
    use craft_anything_engine::workspace::{TargetRef, Workspace};

    #[test]
    fn plan_facts_use_saved_checks_instead_of_the_open_projects_old_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut current = Workspace::create(dir.path(), "Notebook", "Save notes", TargetRef::None).unwrap();
        current.manifest.checks.push(CheckResult::new("restore", 1, true, "The note reopened."));
        current.save().unwrap();
        let open_snapshot = current.clone();
        let investigation = current.investigation_dir().join("investigation");
        std::fs::create_dir_all(&investigation).unwrap();
        std::fs::write(
            investigation.join("state.json"),
            serde_json::to_vec(&serde_json::json!({
                "version":1,"revision":1,
                "build_plan":{
                    "version":1,"goal":"Restore saved notes",
                    "phases":[{"id":"storage","title":"Save notes","goal":"Restore a note","status":"done"}],
                    "acceptance":[{
                        "id":"reopen","phase_id":"storage","description":"The saved note reopens","status":"verified",
                        "checks":[{"check":"restore","round":1}]
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        current.manifest.checks.push(CheckResult::new("restore", 2, false, "The note is missing."));
        current.save().unwrap();
        let facts = Facts::read(&open_snapshot, None, 90.0);
        assert!(facts.build_plan_error.is_none(), "{:?}", facts.build_plan_error);
        assert!(facts.build_plan.unwrap().verified.is_empty(), "a failed saved repeat must revoke the displayed verification during the run");

        current.manifest.checks.push(CheckResult::new("restore", 3, true, "The note reopened again."));
        current.save().unwrap();
        let facts = Facts::read(&open_snapshot, None, 90.0);
        assert!(facts.build_plan.unwrap().verified.contains("reopen"), "a passing saved repeat restores the displayed verification");
    }
}
