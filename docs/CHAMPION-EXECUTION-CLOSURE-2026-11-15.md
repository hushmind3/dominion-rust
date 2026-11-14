# CHAMPION — Verified Execution/Recovery Closure (2026-11-15)

Branch: `dominion-champion`. This note traces **real source and runtime connections**, not a claim of a passing frontier benchmark, build, CI or successful live paid inference.

## Why this change

The 2026 agent-harness literature identifies failures at the interface between model decisions, durable state, actual effects and verification. In [HarnessFix (2026-06)](https://arxiv.org/abs/2606.06324), trace-to-harness failure attribution is used to select narrow repairs; [LongHorizon-Harness (2026-08)](https://arxiv.org/abs/2608.01964) separates manager state from execution observations; [Rethinking Verification in Agent Harnesses (2026-10)](https://arxiv.org/abs/2610.34577) evaluates initial reminders, completion gates and independent reviewers; [Harness Design for Coding Agents (2026-09)](https://arxiv.org/abs/2609.20804) demonstrates that context management, planning and tool scope must be evaluated with matched work budgets.

DOMINION retains its **own Rust executor and owner-configured policies**; these works motivate modifications to existing native execution paths, not dependencies on external harness engines.

## Actual code flow

**Owner's current instruction, no replay**:
`POST /api/sdk/threads/:id/steer` → `agent_sdk::DominionSdk::steer` → SQLite `chat_steer` → `chat/runtime/turn.rs` (stop all unstarted obsolete tool calls) → `chat/context.rs::history` carries original SQLite identity as **`_dominion_frame`**. On restart, deduplication now compares that actual field rather than the nonexistent public `id`. `context.rs` includes an explicit write-then-crash-then-drain test.

**Goal creation and deterministic test gate**:
`POST /api/goals` records `requireTests` and `tokenBudget`, rejecting wrong types/out-of-range budgets. `sdk/typescript` and `sdk/rust` expose both settings. In `goals::run`, a strict coding goal must pass `verification::goal_test_gaps` before an independent reviewer can mark completion. That gate uses `db::session_execution_operations`, including **failed** tool receipts, across resumed turns; it rejects a prior passing test when a later edit or a later failed validation invalidates it. It does not parse model claims or plain `echo` strings as test results. The error-preserving query fails closed after 1,000 records rather than silently dropping evidence. The Rust Axum/mock-provider goal integration test verifies a truthful file write plus a dishonest reviewer cannot complete a strict goal without a real test.

**Evidence-driven progress and repair**:
`goals::run` snapshots previously **SHA-observed** `goal_artifacts`. `goals/progress.rs::observed_artifact_changes` compares actual saved file-content digests; in a strict coding goal, new tool-call arguments by themselves never reset the stall counter. `goals/diagnosis.rs` classifies deterministic gate gaps and observed tool errors by layer, preserving original reasons and marking root-cause attribution explicitly unproven. The next executor turn receives the current compact diagnosis; the replanner receives observed tool evidence, reviewer gaps and bounded attribution hints. The existing native planning, iterative repair, subagents, worktree isolation, approval and goal checkpoints continue to execute as before.

**Session-wide token ceiling**:
`api/goals.rs` persists `tokenBudget` and validates 1,000–2,000,000. `goal_control.rs::remaining_goal_tokens` reads `Store::session_usage` for the actual goal-linked session. Both `GoalControl::complete` (planner/reviewer) and `chat/runtime/turn.rs` (every executor model/program step) check the remaining observed tokens before starting the next step. The benchmark runner passes `--token-budget` per coding instance and records observed overrun. The guard is **step-boundary**: no API can undo already generated/billed tokens, and final in-flight usage may exceed the specified limit. Never present it as an invoice or a guaranteed hard provider cap.

**Real public benchmark**:
`scripts/benchmark/swebench_dominion.py` selects official SWE-bench Verified instances, checks out the dataset's source SHA, executes native Goal in isolated worktrees with `requireTests=true` and the specified token ceiling, exports tracked **and newly created** source files through `goal_git::complete_export_patch`, validates patch bytes and SHA, and feeds predictions to the upstream Docker grader. `official_score` requires per-instance `resolved_ids` for exactly the submitted instance set. A successful run of the script without an official judge report **never** becomes a made-up resolved score.

## Reproducible verification commands

```bash
cargo test --release
cargo test --manifest-path sdk/rust/Cargo.toml --all-targets
npm install --prefix sdk/typescript --no-audit --no-fund
npm test --prefix sdk/typescript
python3 -m unittest discover -s scripts/benchmark -p 'test_*.py'
node --test scripts/champion-system-contract.test.mjs
```

The registered CI job includes these checks. These command lines are the **intended verification procedure**; GitHub source commit acknowledgement does not mean tests have passed. No Rust compiler was available in the editing environment, and no live model / official SWE-bench / high-end video quality score is claimed for this update.

## Open operational proof

- Confirm a real `cargo test --release` and Rust/TypeScript SDK test pass from an independently observed CI job.
- Use identical SWE-bench Verified instances, prompt budgets and source commits for any competitor baseline and capture the official grader output.
- Execute forced-crash tests at the tool-effect persistence and reviewer checkpoint boundaries on real files and shell operations.
- Verify long-horizon task success, recovery rate, token/billing spend, parallel delegated tasks and subjective video continuity through observed outcomes, not README claims.
