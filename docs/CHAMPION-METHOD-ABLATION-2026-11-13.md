# Champion 2026-11-13 — Live matched method control and A→B→A→B

## What changed after the 48-point judgment

The judge identified the missing independent method-use counterfactual. The existing source-replayed A→B→A proof was structurally strict but could not isolate a productive method from an otherwise capable code-editing model.

This update wires a **real control arm into the normal continuous RSI API** and preserves the original candidate/evaluation receipts so an independent reader may replay them. `src/api/rsi.rs::start` sets `rsiMethodAblation=true` by default for a continuous system-evolution campaign; `methodAblation:false` explicitly disables extra model calls and cannot earn whole-RSI proof through the new closure gate. Product/manual campaigns do not silently launch controls.

## Real invocation and evidence path

1. `POST /api/rsi/start` → `jobs::enqueue("rsi_evolve", input)`.
2. `rsi/execution/source.rs::run` constructs the detached experiment checkout and frozen source fingerprint.
3. `rsi/execution/campaign.rs::run_loop` compiles the **current descendant commit** and builds its genuine child controller plan.
4. `rsi/execution/method_ablation.rs::run_control` launches a separately persisted `rsi_generation` job **with the same compiled descendant executable**, model, provider, source commit, source directory, and frozen case plan. The control is launched as `--rsi-worker-job <id>`, not a simulated prediction.
5. In `rsi/execution/worker.rs::worker`, the OFF arm prevents actual compiled `rsi_operator_provenance::dispatch` and omits inherited method instructions. `rsi/learning/methods.rs::{configure_method_session,method_strategies}` also blocks executable procedure injection in that session. Source inspection, model coding, independent candidate creation and evaluation remain real.
6. `rsi_worktree::compare_at` independently evaluates any actual control candidate against frozen source cases. Its candidate and original pair receipt are preserved; **control changes are never adopted**. The normal ON descendant then executes and may earn independent source adoption.
7. `rsi/execution/method_ablation.rs::measure` checks the original OFF and ON worker jobs, exact pre-worker input seal, unchanged frozen profile and selected source revision, both original paired receipts and candidate-job bindings, no OFF method operations, and live ON model+tool+operator provenance. It counts paired frozen holdout task-quality wins/losses and labels the observation `methodAssociationObserved`, not `causalAblationProven`.
8. `rsi_system/replay.rs::revalidated_history` recomputes the control outcome from original storage before any descendant learning. `rsi_method_generalization.rs::learn` integrates only independently adopted and replayed positive control outcomes into method rankings and tracks OFF evaluation evidence hashes.
9. `rsi_system.rs::improvement_directive` binds B's actual control and treatment evaluation IDs and receipts to the next A2 method rewrite. After the revised method is independently adopted, `rsi_method_generalization.rs::adapt` routes a *new* downstream source task through the changed method (B2) rather than endlessly editing A2.
10. `rsi_recursive_closure::evidence` requires **4 contiguous independently adopted generations**: actual model-authored method edit A1, compiled descendant task improvement B1, feedback-linked actual method rewrite A2, and another real changed-method descendant B2. Both B generations need independently replayed ON/OFF worker wins, identical evaluation case/profile and provider/model across method revisions, and B2's observed control-relative advantage must exceed B1's. Original source method hashes and compiled successor commits must match at each handoff.
11. `rsi/proof/campaign_verdict.rs` remains the **single** source of `actualCompletion`, `capabilityDemonstrated`, campaign stopping and API audit; campaigns may still finish for budget/attempt limits without declaring RSI proven.

## Strict measurement distinctions

- `sourceEvolutionDemonstrated`: verified adopted source evolution, not generalized RSI.
- `modelDrivenCapabilityDemonstrated`: model and tool provenance-backed source improvements, not method causation.
- `methodAssociationObserved`: positive on/off outcomes from real workers with matched source/cases and original frozen receipts; the first control and revised-method control are both required for closure.
- `recursiveMethodClosureProven=actualCompletion`: actual 4-generation method-reuse feedback loop with observed *larger* B2 method advantage than B1.
- `causalAblationProven=false`: different stochastic model runs do not establish a randomized causal effect, even with matched inputs and baseline. Future repeated counterbalanced on/off trials can estimate sampling variance, and production release should use that stronger requirement.

## New and updated source tests (written, **not run**)

- `rsi/execution/method_ablation/tests.rs`: activate only actual downstream method trials; reject control-record fabrication; reject mismatched holdout metrics; keep the A2 rewrite separate from method use.
- `rsi_recursive_closure/tests.rs`: reject a fabricated fourth source adoption or a fake control success.
- Existing `rsi.rs` and `api/rsi.rs` tests still assert that mere accepted patches cannot become whole-system completion.

No tests, compilation, benchmarks, CI, local/remote model campaign or weight training were executed during this change. No measured gain or new judge score is claimed.
