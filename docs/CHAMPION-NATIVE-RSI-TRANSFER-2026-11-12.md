# Champion verified native RSI transfer — 2026-11-12

## Provenance
The executably tested native RSI framework is merged from Sub3 commit `ae927f6b4153926f892998f6cd312f3a4e94f7db`, the exact harness commit of independent GitHub Actions run [43127004269](https://github.com/hushmind3/dominion-rust/actions/runs/43127004269). That run independently accepted **three** consecutive native source revisions, with improver optimization and a memory-domain descendant, `capabilityDemonstrated=true` and `accelerationProven=false`. Its model tool call count was zero. The transfer is not by itself a new Champion performance result.

This Champion branch uses the proven native engine source tree as its executable starting point. The parent Champion commit `dab6a549de4cde8fefd27375a2571902ed3f3570` remains in Git history, and a separate snapshot branch `dominion-champion-before-native-transfer-20261112` preserves all prior active Champion code.

Three Champion-only source files (`rsi_action_program.rs`, `rsi_judgement.rs`, `rsi_success_ablation.rs`) are retained verbatim for later tested reconciliation. They are not registered in `src/engine/mod.rs` for this baseline, so they cannot silently change the proven native execution semantics. Prior Champion integration features must not be represented as active until re-integrated and measured.

## Verification path
- `.github/workflows/champion-native-rsi-proof.yml` starts from the exact checked-out Champion commit with a clean worktree, **without restoring a historical bug**.
- `scripts/champion-native-rsi-proof.mjs` is the Sub3 proven campaign script re-targeted to the Champion source/branch. It requests `nativeInduction`, `expandAfterGain`, `stopAfterProof`, and up to 6 attempts under fixed budgets.
- A successful GitHub Actions job alone is not proof. Check the generated `champion-real-model-attempt` artifact for `acceptedGenerations >= 3`, `maxAcceptedStreak >= 3`, `capabilityDemonstrated=true`, `actualCompletion=true`, independently paired receipts, different descendant commits/binary hashes, and 0 regressions.
- Do not credit the historic Sub3 proof as a new Champion proof. Preserve the explicit difference between engine capabilities and actual measured outcomes.

## Operating constraints
No production apply/merge or live service restart is automatic. Native source mutations are only credited after isolated rebuild and unchanged frozen paired evaluation. Non-winning and neutral variants cannot be reported as recursive capability gains.
