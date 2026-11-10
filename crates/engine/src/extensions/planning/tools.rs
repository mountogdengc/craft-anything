//! Planning tools registered by the native extension, shared by every client.
use crate::evidence::ToolError;
use crate::tool::{Ctx, Effects, Requires, ToolOutput, ToolSpec, Toolset, empty_schema, schema_of};
use serde_json::Value;

pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "core_build_plan_show",
            title: "Project build plan",
            summary: "Read the durable build plan and its independent planning revision. Phases, required inputs and acceptance stay separate from the analysis checklist. Use this revision as based_on_revision with core_build_plan_update. Reports stale evidence separately; unreadable or unsupported planning state is an error.",
            beginner: "See the project phases, missing information and what has actually been verified.",
            toolset: Toolset::Core,
            effects: Effects::READ_ONLY,
            requires: Requires::Nothing,
            input_schema: empty_schema,
            run: show,
        },
        ToolSpec {
            id: "core_build_plan_update",
            title: "Update the build plan",
            summary: "Save the goal, phases, dependencies, inputs and acceptance criteria in the planning extension. Read core_build_plan_show first and pass its planning revision as based_on_revision. Preserve all phases and required scope; verified acceptance needs real evidence or recorded checks. Recording an input does not send a question.",
            beginner: "Keep the project phases, missing information and checks together in one plan.",
            toolset: Toolset::Core,
            effects: Effects::MUTATES_SESSION.union(Effects::WRITES_FILESYSTEM),
            requires: Requires::Nothing,
            input_schema: schema_of::<super::BuildPlanParams>,
            run: super::update,
        },
    ]
}

fn show(ctx: &mut Ctx<'_>, _: Value) -> Result<ToolOutput, ToolError> {
    Ok(ToolOutput::Value(super::view(ctx.session)?))
}
