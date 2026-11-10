//! Generic host orchestration for required extension preparation.
use super::*;

impl Worker {
    pub(crate) fn prepare_extensions(&mut self, ws: &mut Workspace, agent: &mut dyn Conversation, cx: PreparationRun<'_>) -> Result<Option<String>, ToolError> {
        let PreparationRun { extensions, prompt, fresh, keeper, deep, sees_pictures, asked } = cx;
        for extension in &extensions.items {
            let Some(preparation) = extension.prepare(ws)? else { continue };
            self.step(ws, Step::Study, Status::Inspecting, &preparation.progress)?;
            let setup = format!("{prompt}\n\n{}", preparation.prompt);
            let setup_fresh = fresh.take().map(|brief| format!("{brief}\n\n{}", preparation.prompt));
            let (setup, setup_fresh) = extensions.decorate(&setup, setup_fresh);
            let reply = self.turn(ws, agent, &setup, deep, setup_fresh);
            if let Some(adapter) = keeper {
                self.keep_session(ws, adapter);
            }
            let reply = match reply {
                Ok(reply) => reply,
                Err(error) => {
                    agent.stop();
                    if self.reset_progress_after_notes
                        && let Err(reset_error) = self.progress_after_notes(ws)
                    {
                        self.emit(RunEvent::Agent(AgentEvent::Status {
                            text: format!("The new request couldn't reset the unchanged-turn count ({}).", reset_error.message),
                        }));
                    }
                    return Err(error);
                }
            };
            if !reply.trim().is_empty() {
                ws.manifest.last_reply = reply.trim().to_string();
            }
            self.progress_after_notes(ws)?;
            if extension.prepare(ws)?.is_some() {
                let summary = format!("Needs more work. {} Continue to finish setup before implementation.", preparation.missing);
                ws.set_status(Status::Incomplete, summary.clone())?;
                self.emit(RunEvent::Step { step: Step::Study, text: summary.clone() });
                return Ok(Some(summary));
            }
            ws.save()?;
            *prompt = preparation.next_prompt;
            if let Some((added, more)) = self.added(ws, deep, sees_pictures) {
                asked.extend(more);
                *prompt = added + "\n\n" + prompt.as_str();
            }
        }
        Ok(None)
    }
}
