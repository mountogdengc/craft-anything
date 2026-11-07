# Upstream Integration Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate reviewed upstream fixes and native observation improvements while retaining R5.8's durable scope, evidence requirements and incomplete previews.

**Architecture:** Merge upstream main at `511d85ffb043c9ead9064442fcc9d8f5d5024aba`, preserving the fork's build-plan engine and UI. Adapt upstream PRs #973 and #1032 into the existing typed completion verdict rather than adding a second completion path.

**Tech Stack:** Rust 1.99, Cargo workspace, egui, existing native capture helpers.

**Spec:** The upstream review and integration recommendation approved in this conversation on 2026-11-06; `docs/build-planning.md` specifies the existing completion contract.

## Global Constraints

- Preserve the primary checkout, local experiments and the experiments ignore rule.
- Work on `chore/upstream-integration-20261106`; never commit or merge to main.
- Keep previews available for incomplete builds and retain required plan scope.
- Advance craft.json to version 4 so both historical version-3 variants migrate without dropping approval choices or incomplete state.
- Native observation uses existing consent and process supervision; reused processes remain running.
- No paid-agent runs or uncontrolled desktop testing; synthetic fixtures provide repeatable checks.
- The referenced sibling craftrules checkout is absent. Follow this repository's instructions and record that limitation.

## Review Focus

- Historical upstream and fork version-3 projects preserve their settings and statuses after saving.
- A readable image or startup dialog does not establish workflow coverage or satisfy a missing comparison.
- Failed capture cannot reuse stale self-check artifacts as current evidence.
- Missing comparison and high-severity unknowns cannot bypass required plan obligations or trigger endless paid refinement.
- New language catalogs preserve the planning UI's translation coverage.

### Task 1: Claim and integrate merged upstream

**Files:** `.gitignore`, `docs/ROADMAP.md`, conflicted AI/engine/UI sources and catalogs, status and format documentation.

**Interfaces:** Preserve `Worker::finish_checks`, `CheckVerdict`, `planning::review`, build-plan tools and project UI; incorporate upstream approval fields and stopped feed steps.

- [ ] Record the R5.8 integration claim and publish the branch early.
- [ ] Build the unchanged baseline CLI and run the baseline workspace tests, recording any existing failures.
- [ ] Merge the pinned upstream main; resolve independent additions by retaining both sides.
- [ ] Preserve the experiments ignore rule and add planning messages to newly introduced catalogs.
- [ ] Verify formatting, translations and compilation before committing the upstream integration.

### Task 2: Protect manifest migration

**Files:** `crates/engine/src/workspace.rs`, `docs/schemas/craft.schema.json`, `docs/file-formats.md`.

**Interfaces:** `MANIFEST_VERSION = 4`, one migration per version, `Workspace::open/save/duplicate` preserve both incomplete status and approval preferences.

- [ ] Add behavioral migration/save/duplicate tests for fork-v3 and upstream-v3 projects, and future-version write refusal.
- [ ] Observe the migration tests fail under the merged version-3 format.
- [ ] Add the v3-to-v4 migration, update format documentation and regenerate schemas.
- [ ] Run the engine tests and strict Clippy; commit with the integration when green.

### Task 3: Adapt observation and comparison fixes

**Files:** Native capture/runtime helpers; `crates/ai/src/rebuild/{study,self_check,mod}.rs`, screenshot checks, observation regressions; engine completion verdict and build-plan verification.

**Interfaces:** Existing `CheckVerdict` is authoritative for missing comparisons. Final completion also requires no unresolved high/critical unknowns and a complete saved plan. Capture can recover a missing observation and optionally reuse one exact-match running process without closing it.

- [ ] Fetch the pinned PR heads for #973 (`f0ed9445563ebe9306ca566b7c51cffc9bc97b49`) and #1032 (`202d2f08501db8b452e2b32d7933c30b506e2d8c`).
- [ ] Bring over their focused regressions first; add integration cases for complete/incomplete plans and missing comparison.
- [ ] Confirm the new regressions expose missing observation/readiness behavior.
- [ ] Adapt capture, stale-artifact cleanup, missing-comparison metadata and builder guidance; preserve the fork's incomplete-preview path.
- [ ] Run targeted regressions and affected suites; document upstream attribution and remaining limits.

### Task 4: Verify and prepare review

**Files:** Roadmap/status/build-planning documentation, generated tool/schema docs, local devlog.

**Interfaces:** CLI built before AI tests; all workspace suites, strict Clippy, format, layers, locks, secrets, translation/tool/schema checks and shared WASM compilation.

- [ ] Run required checks with Rust 1.99 and the dedicated target directory; inspect and repair failures within scope.
- [ ] Request a fresh whole-branch review and address important findings with regression tests.
- [ ] Update roadmap/status and local devlog with exact evidence and limitations.
- [ ] Commit, push the named integration branch and open a PR into the fork's main; leave merging for review.
