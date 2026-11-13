# Sub4 RSI — active architecture (2026-11-13)

This is an entry point to executable code, **not** an adoption receipt. The previous dated Sub4 micro-handoffs were consolidated here to avoid repeated, often stale descriptions of the same system.

## Recompiled descendants

`rsi_evolve.rs` creates detached worktree generations. `rsi_worktree.rs` verifies actual changed source and executes the frozen paired evaluator; `rsi_system.rs` independently replays the original receipts for accepted **and rejected** proposals. The descendant records its generator commit, selected model, source hashes, executed plan and evidence. `rsi_archive.rs` retains eligible source-verified alternatives across campaigns, not text-only successes.

## Mutable methods that can improve one another

- `rsi_meta_controller.rs` chooses and forecasts source interventions from independently measured, source-attributed credit.
- `rsi_prequential.rs` calibrates the forecast and, after repeated measured prediction error, arbitrates an *actual experiment* on its own estimator implementation. Planned, actually edited, independently adopted and rebuilt-successor events are separate.
- `rsi_recursive_rewrite.rs` carries accepted capability innovations into later method rewrites.
- `rsi_principle_learning.rs` revises executable improvement principles using later outcomes. `rsi_intervention_lab.rs` isolates ambiguous multi-module effects.
- `rsi_model_ecology.rs` selects among available fixed-weight model actors using independently observed task outcomes. A provider/model switch is different from training weights.

## Operating boundary

`scripts/sub4-live-rsi.mjs` emits real-model calls, original source writes and model-authored adoption evidence; `.github/workflows/sub4-system-live-rsi.yml` starts this resource-intensive test **only on explicit dispatch**. The last judge-confirmed score was 79. This document does not imply that current HEAD compiled, passed CI or completed a real-model successor chain.

Unit suites live under `tests/unit/`; integration checks remain in `tests/e2e.rs`. Source indexing excludes the tests and frozen oracle. The original historical reports remain recoverable from Git history.

## Fixed-weight-only RSI guarantee

The local LoRA/PEFT training and promotion module, training/evaluation/server scripts, associated tool registration, generation inference sidecar, model-adapter receipts and weight-based principle reward were removed. No running RSI path writes model parameters. `rsi_model_ecology` still experiments with **configured inference routes** to fixed pretrained models, using descendant source-change outcomes. The recursive loop evolves code, policy, memory, tools, and model-routing logic, not model weights.

## Physical module boundaries

`rsi_evolve.rs` is now a small API surface plus native selector template. Generation planning, model execution, research, empirical method trials, and adopted-source lineage logic live in `src/engine/rsi/`. Worktree evaluation, archived source evidence, fixed-weight model ecology, credit assignment, and method rewriting helpers are also grouped physically. The frozen evaluator remains outside the model's source-edit index.

## Source-role isolation and planning boundary (2026-11-13)

- `rsi/planner/domain.rs` now owns UCB/expression domain scoring, while `rsi_evolve::select_domain` stays at its historically required native source-mutation hook. The extracted module is used in both original coordinator selection and compiled descendant calls, not a parallel example implementation.
- The independent `rsi/proof/closure.rs` accepts a method step only if **all** candidate paths are source-editing/planning/learning/meta/execution/lineage methods. Frozen evaluation and proof modules and application capabilities never count as method edits. A capability step accepts only non-method paths.
- This eliminates a blind spot where valid recompiled improvements to `rsi/execution/` and `rsi/lineage/` could not qualify as an A step, and avoids accepting a mixed application patch as proof of method-only growth.
- Tests were authored in `tests/unit/engine_rsi_closure_source_tests.rs`; no tests, compilation, CI or live model runs were invoked here.

Research design context (not reproduced claims): 2026-10-11 *SpecForge* (arXiv:2610.15708) identifies cross-entity output coupling; 2026-09-22 *AIDE²* (arXiv:2609.26457) distinguishes accepted self-edits from hidden held-out gains; 2026-07-06 *MetaSkill-Evolve* (arXiv:2607.05297) distinguishes task skills from evolving improvement methods. The implementation stays frozen-weight and enforces source-category separation for proof.
