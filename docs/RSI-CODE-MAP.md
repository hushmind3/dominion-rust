# RSI source map — lean checkout

Read executable behavior first. Historical design handoffs and one-off local measurements were removed from this branch after consolidation; Git commit history still holds their exact original versions.

| Concern | Live implementation |
| --- | --- |
| Durable multi-generation execution, compilation, work dispatch | `src/engine/rsi_evolve.rs`, `src/engine/rsi/execution/` |
| Frozen paired A/B, detached source receipts | `src/engine/rsi/evaluation/worktree.rs`, `src/engine/rsi/evaluation/metrics.rs` |
| Protected frozen verifier source boundary | `src/engine/rsi/proof/path_policy.rs`, `src/engine/rsi.rs::safe_path`, `src/engine/code_index.rs::is_source` |
| Measured method-off vs method-on successor outcome | `src/engine/rsi/proof/method_control.rs` (original model-usage audit, matched frozen paired comparison), `src/engine/rsi/proof/closure.rs` (shared authoritative gate) |
| Explicit method-off campaign execution | `src/api/rsi.rs::start` → `src/api/rsi/control_request.rs` → `src/engine/rsi/execution/start.rs` → `src/engine/rsi/execution/campaign.rs` → `src/engine/rsi/evaluation/worktree.rs` |
| Method-source classification for independent A→B→A proof | `src/engine/rsi/proof/source_roles.rs` (classification), `src/engine/rsi/proof/closure.rs` (receipt replay), `tests/unit/engine_rsi_closure_source_tests.rs` |
| Actual intelligence-role inventory, independent receipt replay | `src/engine/rsi_system.rs` |
| Source index and bounded model navigation | `src/engine/code_index.rs` |
| Editable improvement strategy | `src/engine/rsi_meta_controller.rs` |
| Target-bound forecast learning, self-repair arbiter and execution proof | `src/engine/rsi_prequential.rs` |
| Recurrent method rewriting / new method principles | `src/engine/rsi_recursive_rewrite.rs`, `src/engine/rsi_principle_learning.rs` |
| Measured contribution and experimental isolation | `src/engine/rsi/learning/credit_assignment.rs`, `src/engine/rsi/evaluation/intervention_lab.rs` |
| Automatic A-off control from normal adopted B | `src/engine/rsi/execution/campaign.rs` → `src/engine/rsi/lineage/auto_control.rs` → `jobs::enqueue_once` → `rsi_evolve::run` (no manual follow-up required) |
| Inherited source evidence and strategy archive | `src/engine/rsi/lineage/archive.rs`, `src/engine/rsi/lineage/transfer.rs` |
| Live model/provider selection | `src/engine/rsi/meta/model_ecology.rs` |
| Real-model execution receipt | `scripts/sub4-live-rsi.mjs` |
| Explicit-dispatch live verification | `.github/workflows/sub4-system-live-rsi.yml` |
| Unit tests extracted from production Rust files | `tests/unit/` (`engine_rsi_api_tests.rs`, `engine_rsi_closure_source_tests.rs`, `engine_rsi_path_policy_tests.rs`, `engine_rsi_method_control_tests.rs`, `api_rsi_tests.rs`, `api_rsi_capability_tests.rs`, `api_rsi_control_request_tests.rs`) |
| Integration tests (not model edit candidates) | `tests/e2e.rs` |

Verification status and historical independently measured descendant evidence: [RSI completion](rsi-completion.md) and [Sub3 evidence record](../benchmark/sub3-rsi-completion-2026-11-12.json). Current Sub4 design: [architecture](SUB4-RSI-ARCHITECTURE.md), [source-bound prequential learning](SUB4-PREQUENTIAL-SOURCE-LEARNING-2026-11-13.md).

**Scan boundary:** `code_index::collect_files` excludes `tests/` and test-only Rust files; `rsi_system::discovered_architecture_sources` skips test-only and frozen-oracle source. The native paired evaluator still runs its test corpus. None of these boundaries asserts performance gain.

## Execution responsibility map

The stable `rsi_evolve` public API dispatches into `src/engine/rsi/`:

- `planner/focus.rs`, `planner/worker_plan.rs`: source choice and compiled descendant plan
- `execution/start.rs`, `execution/context.rs`, `execution/campaign.rs`, `execution/worker.rs`: generation lifecycle and model tool turn
- `learning/methods.rs`, `learning/trials.rs`, `learning/research.rs`: reusable methods, empirical trials, bounded research recall
- `lineage/adoption.rs`: independently validated source adoption and next-generator target reconciliation

The `select_domain()` source and native differential test hook remain in `rsi_evolve.rs` because the existing deterministic source mutator requires that exact owned function.

The old flat `rsi_*.rs` helpers now live under `rsi/{planner,execution,evaluation,lineage,meta,learning}/`, while public `crate::engine::rsi_*` module names stay stable. New Rust method files in `src/engine/rsi/` are recognized without adding an exact-file allowlist.

The self-improvement page now shows automatic method-off job IDs from the
normal generation history and independent A→B / A2→B2 replay outcomes.

| Evolving experiment selection | `rsi/meta/experiment_evolution.rs::propose` → compiled `planner/worker_plan.rs` → executable factorial/negative/open mode → `execution/campaign.rs` → `execution/worker.rs` → `rsi_generation` → `lineage/history.rs` |
| Stable experiment-policy contract | `meta/experiment_evolution.rs::Contract` (serde deny-unknown, SHA-256 revision), `validate` + `implementation_matches` enforced by compiled planner, controller and worker |
