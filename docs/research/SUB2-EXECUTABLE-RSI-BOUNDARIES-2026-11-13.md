# Sub2 executable RSI boundaries — 2026-11-13

## Source change, not a score claim

Two structural changes on `dominion-sub2` preserve the existing API/jobs → child policy → source editing → frozen paired evaluation → accepted descendant path. They separate responsibilities that previously shared large Rust source files and make the experimental method-transfer repair target resolve to its **actual compiled implementation**.

- `rsi_meta_controller.rs`: source-grounded target selection and prediction logic, with `peer_transfer.rs` for donor selection/negative-transfer observations, `observations.rs` for feedback calibration and `guidance.rs` for model-facing instructions. Regression tests live in `tests.rs` and are removed from the runtime-editable source inventory.
- `rsi_system.rs`: system source inventory, ownership and indexed frontier; `evidence.rs` validates plan/lineage receipts, `blueprint.rs` constructs descendant architecture, `history.rs` replays independently verified observations, `planning.rs` accepts model-authored designs and inherits domains, `proof.rs` determines witnessed multi-generation capability, and `guidance.rs` generates model instructions.
- `rsi/planner.rs`: a failed donor-method transfer is now routed to `rsi_meta_controller/peer_transfer.rs`, while prediction-rule repair still targets `rsi_meta_controller.rs`. A targeted unit test checks priority conditions, preventing a self-repair from being silently redirected to a façade.

## Actual calls and evidence ownership

`compiled_system_generation_plan` still calls `rsi_system::revalidated_method_history`, `blueprint`, `architecture_frontier`, `rsi_meta_controller::synthesize`, `peer_lineages`, and `evolve_from_peer_methods` in the same order. The existing public names are re-exported from small Rust submodules, so the coordinator's call graph does not change.

Frozen source receipts and paired evaluation are still required; source placement, retrospective claims and tests alone cannot certify behavioral gain. The history-replay boundary remains distinct from the method that adapts future candidates. New source modules appear under `improvement_methods` in the runtime source inventory and the capped indexed frontier; `tests.rs` remains excluded.

## 2026 research used

