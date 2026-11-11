# Upstream review: November 10, 2026

Scope: 86 commits after `e09888e14641d71eca7951ac6248fa207b16881a`, through
`76b52be3f5b2e66c13f14bfbd55c1d6d7d40f3b4`. Fork baseline: `7474627`.
All 86 are selected; none is deliberately omitted.

## Review findings and integration decisions

| Area | Upstream changes | Integration treatment |
| --- | --- | --- |
| Builder/process lifecycle | Stop waiting after a completed protocol stream, bound stopped-process exit waits, answer interrupted checks, cap live logs | Retain; validate cancellation, taken-check lifetime and supervised process behavior |
| Capture modes | Record capture mode, reject cross-mode reuse, remove Deep-only material before Clean room study, reopen local targets for later Deep runs | Retain with fork native-observation recovery; use the run's single mode decision throughout study |
| Completion | Mechanics sheets no longer claim runtime behavior from source literals; unmeasured comparisons remain unknown and do not cause futile code refinement | Retain with fork CheckVerdict, essential-question errors and required extension assessments; none may turn an incomplete build Ready |
| Planning | Upstream does not introduce an alternative native planning store | Preserve the planning crate, storage adapters, required workflow registry, confirmed-save handoff and independent revision; the AI adapter classifies unavailable verification without dropping readiness blockers |
| Saved projects | Upstream capture modes and fork completion/session contracts independently use manifest version 5 | Write version 6; migrate missing legacy capture mode conservatively, preserve explicit modes, approvals, incomplete status and newer-format write refusal |
| AI setup | Request pacing, loaded-model preference, cancellable model discovery, saved-program diagnostics and key replacement | Retain; keep the confirmed-save request carried through model wrappers and normal provider request construction |
| Formats/evidence | Bounded retained parser data, signature identity/range validation, Swift symbols, texture formats, text-source reading and paged-result discovery | Retain; run affected format/evidence/tool regressions and shared WebAssembly checks |
| Translation/docs | New settings strings overlap appended planning strings and generated tool lists | Combine by source key, preserve both groups, regenerate schemas and tool documentation from the merged registry |

The merge has four code overlap areas: rebuild orchestration, self-check reporting, study and
workspace migration. Translation conflicts are additive. Generated documentation is regenerated
rather than choosing one side's tool/schema catalog.

The integration review found that old cached mechanics verdicts could bypass the new checker.
Saved source-literal verdicts are now rechecked. A plan blocked only by formerly verified citations
to currently unrefinable checks no longer spends repeated builder turns, while unfinished phases,
uncited criteria and other required extensions remain actionable. All obligations remain required
for Ready. The imported Unix mechanics test is adapted to the fork's Incomplete verdict.

## Upstream evidence

The exact pinned commit has a successful [Linux gate](https://github.com/storytold/craft-anything/actions/runs/42873752354)
and four successful [code-analysis checks](https://github.com/storytold/craft-anything/actions/runs/42873751833).
These establish upstream's own validation, not the fork integration's correctness. Local Windows
integration results and omissions are recorded in `STATUS.md` and the PR.

No live AI calls, user-target observation, credential changes or release deployment are part of
this integration. Existing user projects and the ignored experiments folder stay in the primary
checkout; all integration builds use a separate checkout and target directory.
