# Sub2: Isolated cross-lineage method transfer (2026-11-13)

## Objective

Make improvement methods themselves transferable and learnable **without** conflating donor success, source execution, recipient adoption, and method superiority. The edited controller is compiled into descendants. Frozen independent evaluation remains outside this controller.

## Changed call path

1. `rsi_meta_controller::peer_lineages` loads separately accepted designs with a matching frozen suite and aligned indexed source.
2. `synthesize` generates the next descendant's baseline meta policy and candidate source revisions.
3. `evolve_from_peer_methods` matches candidate role, exact path, source SHA-256, and frozen-suite SHA-256. It chooses **one** attributable donor rather than rewarding every donor named in the experiment. References to other donors remain uncredited hypotheses.
4. `rsi_evolve::worker` passes the selected policy into `peer_instruction(peers, policy)`. This renders only the selected donor; when transfer is inactive it renders no cross-campaign donor instructions. This protects the observational non-transfer comparison from this particular prompt-level contamination.
5. A rebuilt descendant may author its own source modification. Independent source evaluation decides adoption. No donor patch is imported automatically.
6. `feedback` records the assigned donor, exact source version, comparison profile, actual source focus, independent assessment, and recipient adoption separately. Later descendants update the editable meta method only from eligible executed observations.

## Matched negative transfer

The donor method's beta-smoothed recipient adoption estimate is compared against **non-transfer** attempts matched on role, source path, source revision, frozen suite, and executed focus. Each job counts once. At least three assessed donor trials and three assessed controls are required to suppress the exploration bonus when the donor's smoothed adoption is more than 0.12 below its matched control.

This comparison is **observational, not randomized**. It is a bounded exploration heuristic and **not a causal estimate or verified transfer superiority**. Model differences, task selection, and unmeasured context can still confound it. Such observations are explicitly labeled as unproven. The observations persist in `peerMethodLearning` even when a transfer proposal is suppressed, allowing a later descendant to redesign the improvement method.

Accepted recipient source credit additionally requires the original independent adoption checks, inclusion in the model-authored targetFiles, and an accepted source SHA different from the studied baseline SHA.

## Source-level regression tests added

The `rsi_meta_controller` tests cover:

- An assigned method changing the compiled policy's actual target and tool-facing directive, without pretending it earned adoption.
- Donor prompt isolation and removal of donor text from non-transfer baseline instructions.
- Exclusion of records with a mismatched donor assignment, source SHA, or frozen evaluation profile.
- Down-ranking after independently assessed recipient failures.
- Suppression of observed negative transfer against matched independently adopted non-transfer controls and retention of the negative observation.
- An independent feedback receipt refusing to attribute observations under mismatched profiles.

Focused verification command (not run in this change):

```bash
cargo test rsi_meta_controller::tests -- --nocapture
```

## Research signals informing design

- [HyperAgents](https://github.com/facebookresearch/HyperAgents) (2026): the meta-level method itself must be mutable.
- [Predict Before You Modify](https://arxiv.org/abs/2610.13991) (2026): predictions should be recalibrated from observed outcomes.
- [HELIX](https://github.com/HKUDS/HELIX) (2026): explicit intervention provenance, independent verifier results, and sibling negatives are useful components of a model/harness feedback cycle.

## Evidence boundary

These commits use `[skip ci]` as requested. Compile, full worker execution, repeated-real-model improvement, and overall recursive self-improvement completion were **not** established by this change. Avoid scoring a method as causally superior on the basis of observational matches alone.

## Judge-response extension: compilation and recursive method self-repair

The 2026-11-13 review identified a compilation blocker in `src/engine/rsi_system.rs`: literal JSON examples inside `format!` were interpreted as Rust format placeholders. Five brace pairs are now escaped in the compiled descendant instruction and an exact-literal regression test is supplied. This is source-level repair, not a claim that the latest HEAD has passed CI.

### Independent evidence replay, including rejected candidates

`rsi_system::revalidated_method_history(state, history)` now checks candidate ID, base commit, original frozen report source and evidence SHA, metrics/verdict equality, and `rsi::replayed_pair_metrics` for **both** accepted and rejected paired evaluations. Accepted generations additionally require the verified candidate and matched adoption commit/generator provenance, no production deployment, and a model-authored design fully covering candidate changed paths.

Without a replayable original receipt, it replaces the mutable evaluation metrics/verdict with nulls, marks acceptance/revalidation false, and clears the untrusted prediction-learning row. Thus a forged positive result, a forged negative result or case-specific measurements cannot train the next method policy. For measured records it recomputes the meta feedback from verified outcome data rather than inheriting stored Brier labels.

`rsi_evolve::compiled_system_generation_plan` now constructs one audited history snapshot and uses it throughout the descendant planner: source-grounded architecture blueprint and frontier, inherited domains, meta-policy forecasts, transferable method selection, and subsequent recursive planning.

### A recursive negative-transfer-to-method-repair edge

A matched comparison with at least 3 independently assessed donor experiments and 3 independently assessed non-transfer controls can mark a negative transfer. If the source-bound `rsi_meta_controller.rs` option exists in the descendant frontier and there have been fewer than 2 recent self-edit attempts, the native controller chooses `repair_own_transfer_method`, updates `target`, `controlPolicy`, forecast and revision, and asks the real descendant to rewrite the executable transfer-selection algorithm itself. The non-transfer/transfer evidence is retained with an explicit **observational, not causal proof** label.

`rsi_system::architecture_frontier` keeps the editable `rsi_meta_controller.rs` and `rsi_recursive_discovery.rs` source options reachable even when more than 12 small method files compete in the role. `rsi_evolve` carries the repair choice into the actual domain and isolates the indexed `rsi_meta_controller.rs` as the source-edit focus. Explicit owner focus and previously adopted successor programmes retain priority. Regression tests cover source discoverability, source-receipt rejection, JSON-literal rendering, meta-repair scheduling, donor attribution, and the negative transfer cooldown.

### Current evidence boundary

- GitHub commits and static call-path inspection verify that these changes were written to `dominion-sub2`.
- A historical failed GitHub Actions log identified the original Rust format-string error. New commits use `[skip ci]`; no new clean compile, worker loop or real-model descendant adoption is claimed.
- The matched-control signal is a search heuristic, not a randomized causal estimate; the post-repair benefit must be independently measured in subsequent generations.

Research context: [AIDE² (2026)](https://arxiv.org/abs/2609.26457) distinguishes actual self-rebuilt agents and sequential improvements; [EvoAgentBench (2026)](https://aclanthology.org/2026.findings-emnlp.2344/) separates procedural encoding, routing and uptake; [Faithfulness of Self-Evolvers (2026)](https://proceedings.mlr.press/v306/zhao26aw.html) motivates measuring real reliance on prior experiences rather than merely citing summaries.
