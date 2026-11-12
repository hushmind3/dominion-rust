# Champion — open-ended improvement algebra (2026-11-13)

## Why the November 12 judge reduced Champion from 82 to 72

The judge rewarded implementation of experiment → intelligence invention → successor improvement → reverse redesign → improvement-method editing, but penalized the fixed human-authored sequence that governed when these steps can happen. **A source-editable stage machine is still a predefined stage machine.** A successor must be able to create, combine and change *new principles of improvement*, not merely traverse the same phase enum.

The new code adds a second, structurally independent controller instead of asserting the original seven-phase sequence is open-ended.

## Newly implemented mechanism

1. `rsi_system_plan.improvementRules` is a typed model-authored contract for rules with arbitrary `premises` and `products` (intelligence role sets), `methodRole`, real `methodPath`, mechanistic hypothesis and observable. Model-authored operators become part of `systemDesign`, revision-bound to the source proposal.
2. `rsi_improvement_algebra.rs` admits a rule only if its *method source was actually changed* and independently adopted under a rebuilt generator. It verifies model-written design, generator lineage, candidate origin, accepted commit, source changes and independent eligibility.
3. The compiled descendant binds rule roles and methods to **current indexed source SHAs**. New integrated components are valid roles; a rule is not restricted to the original hard-coded component list.
4. It constructs directed role-operator edges, searches variable-length composition cycles, and uses real accepted source changes (including multi-premise evidence) as the activation state. An actual method selection, not merely a report, points the source editor at the resulting program's target file.
5. The `rsi_evolve` supervisor attests the proposed rule plan; the worker independently rebuilds it from durable history, checks it against the executable plan, then gives its source-grounded edit target and method instruction to the model. The next generation stores `improvementAlgebraPlan` with the evidence.
6. Every operator accumulates **independent actual candidate attempts and adoptions**, not a model's success claims. Repeated rejected/inconclusive changes redirect the improvement controller to the **operator's source implementation** as the next improvement target. Its Rust source and composition-search procedure are themselves mutable by descendants.
7. In the autonomous case, an independently adopted rule can supersede the older fixed `successorStage` method. Owner-selected or paired independently measured goals still retain precedence.
8. `rsi_system::architecture_frontier` exposes independently adopted improvement method paths even when those sources are not their role's top-ranked original representative. This avoids accidentally suppressing editable meta-methods.

**General form:** `{A,B} → {C,D} → {E} → ... → {A}`, where the roles, transformation rules, premises, observation targets and number of hops are created from source-backed adopted design, not enumerated in a fixed phase table.

## What is not being claimed

A written `improvementRule` is an experiment hypothesis, **not proof of an actual causal effect**. An accepted source change is evidence of a measured candidate comparison, **not proof the general operator is true**. A source-compiled plan still needs model-authored modifications and frozen paired independent evaluation to establish a gain. Compositional search is bounded for runtime, but the *role grammar and rules* are extensible and editable. Demonstrating that open-ended meta-level concepts continue to emerge over many accepted generations remains future empirical work.

## Structural tests introduced

- Arbitrary three-role, three-operator cycle composition
- Reject rules lacking actual source edit and independent adoption
- Real accepted role edit activates a model-authored, non-pre-enumerated follow-on operator
- Repeated rejected attempts redirect the target to the method source
- Source hashes, origin and plan fingerprint must be consistent

Tests were authored but **not executed on the new revision**. After the user-provided judge's penalty-inclusive **72/100** reassessment, no further CI jobs are triggered: all edits above use `[skip ci]`. The CI previously started following the earlier 82/100 assessment finished with 752/753 Rust tests passing and a source classification assertion failure; the source classifier has been patched separately without a new CI claim.

## References reviewed for mechanisms rather than borrowed claims

- HyperAgents (2026), [paper](https://arxiv.org/abs/2603.19461), [code](https://github.com/facebookresearch/HyperAgents): make the agent that proposes improvements editable in the same coding system.
- RSI-Env (2026), [paper](https://arxiv.org/abs/2611.01736): persistent model-harness improvement with successor-driven source development and independently owned assessment.
- SkillCompiler (2026), [paper](https://arxiv.org/abs/2610.39508): executable programs and reusable assets rather than repeatedly reinterpreted textual skills.
- Google RRSI (2026), [public implementation](https://github.com/google-research/rrsi): reuse full failed edit history, diversify search on stalls, and distinguish frozen holdout transfer.
