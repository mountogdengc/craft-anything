//! Required native workflow extensions. Optional plugin checks are a separate mechanism.
use super::*;

pub mod planning;
mod run;

pub(super) struct PreparationRun<'a> {
    pub extensions: &'a Extensions,
    pub prompt: &'a mut String,
    pub fresh: &'a mut Option<String>,
    pub keeper: Option<&'a dyn crate::cli_agents::AgentAdapter>,
    pub deep: bool,
    pub sees_pictures: bool,
    pub asked: &'a mut Vec<AskedQuestion>,
}

#[derive(Debug, Clone)]
pub struct Preparation {
    pub progress: String,
    pub prompt: String,
    pub next_prompt: String,
    pub missing: String,
    /// A corrective turn when setup returned without satisfying the persisted obligation.
    /// The host allows at most two corrections, and never infers completion from reply text.
    pub correction: Option<String>,
    /// API hint for corrective turns; persisted state remains the authoritative check.
    pub correction_tool: Option<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct Assessment {
    blockers: Vec<String>,
    refinement: Vec<String>,
}

impl Assessment {
    pub fn ready() -> Self {
        Self::default()
    }
    pub fn blocked(reason: String, refinement: String) -> Self {
        Self { blockers: vec![reason], refinement: vec![refinement] }
    }
    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }
    pub fn explanation(&self) -> String {
        self.blockers.join("\n")
    }
    pub fn refinement_prompt(&self) -> String {
        self.refinement.join("\n\n")
    }
}

pub trait WorkflowExtension: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &'static str;
    fn prepare(&self, _: &Workspace) -> Result<Option<Preparation>, ToolError> {
        Ok(None)
    }
    fn instructions(&self) -> &'static str {
        ""
    }
    /// Assess required obligations without changing them. Errors are blockers, never passes.
    fn assess(&self, ws: &Workspace, good_enough: f64) -> Result<Assessment, ToolError>;
}

#[derive(Debug, Clone)]
pub struct Extensions {
    items: Vec<Arc<dyn WorkflowExtension>>,
}

impl Default for Extensions {
    fn default() -> Self {
        Self { items: vec![Arc::new(planning::PlanningExtension)] }
    }
}

impl Extensions {
    pub fn register(&mut self, extension: Arc<dyn WorkflowExtension>) -> Result<(), ToolError> {
        let id = extension.id();
        if id.is_empty() || id.len() > 64 || !id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-')) {
            return Err(ToolError::invalid("A native extension needs a stable lower-case id."));
        }
        if self.items.len() >= 16 {
            return Err(ToolError::new(ErrorCode::OverLimit, "At most 16 required workflow extensions can be registered."));
        }
        if self.items.iter().any(|item| item.id() == id) {
            return Err(ToolError::invalid(format!("Workflow extension {id} is already registered.")));
        }
        self.items.push(extension);
        Ok(())
    }

    pub fn assess(&self, ws: &Workspace, good_enough: f64) -> Result<Assessment, ToolError> {
        let mut total = Assessment::ready();
        for extension in &self.items {
            let result = extension.assess(ws, good_enough)?;
            total.blockers.extend(result.blockers);
            total.refinement.extend(result.refinement);
        }
        Ok(total)
    }

    pub fn decorate(&self, prompt: &str, fresh: Option<String>) -> (String, Option<String>) {
        let instructions = self.items.iter().map(|extension| extension.instructions()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
        let add = |text: &str| if instructions.is_empty() { text.to_string() } else { format!("{instructions}\n\n{text}") };
        (add(prompt), fresh.map(|text| add(&text)))
    }
}
