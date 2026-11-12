# Sub4: Source-bound prequential meta-learning (2026-11-13)

The prior controller used an aggregate adoption label and accepted Brier errors without binding each historical prediction to the selected source. This could reward a policy for an accepted edit to a different component, or calibrate against unevaluated, stale, or mismatched observations.

## Executable path

- The compiled descendant forecasts adoption in `rsi_meta_controller::synthesize`; `rsi_prequential::calibrate` corrects that prior from up to 32 *independently replayed*, policy-bound, candidate-bound, source-attributed observations.
- `rsi_meta_controller::feedback` requires a changed selected source path and a verified role-specific adopted design before it records a successful target adoption.
- Feedback excludes missing paired receipts, compiler failures without a paired evaluation, substituted plans, mixed origins, and invalid adopted receipts. A successful patch elsewhere is recorded as an **off-target outcome** for the intended source.
- Historical observations on the exact path and original file SHA carry full weight. Different file revisions receive 0.25 transfer weight; all samples decay by recency. Two virtual prior trials prevent one lucky outcome from overwhelming the choice.
- The posterior is used in the next descendant's **actual ranked source-choice score, forecast and source-editing instruction**. The method itself is a new editable Rust module, discoverable as an improvement-method source.
- The immutable paired evaluator remains authoritative for adoption; posterior probabilities are planning estimates, never adoption receipts.

## Research context

Designed with the prequential prediction/update loop in *Predict Before You Modify* (arXiv:2610.13991) and self-editable meta-agents in *HyperAgents* (arXiv:2603.19461). The source-specific, evidence-gated observer is a Dominion implementation, not a reported reproduction of either study.

## Verification

New unit tests cover plan mismatch, missing independent receipt, off-target acceptance, source-shift discounting and forged-history exclusion. No full Rust build, CI or live-model RSI run was executed as part of this source change.

## Recursive prediction-method repair

Four or more source-bound, independently replayed prequential forecasts in the six most recent assessed observations, with mean Brier error above 0.35, add a bounded 2.0 search bonus to the *executable* predictor module. That bonus changes the compiled successor's actual ranked target and model instruction; it does not claim that a source modification will help. A measured predictor-target attempt within the four latest generations imposes a cooldown. An owner-restricted domain or verified inherited successor stage remains authoritative.

## Historical receipt replay

`rsi_system::revalidated_history` now reconstructs `pairedMeasured` for both accepted and rejected candidates from the original persisted `rsi_pair` receipt via `rsi::replayed_pair_metrics`, checking the base commit, metrics, verdict and evidence SHA against the evaluated candidate and generation. It restores changed paths from the evaluated candidate and recomputes the prequential label at history-read time. A synthetic history flag or edited Brier score cannot become an independent observation merely by being persisted.

The predictor-drift and receipt-replay contracts have corresponding Rust unit tests in `rsi_meta_controller.rs` and `rsi_system.rs`; they are authored but not executed in this pass.

## Model-written execution and independent receipt closure

A compiler-issued repair *decision* is now reconciled by `rsi_prequential::arbitrate_repair` **after** source principle composition and recurrent method scheduling, before the factorial executor. A permitted automatic repair persists as an actual `metaPolicy.target`; concurrent principle/recursive target claims are explicitly deferred. A verified owner-selected successor stage and restricted objective have precedence. The real worker's `metaPolicyExecuted`, exact `focusFiles`, evaluated candidate paths, replayed A/B receipt and source adoption are separate predicates, not interchangeable counters. A source-bound prediction is censored if the worker focused elsewhere or executed another policy revision.

`rsi_evolve` persists `predictionRepairEvidence` on each generation. `scripts/sub4-live-rsi.mjs` retains it in the unedited real-model result and separately counts model-authored prediction-source edits, adopted source revisions, subsequent rebuilt descendants and later accepted edits to another capability. A *causal advantage* of the repaired method is explicitly unproven without a matched no-repair control. CI's live-model workflow is now **dispatch-only**, avoiding unrequested provider usage. A strict optional `require_prequential_repair` gate fails when the actual replay/adoption/successor evidence is absent. Unit tests also require focus-file and policy-revision binding and verify that scheduling itself never fabricates an adoption receipt.
