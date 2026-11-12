# Champion — compiler gate repair and whole-intelligence feedback (2026-11-13)

## Latest judge result

The current judge reported **Champion 66/100, rank 5**, against sub3 79, sub4 76, sub1 73, sub2 71. The decisive defect was a Rust move-after-use in `src/engine/rsi_improvement_algebra.rs`: consuming `candidates` with `into_iter()` then referencing `candidates.len()` in the generated plan. The immediate repair stores `total_candidates` **before the consuming iteration**, then emits that count. A regression assertion now checks `candidateCount` and the actual ranked candidates in the multi-role rule-composition unit test.

## Structural correction beyond syntax

A separate issue could undermine complete RSI even after compilation: **as soon as any model-authored meta improvement operator existed, its mere presence took precedence over independent failures of model, memory, learning and tool subsystems**. That is a controller-selection bias, not open full-intelligence learning.

`rsi_method_generalization::prefer_capability_target` adds evidence-based competition between:

- an authored, executable source-bound improvement operator with observed transfer performance;
- a currently observed, receipt-replayed frozen holdout regression in the source-backed model/memory/learning/tool/other capability frontier.

The learner does not blindly replace every operator with the latest task failure. When an operator already targets the deficient role, or independently measured cross-capability benefits are substantial, the operator remains eligible. When an operator has no independent downstream record, a real capability failure can take priority. The planner and separately revalidating worker apply the **same selector**. The selected role and actual source path feed the next descendant code edit.

A new unit test covers both directions (unmeasured meta rule losing to an observed model regression; a productive source-written transfer rule retaining precedence).

This fixes a selection failure in which additional meta-agent features could paradoxically **reduce** the chance of further task-intelligence improvement.

## Comparison with other branches and research

Sub3's evolving program graph routes independent observed outcomes into future source plans. Sub4's local LoRA trainer creates model-weight candidates, but independent adapter evaluation, promotion and live descendant inference still require explicit evidence. Champion's chosen work here focuses on the **executed and evaluated child improvement feedback loop** rather than falsely claiming weight training from router/model source edits.

AIDE² ([paper](https://arxiv.org/abs/2609.26457)) emphasizes evaluating downstream task performance for newly rewritten research agents. Hugging Face [PEFT documentation](https://huggingface.co/docs/transformers/peft) distinguishes training adapter weights from actually loading and selecting the resulting adapter. These mechanisms should not be conflated in judge claims.

## Validation and limits

- Both code paths and unit tests were committed to `dominion-champion` with `[skip ci]`.
- The move-after-use source error is explicitly repaired.
- There is **no assertion that the newest Rust source compiles**: local Rust toolchain was unavailable, and the judge's 66-point score is below the project's 80-point CI qualification threshold.
- No third-party model-weight training or independently measured multi-generation closed-loop improvement is claimed.
