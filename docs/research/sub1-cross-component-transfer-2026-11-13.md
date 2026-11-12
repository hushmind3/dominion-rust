# Sub1: Source-grounded negative-transfer repair in the compiled successor (2026-11-13)

## Problem and architectural contribution

A whole-intelligence agent can improve a local task while damaging another
capability. The existing \`rsi_whole_agent\` ledger counts unresolved cases,
regressions, adopted holdout gains and directly rebuilt descendants. These
aggregates do not, by themselves, say which *source edit* threatens a different
capability. Repeated improvement of the most obvious failing role may therefore
miss a cross-role dependency.

Sub1 adds a counterfactual *interference-hypothesis* channel to the existing
compiled \`rsi_whole_agent::plan\` / \`valid\` / \`apply\` / \`instruction\` path,
not an orphan scorecard. A compiled child gets a two-source edit focus:

    source role A changed  -->  frozen held-out capability B regressed
             |                            |
        current source A              current source B
             \____________________________/
                   next paired repair trial

This is **not** evidence that A causally damaged B in an adopted successor.
Rejected candidate edits can expose possible interference, but cannot count as
inherited deterioration, method improvement, or successful self-improvement.

## Evidentiary boundary

An observation must have all of the following:

1. Existing independently paired evaluation, controller verification,
   evaluator evidence hash, comparison-profile hash and case-level holdouts.
2. An actual source-edit candidate with a model-authored design bound to
   its generator commit and job; exactly one changed, safe source path.
3. That path must equal a source in the current compiled intelligence frontier
   and be mapped to exactly one role by the generation's \`componentFiles\`.
4. A *different* capability's identical frozen evaluation has a previously
   passing case that now fails, or an explicit metric regression.
5. Evidence for one edge remains partitioned by evaluation profile.
   Duplicate cases in a single candidate do not multiply the evidence.
6. Two distinct independently evaluated candidates in the same profile are
   required before the edge may redirect the next generation's edit focus.

The newly selected phase
\`investigate_repeated_cross_component_negative_transfer\` chooses the
affected capability as primary and its source of interference as secondary.
Both exact source anchors enter the next generation's \`focus\`. Only the
original server-owned frozen evaluator can decide adoption.

## Recursion and interactions

- The actual binary containing \`rsi_whole_agent.rs\` computes the hypothesis;
  a parent-only planner does not get to invent a different result.
- \`rsi_evolve\` already freezes and checks this exact whole-agent plan with
  \`rsi_whole_agent::valid\` in its rebuilt-worker path.
- \`rsi_whole_agent::apply\` carries the pair into the executable edit focus.
  The ordinary source-hash grounding, model-authored system design, detached
  worktree edit and paired verification paths remain in force.
- The model is directed to contrast A-only, B-only and A+B changes under the
  same cases. Its improved algorithm or new capability can rewrite the
  interference learner itself in the next accepted source generation.
- Inherited model-authored stages and non-factorial scientific phases retain
  their existing precedence. This does not silently override a descendant's
  accepted redesign program.

## Deliberate limitations

This initial implementation does not assert causal identification, a passed CI,
a completed autonomous RSI cycle, a trained model weight change, or general
competence retention. It uses 2 witnessed single-path edits as an *exploration*
threshold, not statistical significance. The two-role repair must still be
authored, independently tested and adopted before being called an improvement.

### Source and test map

- \`src/engine/rsi_whole_agent.rs\`: \`cross_component_interference\`,
  \`plan\`, \`apply\`, \`valid\` and \`instruction\`.
- \`repeated_measured_negative_transfer_makes_next_child_edit_both_sources\`:
  proves the intended deterministic planning and focus behavior with synthetic
  witnessed records.
- \`missing_receipts_different_cohorts_and_multifile_edits_cannot_invent_an_edge\`:
  rejects mixed cohorts, missing receipts and ambiguous multi-source edits.

## Research rationale

- HyperAgents (2026): improvement machinery is editable alongside task code.
  https://arxiv.org/abs/2603.19461
- Mendel Gödel Machine (2026): comparative evidence over multiple trajectories
  is more informative than responding to a single local failure.
  https://arxiv.org/abs/2608.07645
- Predict Before You Modify (2026): the model of improvement dynamics itself
  can be learned and used to choose subsequent revisions.
  https://arxiv.org/abs/2610.13991
- CausalEvo (2026): paired interventions on a shared benchmark separate
  hypotheses about improvements from verified outcomes.
  https://arxiv.org/abs/2610.21016

These papers motivate architecture, not a claim that Dominion reproduces
their published results. The next experiment is an independently measured
three-arm source-edit comparison on the same frozen holdout profile, followed
by a rebuilt-child cross-role retention test.
