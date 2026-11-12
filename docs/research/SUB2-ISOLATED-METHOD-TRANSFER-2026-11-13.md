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
