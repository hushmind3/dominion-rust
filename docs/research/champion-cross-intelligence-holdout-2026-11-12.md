# Champion: source-attributed cross-intelligence feedback (2026-11-12)

## Structural distinction

The compiled descendant now learns from **two complementary, independently
bounded observation channels**, rather than interpreting every cross-component
redesign as a successful causal transfer:

1. **Intergenerational adoption:** `rsi_symbiosis::transition_graph` tracks
   A's accepted implementation followed by an actual rebuilt child attempting
   B and potentially returning to A. This supplies a lineage-level hypothesis,
   not a causal label.
2. **Within-generation held-out effects:** `rsi_interaction::influence_graph`
   maps *actual changed candidate paths* to separately reported frozen holdout
   outcomes by domain. An isolated modification yields a directional observation;
   multi-role edits are explicitly marked as jointly confounded.
3. **Editable hypothesis selection:** `reinforce_meta_with_holdout` uses only
   isolated cross-role observations to adjust source-editable meta target scores.
   Bounded, shrinkage-weighted adjustments cannot mint acceptances. Other-lineage
   references and A→B→A lineage evidence are then considered by the meta
   controller as separate signals.
4. **Cross-domain source co-design:** `coupled_portfolio` produces a current
   SHA-anchored set of distinct modules, even when they belong to different
   domain taxonomies. The rebuilt descendant's generated target list and prompt
   include these source-backed experiments; the current source index verifies
   the paths before editing.
5. **Descendant execution receipt:** The running compiled successor recomputes
   the graph from its recorded history, checks the model-editable planning
   output against that graph, validates the source-bound portfolio, and
   persists the actual changed candidate paths and experiment outcomes.

## Where implemented

- `src/engine/rsi_interaction.rs` — isolated/joint effects, evidence hashes,
  current-source portfolio and bounded meta adjustment
- `src/engine/rsi_evolve.rs` — planning, model context, execution validation,
  actual change receipts, cross-domain focus
- `src/engine/mod.rs` — includes the interaction learner in the executable

## Why this is a different capability

A successful A→B successor handoff does not establish *what* inside A increased
B's ability to improve. Aggregate wins conceal regressions on other domains.
The new feedback channel lets descendants distinguish negative transfer from
positive transfer and single-role observations from joint interventions,
then choose a new falsifiable redesign accordingly.

## Research connections

- HyperAgents (2026): task and metacognitive improvers are themselves
  editable source. https://arxiv.org/abs/2603.19461
- Mendel Gödel Machine (2026): compare multiple outcomes and lineages instead
  of mutating from one recent failure. https://arxiv.org/abs/2608.07645
- Predict Before You Modify (2026): retain task-level predictive feedback
  as the improvement model evolves. https://arxiv.org/abs/2610.13991

## Verification status

This series intentionally uses `[skip ci]`. Source changes and unit-test
cases for source attribution, confounded multi-role edits and evidence-bound
portfolios are committed. Compilation, runtime behavior, quantitative
improvement and a complete independent A→B→A recursive cycle have **not** been
established by these commits. No unexecuted plan is labeled an adopted gain.
