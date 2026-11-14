# DOMINION RSI: 2026 open-ended search architecture and implementation

Date: 2026-11-14. Target: native Rust source-level recursive self-improvement. This document describes code actually committed on dominion-sub2.

## Real source-code comparison

| Project | Inspected source | Distinct advantage | DOMINION implementation decision |
|---|---|---|---|
| US OpenEvolve | algorithmicsuperintelligence/openevolve: openevolve/database.py, config.py | MAP-Elites feature cells, island parent sampling, archive + diversity | Preserve independently verified paired Pareto results, add measured behavior niches |
| China EvoAgentX | ANative-Lab/EvoAgentX: evoagentx/optimizers/map_elites_optimizer.py, aflow_optimizer.py | Feature-indexed optimization and exploration vs exploitation | Native Rust fitness niche and verified lineage, not the Python runtime |
| ShinkaEvolve (Sakana AI) | SakanaAI/ShinkaEvolve: shinka/database/island_sampler.py, islands.py | Equal island sampling, diverse offspring, bounded migrations | Prefer less explored measured niche among otherwise independently eligible candidates |
| Darwin Gödel Machine | SakanaAI public results and DGM code/research | Non-greedy branching from archived stepping stones, independent benchmarks | Keep source archive across campaigns, no promotion from a model's own verdict |
| DOMINION native RSI | src/engine/rsi_archive.rs, src/engine/rsi_worktree/comparison.rs, src/engine/rsi/evaluation.rs | SHA-bound, frozen paired A/B, neutral Pareto archive, compiler-controlled descendant | Extend selection, do not introduce a second evaluator or external agent SDK |

Sources:
- https://github.com/algorithmicsuperintelligence/openevolve/blob/main/openevolve/database.py
- https://github.com/ANative-Lab/EvoAgentX/blob/main/evoagentx/optimizers/map_elites_optimizer.py
- https://github.com/SakanaAI/ShinkaEvolve/blob/main/shinka/database/island_sampler.py
- https://sakana.ai/dgm/

## Problem: held-out benchmark feedback can leak via evolutionary selection

A frozen independent evaluator may appropriately use held-out cases to
accept/reject a code revision. However, when its per-case hidden scores also
choose the **next parent**, the mutation search may adapt to held-out task
patterns indirectly, even if prompts contain no hidden test text.

The former archive fitness used every measured case. This allowed the next
parent's Pareto placement to depend on a held-out score. The revised Rust
selection computes **both training Pareto fitness and behavioral niches only
on case records with holdout != true**. The independent full paired evaluator
is unchanged and remains the sole authority for adopting improvement.

## Implemented selection mechanics

1. Persist only compiled, independently compared neutral source candidates
   with matching source branch, base SHA, patch SHA and paired evidence receipt.
2. Recompute real measured training-case fitness from replayed paired data.
   A candidate cannot submit its own measured scores or declare quality.
3. Assign a deterministic niche using the strongest positive relative
   measured change among training case/metric pairs. Holdout cases cannot
   affect this coordinate. A missing training gain yields train:neutral.
4. Select from the nondominated training Pareto frontier.
5. Prefer niche with the fewest observed descendant-generation job IDs,
   then parent with fewest descendant attempts, then stable ID order.
6. Feed the archived parent into the existing compiled descendant engine.
   Do not change frozen oracle, model weights or source adoption criteria.
7. Preserve the existing selection policy identifier for compatibility,
   and expose searchStrategy, explorationNiche, nicheCount,
   nicheDescendantAttempts and descendantAttempts as audit metadata.

Changed Rust file: src/engine/rsi_archive.rs.

## Tests and evidence boundaries

Rust tests verify: holdout metric mutation cannot change Pareto search fitness
or niche; independently measured candidates with orthogonal strengths occupy
different niches and switch after an observed offspring; duplicate patch,
dominated source and wrong-branch replay defenses remain in place.

These are source-verified regression tests, NOT a claim that the new selection
already improves SWE-bench solve rate. Existing actual-model scripts:
scripts/sub2-live-rsi.mjs and scripts/sub2-system-live-rsi.mjs.

Next causal ablation: compare old parent-only offspring balancing to the new
niche+parent balancing, with identical frozen tasks, model, source baseline,
token budget, total calls and independent held-out oracle. Log actual source
hashes, accepted descendant reuse, tool costs and solved test cases. Adoption
or increased scores cannot be inferred from an archive selection change alone.

## Configurable search-strategy ablation

The operator can select either strategy without changing Rust source or any
held-out evaluation fixture. Set DOMINION_RSI_PARENT_SEARCH=parent_only_v2
to reproduce the original least-used-parent selector. Leave the variable
unset or set DOMINION_RSI_PARENT_SEARCH=measured-niche-diversity-v1 to
select the new least-explored-training-niche policy. Both paths share the
same admissible candidates, training-only Pareto frontier, branch/source
and independent paired receipts, and frozen holdout promotion rule.

The selected strategy is captured in each candidate search receipt
as searchStrategy. Rust regression tests demonstrate that both strategies
choose different candidates on the same controlled state, without changing
any acceptance criterion. This makes an experimental ablation possible;
it does not itself establish a performance improvement.
