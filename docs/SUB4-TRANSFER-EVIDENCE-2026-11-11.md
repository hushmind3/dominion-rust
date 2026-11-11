# Sub4 — executable source-domain transfer, 2026-11-11

## Evaluation standard
Judge baseline on 2026-11-11: Sub4 35/100, fourth of five. A passing build is not an independently demonstrated model-generated improvement; a manually simulated accepted result is not real adoption. Keep this distinction when scoring.

## Changes on dominion-sub4
- `9aef9d3139`: add `$rsiFocusPath` symbolic procedure binding, bind at generation setup to the currently indexed `rsiFocusFiles` rather than the original generation's hardcoded file. Apply the same focus to executable node arguments and file postconditions; retain strategy ID/revision and evidence.
- `379a545270`: add end-to-end tool-level, two-domain Rust test.
- `19b2231ff5`: document `$rsiFocusPath` for model-generated `rsi_method_propose` proposals.
- `f4df05b6a7`: strengthen the test to **actually edit** two separate workspace source files via the production `workspace_replace` tool and verify the resulting contents and recorded execution/procedure feedback.
- `d68aad89e5`: fix a test assertion: the source domain's transfer flag is absent/null, whereas only a cross-domain trial sets true.

## Verified, reproducible evidence
CI: https://github.com/hushmind3/dominion-rust/actions/runs/42945602109

- 2026-11-11: success on `d68aad89e5`.
- Rust tests: 16 + 644 + 3 + 99 = **762 passed, 0 failed**.
- Frontend TypeScript typecheck and build: passed.
- `learned_procedure_repairs_two_distinct_source_domains_with_real_tool_receipts` uses separate `src/engine/memory.rs` and `src/engine/tools.rs` sandbox files, starts each with `false`, executes the identical learned procedure with its runtime-bound target, changes each to `true` through the actual tool dispatcher, and checks `procedure::evaluate` plus durable receipts.
- The test's successful promotion/adoption flags are **synthetic test inputs**. Therefore the recorded `transferVerifiedDomains` in this test is evidence of correct feedback plumbing, NOT evidence that a model actually discovered, independently validated, and transferred a performance-improving method.

## Known blocking gaps and next priority
1. Run a **real-model** campaign on two different, meaningful problems (memory and tools) in isolated worktrees. Have a real model propose a new method in the source domain, record that method, and apply it in the target domain. Avoid supplying a pre-authored method.
2. Pair baseline and candidate on **independent holdouts** in each domain, using the same model/protocol/task suite. Record task success and any regressions, costs, evaluation provenance, source commits, method ID, actual tool operations and run IDs.
3. Only mark independently earned adoption/transfer if the model-generated patch improves held-out task success and that improvement persists after the next generation's rebuild. If no true improvement, keep score in 0–39.
4. Run at least three successive accepted improvements with demonstrable gains if aiming beyond the first loop. No score increase from this CI test alone.

No changes were made to Champion, Sub1, Sub2, Sub3, or main.
