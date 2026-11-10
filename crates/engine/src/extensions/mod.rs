//! Native extensions registered at one composition point. Shared host code calls these seams.
pub mod planning;

type Investigation = crate::tools::investigation_tools::Investigation;
type Result<T, E = crate::evidence::ToolError> = std::result::Result<T, E>;

pub(crate) fn hydrate_project(root: &std::path::Path, view: &mut Investigation) -> Result<()> {
    view.build_plan = planning::load_project_state(root)?.plan;
    Ok(())
}

pub(crate) fn hydrate_session(session: &crate::Session, view: &mut Investigation) -> Result<()> {
    view.build_plan = planning::load(session)?.plan;
    Ok(())
}

pub(crate) fn hydrate_project_strict(root: &std::path::Path, view: &mut Investigation) -> Result<()> {
    view.build_plan = planning::load_project_strict(root)?.plan;
    Ok(())
}

pub(crate) fn hydrate_session_strict(session: &crate::Session, view: &mut Investigation) -> Result<()> {
    view.build_plan = planning::load_session_strict(session)?.plan;
    Ok(())
}

pub fn specs() -> Vec<crate::ToolSpec> {
    planning::specs()
}

pub(crate) fn project_created(session: &crate::Session) -> Result<(), crate::evidence::ToolError> {
    planning::save_scratch_project(session)
}

pub(crate) fn append_investigation_view(session: &crate::Session, body: &mut serde_json::Value) {
    planning::append_investigation_view(session, body);
}

pub(crate) fn preserve_before_investigation_write(session: &crate::Session, cancelled: &dyn Fn() -> bool) -> Result<(), crate::evidence::ToolError> {
    if let Some(project) = &session.project {
        craft_anything_planning::store::preserve_before_legacy_write(&project.root, cancelled)?;
    }
    Ok(())
}
