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
