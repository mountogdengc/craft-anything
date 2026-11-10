# Planned builds and completion

A build has a durable product plan in its planning extension. The builder, project screen and
completion decision read the same record. Provider-specific todo lists remain readable in older
projects, but they do not replace a saved build plan or prove that a requested feature works.

## Before implementation

When the project has no build plan, the run first asks the builder to study the available material
and save its phases, input inventory and acceptance criteria. It does not advance to an
implementation turn without a valid saved plan. This is a planning turn, not a request for the
person to approve routine technical choices. Existing projects can gain a plan when resumed.

The builder is instructed to preserve the original requested scope, use stable IDs when updating
the plan, and include meaningful subpages, interactions, data changes and persistence where the
request needs them. It must record missing evidence rather than infer a complete workflow from a
start-page screenshot.

## One record, three related views

- **Phases** have a stable ID, a title, a goal, implementation status and dependencies.
- **Inputs** explain what is missing, why it matters, what format would help, which phases need it,
  whether it is required, and what has been supplied. A required input remains unresolved when
  deferred. Optional inputs do not block completion.
- **Acceptance criteria** describe observable outcomes and distinguish pending, implemented and
  verified work. Marking a phase done or supplying an input does not verify a criterion.

A required input blocks its dependent phases. The builder can continue independent work. The
input inventory itself does not send questions: `core_person_ask` still uses the project's existing
question preference. In the project screen, **Provide details** prepares a reply in the normal
composer, where the person can add an answer or screenshots before sending it.

## Headless tools and persistence

Call `core_build_plan_show` to read `plan` and its independent planning `revision`. Save the
whole revised plan with `core_build_plan_update`:

```json
{
  "based_on_revision": 0,
  "plan": {
    "version": 1,
    "goal": "Keep unfinished notes available after reopening the app",
    "phases": [
      {
        "id": "drafts",
        "title": "Save unfinished notes",
        "goal": "Keep drafts and restore them in the notes list",
        "status": "todo",
        "depends_on": []
      }
    ],
    "inputs": [],
    "acceptance": [
      {
        "id": "restore-draft",
        "phase_id": "drafts",
        "description": "An unfinished note appears in the list after closing and reopening",
        "required": true,
        "status": "pending"
      }
    ]
  }
}
```

Use the actual current revision, not the example's zero. A stale revision is refused; reload and
merge the change before retrying. Cross-process locks and atomic writes protect updates. Planning updates do not advance the
analysis revision; analysis updates do not advance the planning revision. Progress updates retain existing phase IDs and required input and acceptance
IDs; a required criterion also retains its phase. These records cannot be removed or made optional
to obtain a completed result. Optional records may be removed. An explicit required-scope revision
workflow is follow-up work; the person can still accept a partial result with **It's done**.
The canonical data lives in
`investigation.cany/extensions/planning/state.json`; `extensions/planning/plan.md` is its readable
rendering. State has its own version and [schema](schemas/planning.schema.json). Embedded legacy
plans are read without rewriting their source. The first planning update or analysis save preserves
them in the extension record. A present extension record is authoritative even when unreadable. Corrupt, oversized or unsupported state is refused rather than
overwritten or treated as a completed plan.

Verified criteria must cite observed/derived evidence or actual recorded check results. Check
references identify the check, its recorded round, and optionally an exact claim. A later passing
repeat of the same check and claim revalidates the citation; a missing or failing latest result
does not. Fabricated evidence IDs and unsupported check citations are rejected.

## Native extension boundary

`crates/planning` owns the model, validation, completion policy and versioned store.
`engine::extensions` registers its tools and projects compatibility views through generic hooks.
`ai::rebuild::extensions::WorkflowExtension` contributes setup, builder instructions, refinement
and required readiness assessments. The default registry always includes planning; additional
required extensions can be registered without replacing it. A failed required assessment blocks
automatic completion and stops automatic refinement. Optional plugin checks are separate.

Reused upstream check results still pass through the current required extension assessments.
Changing the quality target, picture threshold, comparison widths or page limit invalidates the
check cache, so a result accepted under an older comparison policy cannot satisfy a stricter one.

Plans created before a project is open transfer with their evidence when a shared project is
created. Opening an existing project reads its own planning record instead.

## What completion means

Automatic **Ready** requires a nonempty current check round whose every result meets its basic
success condition, similarity threshold, claims and evidence requirements, with no unresolved
high or critical investigation question. The saved plan must
also have completed phases, supplied required inputs and verified required acceptance criteria.
No checks, any failing result in the current round, an unfinished phase or an unverified criterion keeps the
project at **Needs more work**. Available previews remain openable, and **Continue** resumes the
work. The final progress stage is not shown as done for an incomplete result.

The person's explicit **It's done** action remains available through `project_build_finish` and
records that the person accepted the current result. It is separate from automatic completion.

## Native observation and missing comparisons

GUI recreations need a usable picture of the original or a readable supplied reference before
the builder starts. With Run to study enabled, an executable target can recover a missing or legacy
window capture on a follow-up. Blank and unreadable pictures, and a modal dialog covering the
working interface, remain observation gaps. On Windows the capture tool can reuse one open
process with the exact executable path; it leaves that process open. Captures follow the selected
process and its descendants and report the process that actually owns the pictured window.

If runtime capture fails, readable supplied references still allow implementation. The study
records the capture limitation and discards partial runtime artifacts; cancellation stops the run.
In Deep mode, console programs recover recorded input/output runs instead of requiring a window,
and static package evidence can support implementation without a GUI capture. These paths still
need the applicable comparison and plan evidence before automatic Ready.

An app-screenshot check can establish that the build runs while still recording `not_compared`
when a reference comparison was expected but unavailable. This marker survives storage, blocks
the typed check verdict and acceptance citations, and ends automatic refinement without spending
more builder turns on missing reference input. The project keeps its preview and unfinished plan.
Adding readable references and completing a passing check can revalidate earlier legitimate
check citations; a citation that never supported its claim is still refused.

Runtime self-check attempts discard the previous attempt's window picture and UI tree before
capturing again. Capture failures remain unknowns, and cancellation remains cancellation. A
startup capture establishes one visible state; workflow coverage still requires its own evidence.
Native picture similarity continues to use the configured picture threshold.
Unreadable investigation metadata or question records also keep the result incomplete and prevent
a self-check from reporting that it would pass; a failed read is never an empty question list.

These changes adapt upstream [#973](https://github.com/storytold/craft-anything/pull/973) and
[#1032](https://github.com/storytold/craft-anything/pull/1032) to the durable planning contract.

This foundation does not automatically generate and execute tests for every arbitrary workflow.
The relevance and completeness of recorded criteria and evidence still need judgment. Existing
checkers may also report that a build or test command was unavailable; passing the available
checks does not establish coverage of behavior that was never exercised. Authentication-aware
reference collection, broader interaction/persistence checks, and invalidating evidence after
relevant source changes remain separate roadmap work.
