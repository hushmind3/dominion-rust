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
