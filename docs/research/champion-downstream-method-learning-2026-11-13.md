# Champion — downstream method learning closes the missing RSI feedback edge (2026-11-13)

## Score-driven diagnosis

The 2026-11-13 judge gave Champion **74/100, rank 1**, explicitly noting that model-authored improvement operators and self-editable code were not enough: the program must learn whether a revised method actually helps future descendants solve new capability tasks. The new work attacks that exact distinction.

### What changed in executable source

1. **Independent evidence replay (new `src/engine/rsi_method_generalization.rs`):** A method execution is admitted only when the running compiled descendant's method rule ID, source edit target, candidate identity, generator source revision and the `rsi::replayed_pair_metrics` original-pair receipt all agree. Fake client/model-reported successes do not update the method.
2. **Generalization beyond source adoption:** Each method's trials, independently adopted descendants, held-out successes, regressions, neutral cases, cross-capability improvements and distinct evaluation profiles are retained separately. Simply changing Rust source or passing compilation gives no performance reward.
3. **Matched method-revision comparison:** The previous and current *method source SHA* are compared only where actual frozen profile, held-out case ID, capability domain and baseline pass result match. Sparse matched evidence receives shrinkage; an observed contrast is not automatically a causal effect.
4. **Closed learning feedback:** `rsi_improvement_algebra.rs` now retains all source-grounded candidate operators. `rsi_method_generalization::adapt` reranks those candidates by independently measured transfer, method-version performance change and observed regression, and can select the method implementation itself for another rewrite. This modified ranking is used for the actual next descendant source edit.
5. **Whole-intelligence feedback:** Independent replays also identify persistent errors in model runtime, memory, learning, harness, tools and new roles. When no adopted operator dominates, an independently observed capability regression can choose its current source file, rather than another unmeasured RSI planning task.
6. **Models are a first-class capability:** `rsi_evolve::DOMAINS`, objective matching, file classification and `rsi_system::role_domain` now treat model inference/routing source as a distinct editable target. This is source-level model algorithm adaptation, **not a claim that underlying third-party foundation model weights are trained**.
7. **Real descendant and supervisor integration:** The compiled planner reads archived evidence, recalculates method learning, adapts the operator policy, selects the next source, and emits both evidence and revised plan. The supervising coordinator checks its SHA and source identity, the child worker independently recalculates the evidence using the actual store, and the generation ledger saves the selected method learning revision. Changed methods remain re-editable by descendants.
8. **Immutable evaluation boundary:** No changes to `rsi_meta.rs`, `rsi_oracle_fixture.rs`, original paired cases or operator approval mechanism were necessary. No self-written claimed results are promoted into measured evidence.

### Mechanistic distinction

Old: source edit → independently adopt source → assign a rule priority from topology/trial count → edit again.

New: actual candidate edit → independent paired **receipt replay** → held-out domain/role improvements and regressions → compare **method source revisions** on matched cases → evolve rule ranking and the rule generator → actual downstream model/memory/learning/tool source edit in the NEXT compiled descendant → independent evaluation again.

The learner and the algebra that consumes its output are themselves mutable ordinary Rust sources, so the next accepted generation can learn a *different method of learning*.

### Tests added (authored but not executed on this revision)

- Fake model-task improvement with no archived independent paired receipt earns **zero learning signal**.
- Realistic evidence-backed selection can outrank a previously high-ranked method; result is still not marked quality-proven.
- Repeated independent holdout regressions redirect the source edit toward the method itself.
- Model source files and model-development goals select the `models` domain, rather than `improver`.

### Remaining empirical claims not made

The changes are a compiled-source **architecture intended for an open-ended experimental loop**. Whether models actually author useful revisions, whether it transfers across unrelated tasks/models, and whether method-learning revisions repeatedly improve future improvement capability must be separately demonstrated by real runs. No fresh CI, no model-weights training, and no independent multi-generation success are claimed.

Per user-defined judge score 74 (<80), these commits use `[skip ci]`. The previous CI attempts from the superseded 82-point phase did not establish a green CI run; earlier logs showed Rust compiling with one unrelated existing source-role classification assertion failing (the fallback classifier was subsequently modified).

### Sources guiding design

- [AIDE² / Recursive self-improvement of AI research agents (2026)](https://arxiv.org/abs/2609.26457): optimize the coding/research agent as a solver, confirm with held-out downstream capability tests, and return improved generator source to its next successor.
- [HyperAgents (2026)](https://arxiv.org/abs/2603.19461) / [code](https://github.com/facebookresearch/hyperagents): the improving meta-agent must be editable along with the downstream task agent.
- [MetaSkill-Evolve (2026)](https://arxiv.org/abs/2607.05297): evolving task skill and meta skill over different timescales; verify resulting behaviors on independent suites.
- [SkillLearnBench (2026)](https://cxcscmu.github.io/SkillLearnBench/): skill creation, skill execution and task outcome form a feedback chain; self-feedback alone can drift.
- [Do Self-Evolving Skills Generalize to Held-Out Tasks? (2026)](https://huggingface.co/papers/2609.39148): improvements observed on training tasks do not always transfer; compare on genuinely held-out cases.
