# Champion RSI — current code map

**Branch:** `dominion-champion` · **Updated:** 2026-11-13.
This file is a short routing index, not proof of performance. Source code is the canonical description.

## Follow the actual runtime

1. `src/engine/rsi_evolve.rs` — **small compatibility facade**, preserved external entry points. Full implementation follows explicit submodules:
   - `src/engine/rsi/planner/` — strategy search, source-grounded context and compiled child plan
   - `src/engine/rsi/execution/` — campaign loop, generation worker, source/worktree and budget guards
   - `src/engine/rsi/evaluation/` — original-paired adoption and success criteria
   - `src/engine/rsi/learning/` — verified edit methods, transfer trials and research reuse
   - `src/engine/rsi_evolve_tests.rs` — separated Rust unit tests
2. `src/engine/rsi_system.rs` — source-grounded architecture, selected-target validation and independently replayed history. Tests: `rsi_system_tests.rs`.
3. `src/engine/rsi_meta_controller.rs` — editable source-target ranking and prediction-repair priority.
4. `src/engine/rsi_prequential.rs` — source-bound prequential predictions. Invalid/missing original paired receipts are not learning labels; only actual changed target source can be a target success.
5. `src/engine/rsi_peer_transfer.rs` — one donor method hypothesis, matched source/frozen-profile control, negative transfer suppression; never auto-apply a peer patch.
6. `src/engine/rsi_archive.rs` — persistent stepping-stone selection, evidence identity and completed neutral candidate persistence.
7. `src/engine/rsi_method_generalization.rs`, `rsi_improvement_algebra.rs`, `rsi_recursive_scientist.rs`, `rsi_model_lineage.rs` — descendant-editable improvement algorithms.
8. `src/engine/rsi.rs` and `src/engine/rsi_worktree.rs` — independent paired evaluation and detached source worktree. Frozen evaluator controls adoption.

## Evidence boundaries

- `docs/rsi-completion.md` and `benchmark/champion-native-rsi-completion-2026-11-12.json` document **historical** native proof, not latest Champion model-driven results.
- The later integration of the durable archive, prequential self-repair and source-matched peer transfer has **no fresh passed CI or live-model multi-generation proof** recorded here.
- `docs/RSI-MANUAL-RUNBOOK-2026-11-09.md` describes setup/operations; `docs/research/` contains historical research/evaluation context.
- Git history retains earlier one-off implementation notes and 2026-11-05/11-08 handoffs removed from the current file tree.

## Readability rule

Production Rust modules contain runtime logic; their large inline test modules were moved to sibling `*_tests.rs` files without changing test cases. Don't reintroduce historical handoffs or generated evidence dumps into source-reading context. Preserve frozen oracle receipts and actual accepted descendants.

## Pruned source files

Three historical unregistered implementations (`rsi_action_program.rs`, `rsi_judgement.rs`, `rsi_success_ablation.rs`) were not part of the engine module registry or executable dependency chain, so they were removed from the active tree. Git history retains them. The dynamic frozen oracle (`rsi_oracle_fixture.rs`) and native mutation template (`rsi_domain_accumulator.rs`) are **not** dead: the actual runtime loads their bytes with `include_str!`. The facade now `include!`s that native template instead of storing a second copy.
