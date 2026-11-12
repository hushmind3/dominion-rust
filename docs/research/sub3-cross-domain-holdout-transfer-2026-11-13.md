# Sub3 — isolated cross-domain holdout transfer in the compiled meta-controller (2026-11-13)

## Motivation and prior art

- AIDE², *Recursive self-improvement of AI research agents* (2026): requires gains to transfer beyond the selection task. https://arxiv.org/abs/2609.26457
- Reglos, *A Self-Evolving Personal Memory Agent with Recursive Self-Improvement* (2026-11): a patch that repairs a visible failure may not generalize. https://arxiv.org/abs/2611.06049
- Existing Sub3 has a compiled successor planner, independently paired evidence, a joint-intervention-aware interaction graph, inherited improvement programs, and a mutable meta controller. This work connects **isolated, other-domain holdout results** to the successor's ordinary target ranking.

## Code, not just a research proposal

`src/engine/rsi_meta_controller.rs` computes `generalization_evidence` for each source-backed frontier file. The evidence requires:

1. A separately paired, evaluator-verdict-bearing observation, with a candidate ID.
2. The exact file to be present in `changedSourcePaths`.
3. Precisely one source role, attributed from real changed paths or an authentic model-authored role-to-file mapping via `rsi_interaction::actual_roles`. Joint interventions are *not* credited.
4. A holdout case with explicit baseline/candidate pass booleans, from a **different normalized domain**. Domain aliases such as memory/recall are kept in the same family.
5. Per-case regressions dominate simultaneous metric improvements; unpaired, missing, in-domain and non-holdout claims earn no cross-domain credit.

For each target, a bounded per-domain ledger records separately observed gains, regressions and neutral cases. The descendant's own, editable source computes the shrinkage-adjusted search signal:

```
adjustment = (0.20 * other_domain_gains - 0.44 * other_domain_regressions)
             / (number_of_other_domain_holdout_cases + 4)
```

No evidence gives adjustment zero. For observed repeated transfer losses the compiled policy can choose a `cross_domain_generalization_repair` investigation. The selected evidence travels in the signed meta policy, appears in the model's redesign instructions and is retained in next-generation `metaLearning` feedback.

## Role in the RSI system

This is a **next-goal learning mechanism**, not a change in the frozen evaluator or an automatic acceptance threshold. Its weights and source are themselves editable by adopted descendants. It makes evidence from *other capabilities* part of their future search and method redesign, while preserving attribution limits. The existing acceptance and lineage receipts remain independent.

Tests cover: an independently measured other-domain loss, alias filtering, non-holdout filtering, confounded multi-role changes and unpaired changes. The exact old-and-new policy revision is hashed before supervisor acceptance.

## Status and limits

Implemented on `dominion-sub3` in commit `86ec2f53907ffac17c2e8f8c50821d8a5a2a4f2e` (plus this documentation). Marked `[skip ci]` as requested. New unit tests and Rust compilation have **not** been executed; no live-model generalization or performance improvement is claimed. The evidence graph is observational (not causal proof). Cross-domain usefulness must be measured with real campaigns, genuinely unseen tasks, and recompiled successors.
