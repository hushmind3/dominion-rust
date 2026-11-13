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
