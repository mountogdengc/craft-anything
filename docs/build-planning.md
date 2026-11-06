# Planned builds and completion

A build has a durable product plan in its investigation. The builder, project screen and
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

Call `core_investigation_show` to read `build_plan` and the investigation's `revision`. Save the
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
merge the change before retrying. The existing cross-process investigation lock and atomic write
path protect updates. Progress updates retain existing phase IDs and required input and acceptance
IDs; a required criterion also retains its phase. These records cannot be removed or made optional
to obtain a completed result. Optional records may be removed. An explicit required-scope revision
workflow is follow-up work; the person can still accept a partial result with **It's done**.
The canonical data lives in
`investigation.cany/investigation/state.json`; `plan.md` is a readable rendering. State has a format
version and a published [schema](schemas/investigation.schema.json). Legacy unversioned
investigations load with defaults. Corrupt, oversized or unsupported state is refused rather than
overwritten or treated as a completed plan.

Verified criteria must cite observed/derived evidence or actual recorded check results. Check
references identify the check, its recorded round, and optionally an exact claim. A later passing
repeat of the same check and claim revalidates the citation; a missing or failing latest result
does not. Fabricated evidence IDs and unsupported check citations are rejected.

## What completion means

Automatic **Ready** requires a nonempty current check round whose every result meets its basic
success condition, similarity threshold, claims and evidence requirements. The saved plan must
also have completed phases, supplied required inputs and verified required acceptance criteria.
No checks, any failing result in the current round, an unfinished phase or an unverified criterion keeps the
project at **Needs more work**. Available previews remain openable, and **Continue** resumes the
work. The final progress stage is not shown as done for an incomplete result.

The person's explicit **It's done** action remains available through `project_build_finish` and
records that the person accepted the current result. It is separate from automatic completion.

This foundation does not automatically generate and execute tests for every arbitrary workflow.
The relevance and completeness of recorded criteria and evidence still need judgment. Existing
checkers may also report that a build or test command was unavailable; passing the available
checks does not establish coverage of behavior that was never exercised. Authentication-aware
reference collection, broader interaction/persistence checks, and invalidating evidence after
relevant source changes remain separate roadmap work.
