# Planning extension and upstream synchronization

The user approved moving the fork's planning features into a Rust extension module with minimal
upstream hooks, followed by integration of the newer upstream changes. Reduce recurring overlap;
preserve behavior and existing projects. This is an extraction and integration, not additional
workflow features.

## Required behavior

- Retain phases, dependencies, required inputs, scope preservation, evidence/check citations,
  revision conflicts, incomplete previews, and the distinction between implemented and verified.
- Missing/corrupt/unsupported planning state never establishes Ready. Required extension failures
  remain blockers; optional WASM check failure behavior is not used for this authority.
- CLI, MCP, API builders and the desktop use the same planning tools and policy. Preserve
  `core_build_plan_update`; add a dedicated `core_build_plan_show` for the extension's revision.
- Migrate existing `investigation/state.json` embedded build plans without deleting or rewriting
  the legacy file as part of migration. Preserve evidence IDs, required scope and progress.
- New writes use `<investigation-project>/extensions/planning/state.json`, version 1, with an
  independent optimistic revision. A present sidecar is authoritative; corrupt/future sidecars
  cannot fall back to an older embedded plan. Read-only inspection must not create files.
- Before an analysis update can discard a legacy embedded plan, preserve that plan under the
  existing investigation update lock. Always acquire the legacy lock before the extension lock
  when both are needed. Cancellation and lock deadlines remain effective.
- Keep the existing UI panel and input-composer behavior, with background reads only.
- Bring in the newer upstream history after extraction. Preserve session-mode isolation, check
  caching/deadlines, vision-aware builder guidance and parser/process fixes. Reconcile both
  historical format-4 project variants explicitly; do not reinterpret unknown session modes as
  permission to resume a Deep session in Clean room.
- Keep local experiments ignored and preserve the primary checkout and the previous integration
  branch. Do not merge main. Prepare a new PR that supersedes the older integration PR once ready.

## Module boundaries

Use a `craft-anything-planning` L3 crate for the model, validation, bounded persistence and legacy
import. It depends only on evidence, platform and existing serde/schema libraries. A small
`RecordedCheck` trait lets the model validate host checks without depending on the engine.

Engine's `extensions/planning` adapter owns ToolSpecs and evidence/session integration. Keep the
old `engine::build_plan` import path as a compatibility re-export. Remove planning implementation
from investigation tools; one generic extension-preservation call protects legacy data before an
analysis write. Planning revisions are read from the dedicated show tool, not the analysis revision.

AI's `rebuild/extensions` owns required workflow extension registration, initial setup, prompt
contributions, refinement and completion assessments. The rebuild loop invokes generic hooks;
the planning implementation and prompt wording live in the extension. Required errors propagate.
The UI remains a thin renderer and calls the extension's public query API.

Generic missing-comparison/readiness protections and native-capture fixes stay in their owning
host layers. They are not copied into the planning extension or dropped during synchronization.

## Validation and limits

Use existing planning/completion regressions plus focused new sidecar, legacy-import, concurrency,
future-version, required-hook failure and combined-manifest migration cases. Build the CLI helper
before tests that start it. Check affected entry points, layer/lock contracts, generated tools and
schemas, formatting and staged secrets. Do not run the full workspace suite by default. Explain
any broader check before running it. No live paid-provider, person-app capture, or release test is
required for this extraction.

The sibling craftrules checkout remains unavailable. Follow the available project instructions.
