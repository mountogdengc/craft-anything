# Planning Extension Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to execute inline, with one independent final review.

**Goal:** Isolate planning behind narrow Rust extension hooks, preserve existing plans, then synchronize newer upstream history.

**Architecture:** A dependency-safe planning core owns typed policy and separately versioned storage. Engine and AI adapters register tools and required workflow hooks; the UI reads the adapter's view. The shared manifest follows upstream session semantics while preserving the fork's completion contract.

**Tech Stack:** Rust 1.99, existing Cargo workspace, serde/schemars, platform atomic writes and bounded locks.

**Spec:** `docs/superpowers/specs/2026-11-09-planning-extension.md`; the approved chat instruction and `docs/build-planning.md` define compatibility.

## Global constraints

- Work on `feature/r5.8-planning-extension-20261109`; retain the existing isolated worktree and its dedicated Cargo target.
- Claim and publish before production changes. Never commit or merge main; preserve experiments.
- Preserve all required scope, genuine verification, incomplete previews and cancellation.
- Extension state version 1; 32 MiB read/write cap; atomic writes; optimistic checked revisions.
- Required extension failures block completion. No dynamic-library ABI or new plugin runtime.
- No new external dependencies; register the planning crate at L3 in the layer table.
- Validation is scoped; no default full workspace or optional WASM-host test run.

## Review focus

- An analysis write before the first planning update must not erase an embedded legacy plan.
- A future/corrupt authoritative sidecar must not silently reactivate older legacy verification.
- Concurrent migration/update must preserve revisions and avoid reversing legacy/extension lock order.
- Cached upstream checks must not bypass new plan obligations or required-hook errors.
- Both historical format-4 variants must preserve approval choices, incomplete state and safe session-mode isolation.

### Task 1: Extract planning model and independent storage

**Files:** new `crates/planning/{Cargo.toml,src/lib.rs,src/model.rs,src/store.rs}`;
`Cargo.toml`, `Cargo.lock`, `xtask/src/main.rs`; engine planning compatibility re-export and adapter.

**Interfaces:** `RecordedCheck` exposes check id, round, threshold-aware success and named-claim
success. `State { version, revision, plan }` owns the extension envelope. `store::load(root)` is
read-only; `store::update(root, expected, plan, cancelled, validate)` validates under its lock and
atomically saves; `store::preserve_before_legacy_write(root, cancelled)` assumes the host's legacy
lock is already held and acquires only the extension lock. New core code must not depend on engine.

- [x] Add engine-level regression cases for sidecar writes and legacy preservation, observe RED:
      legacy revision 7 and required criterion IDs survive first extension update; legacy bytes
      remain unchanged by import; corrupt/future sidecar blocks read/update instead of fallback.
- [x] Move existing policy into the L3 crate; adapt recorded-check validation through the trait.
- [x] Implement bounded read, checked revision increments, locking, legacy import and atomic save.
- [x] Preserve scope/evidence validation and add direct storage cases for stale updates, racing
      writers, unavailable lock/cancellation and legacy overwrite prevention.
- [x] Run the planning crate tests and focused adapter migration tests; commit the tested extraction.

### Task 2: Move tools, workflow policy and UI reads to extension adapters

**Files:** `engine/src/extensions/{mod.rs,planning.rs}`, engine investigation/tool registration;
`ai/src/rebuild/extensions/{mod.rs,planning.rs}` and workflow call sites; existing UI planning view,
project facts and relevant integration tests; tools/schema and build-planning documentation.

**Interfaces:** `core_build_plan_show` returns the plan, independent revision and verification
issue; `core_build_plan_update` retains its input shape and scope/evidence enforcement.
Required workflow hooks supply preparation instructions, prompt contributions and completion/
refinement assessments. Their errors remain errors/blockers; absence of a saved plan requests
setup before implementation. The host loop knows generic extension assessment, not plan fields.
The adapter also owns a session-scoped `State` slot. A generic new-project hook transfers it
without replacing existing canonical or legacy planning data.

- [x] Add focused hook regressions, observe RED: required-hook failure and missing/corrupt state
      cannot produce Ready; passing built-in checks cannot mask a failed required assessment.
- [x] Move ToolSpecs, planning queries, evidence checks and renders into the engine adapter.
      Route analysis persistence through the generic legacy-preservation hook; update clients to
      use the dedicated planning revision. Preserve the old model import/tool ID where practical.
- [x] Move preparation, planning prompts, refinement and completion policy into the AI extension;
      retain only generic hook calls and generic incomplete-result handling in the host loop.
- [x] Switch UI facts and tests to the extension query; preserve supplied-input composer behavior.
- [x] Verify pre-project planning revisions, new-project transfer with usable evidence, and that
      opening another existing project cannot import unrelated scratch planning state.
- [x] Run planning, readiness, review-regression and UI planning cases plus affected headless tool
      registration checks. Regenerate changed tool/schema docs; commit the tested adapter extraction.

### Task 3: Integrate newer upstream without losing extension authority

**Files:** incoming upstream changes and remaining conflict sites; manifest migration tests;
extension integration with session modes, check reuse/deadlines and vision-aware prompts.

**Interfaces:** preserve upstream history in a two-parent integration commit. The required
extension still assesses completion after the current checks, including reused valid results.

- [x] Pin `e09888e14641d71eca7951ac6248fa207b16881a`; its one-commit delta from the audited
      snapshot concerns OpenCode unfinished-session cleanup, app startup and documentation.
- [x] Merge into this feature branch, resolve shared code by retaining upstream mechanisms and
      calling the generic extension hooks. Do not omit whole commits merely for text overlap.
- [x] Add migration regressions for fork/upstream v4 records and required-hook/cached-check
      integration; observe failures before the corresponding fixes.
      Unlabelled legacy sessions start fresh in Clean room, preserving files and plans; explicitly
      Clean room sessions retain their recorded mode and permitted resume behavior.
- [x] Run focused migration, planning, readiness, self-check and changed lifecycle regressions.
      Check affected CLI/MCP/desktop compilation; explain any broader validation that is needed.
- [x] Commit the tested integration with upstream authorship/history retained.

### Task 4: Review, verify and publish

- [x] Run scoped strict Clippy for planning/engine/AI/UI and affected entry-point compilation;
      formatting, layers, locks, staged secrets, relevant generated schema/tool checks and the
      shared WASM check for integrated L0-L2 changes. No full workspace suite by default.
- [x] Request one fresh whole-branch review. Resolve Important/Critical findings in one regression
      fix pass, with focused RED/GREEN evidence; record any deferred minor findings.
- [x] Update roadmap, STATUS, extension/migration documentation and the local devlog with exact
      tested/not-tested scope. Report reduction in shared-file planning coupling.
- [x] Push the named branch and open its PR into fork main, referencing the older PR as superseded
      only after the new PR exists. Leave main unmerged and retain the worktree for review.

Published as [PR #3](https://github.com/mountogdengc/craft-anything/pull/3); PR #2 is superseded. Main remains unmerged.