- [HyperAgents](https://ai.meta.com/research/publications/hyperagents/) (2026-03-24): the algorithm for improving agents must itself be editable, not a permanently fixed supervisor.
- [Recuris](https://github.com/Gen-Verse/Recuris) (2026-08): source-grounded, localized experience updates plus independent held-out evidence.
- [EvolveNet](https://arxiv.org/abs/2608.04968) (2026-08): compose independently observed harness modifications using typed scope and evidence instead of treating every donor's success as transferable.

## Verification status

The tree/branch and function-preservation checks were performed against GitHub content. **No compile, build, CI, real-model test or full RSI generational proof was performed** for these commits. The focused Rust tests in the source are proposed regression coverage until actually executed. No model-weight learning, LoRA or fine-tuning path was added.

## Runtime and detached worktree boundary cleanup

- `src/engine/chat/runtime.rs` was structurally reduced from 6,984 to about 647 lines. The large live-turn function remains intact in `runtime/turn.rs`; adaptation, durable program execution, tool catalog, tool gateway, semantic finalization, session signals and tests are now separate child modules. Static checks matched all 73 non-test top-level declarations and original function bodies after visibility normalization.
- `src/engine/rsi_worktree.rs` now delegates to `rsi_worktree/{metrics,checkout,validation,candidate,feedback,promotion,comparison}.rs` while preserving the original entry-point names. Paired comparison and operator-controlled source promotion reside in separate modules. Tests were moved out of the runtime implementation.
- Worktree integrity and paired evaluation modules remain in the descendant's full source-inventory fingerprint, but do **not** appear as editable architecture targets. This maintains an independently held verification boundary after the file split. A source-level regression test asserts this distinction.

GitHub source commits use `[skip ci]`; static equivalence checks are not Rust compilation, external CI results, or a verified model-generated descendant success.

## Recursively editable method-learning and descendant planning modules

The source-level `rsi/learning.rs` responsibilities have been split into `learning/{selection,repairs,ablation,failed_edit,feedback}.rs`. These retain the original executed-method proposal/reuse, source-repair synthesis, outcome-bound verified write attribution, and future method feedback. Source-repair attempts still cannot earn accepted-improvement credit merely by being generated.

The `rsi/planner.rs` controller now delegates to `planner/{actions,strategy,edit_policy,compiled}.rs`. Action-outcome adaptation, generation target selection, source-edit rule calibration and compiled descendant planning can be independently redesigned without altering the frozen pair evaluator. The negative-transfer target regression test remains in the parent planner module. Re-exports preserve the previous internal entry points.

Static inspection confirmed 25/25 method-learning and 19/19 planner function declarations accounted for, with no missing cross-module names among those source files. These are **source-preservation checks**, not Rust type-check or runtime proof. A future independent judge should inspect the entire updated source tree and the true API/jobs/model/worktree path rather than score this document.

### Champion integration guidance

Preserve three material boundaries when selecting these changes:

1. The descendant's editable transfer algorithm is `rsi_meta_controller/peer_transfer.rs`, and an observed failed transfer must target that real implementation rather than the parent re-export façade.
2. Detached worktree source, comparison and promotion remain fingerprinted but excluded from model-editable architecture frontiers; they independently verify source changes.
3. A proposed new method, code-edit call, or research finding is not an adopted improvement. Only a verified source diff with independent paired evidence and a descendant able to execute its inherited method can establish that link.

All sub2 changes are isolated to `dominion-sub2` and committed with `[skip ci]`; the user did not request a build or a full-model CI run at this stage.

## 2026-11-13 judge 35/100 feedback: inheritable executable method closure

The independent judge awarded **Sub2 35/55 structural and 0/45 RSI** on the available evidence; the scores are not changed by author documentation or source-only commits. In response, this branch repaired a specific implementation failure and added a native, independently revalidated descendant-method feedback path.

### Source-backed invocation path

`src/api/rsi.rs` → `src/engine/jobs.rs::run` (`rsi_evolve`) → `src/engine/rsi/execution.rs::run_loop` → newly built `rsi/worker.rs::worker` → `rsi/planner/compiled.rs::compiled_system_generation_plan` → `rsi_meta_controller::synthesize` / `peer_transfer::evolve_from_peer_methods` → actual model edit gateway → `rsi_worktree/comparison.rs::compare_at` frozen paired evaluation → `rsi/lineage.rs::adopt` → following generated binary.

The previous module extraction left `rsi_meta_controller/observations.rs` calling private helpers in `peer_transfer.rs` without importing them. This is repaired by exposing them to sibling modules and explicitly importing them. The executable `peer_transfer.rs` source is also always included in the candidate alternatives when present in the frontier, avoiding silent loss under top-six truncation.

### Verified-method inheritance and recursive repair

- `rsi_system/method_lineage.rs::inherited_method_receipt`: requires an independently adopted ancestor with a model-authored design covering a recognized, actually called meta-method module. Both the inherited commit and current source bytes must match the accepted before/after SHA lineage. It records whether the rebuilt child changed target or mode and actually focused that new target, WITHOUT claiming causal gain.
- `rsi/worker.rs::worker`: computes the receipt against `revalidated_method_history` before the child model call, persists it in the job and provides the observed lineage in the actual tool-facing instruction when applicable.
- `rsi/execution.rs::run_loop`: persists `metaMethodInheritance` and `metaMethodOutcome` after the real child was evaluated, keeping assessed, adopted, and subsequent method rewrite distinct.
- `rsi_system/history.rs`: replays original paired measurement records and recomputes method inheritance/outcomes, discarding unmatched stored reports.
- `rsi_meta_controller.rs::inherited_method_repair`: upon two independently assessed unsuccessful successors in the same method/commit lineage and no adopted successes, it schedules a rewrite of the very meta-method source; `rsi/planner/compiled.rs::evidence_based_meta_repair_focus` and `guidance.rs` turn this into an executable source target and tool-facing task. Cooldown prevents an endless meta-only revision spiral.

**Boundary:** neither the success of an edited source nor an observed changed next target proves that the edited policy CAUSED a gain. Follow-up live-model, independent same-budget controls and multiple accepted descendant generations are still outstanding. This work does not execute CI, compile, run a benchmark, or connect model training.

### 2026 sources for architecture decisions

- [Meta HyperAgents, 2026-03-24](https://ai.meta.com/research/publications/hyperagents/): put task-solving and meta-improvement methods in the actual mutable execution program.
- [AIDE², 2026-09-22](https://arxiv.org/abs/2609.26457): an accepted source rewrite becomes the next agent, and hidden held-out evaluations determine admission.
- [RSI-Env, 2026-11-01](https://arxiv.org/abs/2611.01736): retain a persistent independent host controller for evaluation/versioning while accepted descendants take over subsequent improvement rounds. The cited system may also train models; this branch restricts RSI to source, policy, memory and harness changes with fixed base weights.

## Adoption-source provenance and independent descendant attempts — 2026-11-13

The method-inheritance receipt now requires `sourceLineageVerified`, recomputed by
`rsi_system/history.rs` from an independently replayed paired candidate and a
second source-hash ledger stored at **the actual worktree adoption action**.
`rsi/execution.rs` writes the adopted source SHA-256 map both to its generation
summary and to the candidate's `experimentAdoption.acceptedSourceHashes`.
The revalidator binds candidate ID, originating child job, origin, base commit,
adoption commit, and each mapped changed path before allowing the
`rsi_system/method_lineage.rs` inheritance claim.

Prior paired outcomes without the new adoption ledger are still replayed as
task feedback but are not retroactively credited with method-source inheritance.
The `rsi_meta_controller::inherited_method_repair` uses unique child job IDs,
so two copies of one assessed child cannot produce the two-independent-failure
signal that rewrites the executable improvement method. Regression cases cover
cross-job borrowing, modified SHA maps, and duplicate descendant observations.

This is a stronger provenance and independent-observation boundary; it is not
a cryptographic append-only record and does not prove that an edit caused a
quality gain. It follows the 2026 AIDE² principle of inherited accepted
agent revisions (https://arxiv.org/abs/2609.26457) and 2026 RRSI history-aware
proposal discipline (https://github.com/google-research/rrsi).
No test, compile, benchmark, workflow, or model-generation run was performed.
