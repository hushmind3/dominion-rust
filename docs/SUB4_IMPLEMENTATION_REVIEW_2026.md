# Sub4 — source-level implementation review (2026-11-17)

**Target:** `dominion-sub4` only. This note distinguishes source inspection from benchmarked results. It does not grade any branch as production-ready.

## What was inspected

| Implementation | Actual source | Relevant mechanism | Sub4 consequence |
| --- | --- | --- | --- |
| Qwen Code, 2026 session agents | [`packages/core/src/agents/session-agents/chain.ts`](https://github.com/QwenLM/qwen-code/blob/eb3a599ef3349bd430dca73a136d7bc3e3e71586/packages/core/src/agents/session-agents/chain.ts), [design 2026-10-05](https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-05-session-multi-agent.md) | Explicit agent-to-agent chain depth, mention self-loop exclusion, optional per-session token budget and coalesced triggers | Sub4 already persists child budget reservations and limits direct children to 4, but does **not** implement Qwen's session-mention routing or its token-budget semantics. Do not claim feature parity. |
| OpenAI Agents SDK (JS) | [`packages/agents-core/src/runner/toolUseTracker.ts`](https://github.com/openai/openai-agents-js/blob/8aeac0eb255dbdff9adcdc65a0eae638cbcea74f/packages/agents-core/src/runner/toolUseTracker.ts), [tracing](https://openai.github.io/openai-agents-js/guides/tracing/) | Per-agent tool history with explicit retention rules and nested trace spans | Sub4 has durable tool receipts and compact `RunUpdate` events. It has no equivalent complete span-based tracing view; tool receipts alone do not establish trace-feature parity. |
| OpenHands Software Agent SDK | [`openhands-sdk/openhands/sdk/workspace/base.py`](https://github.com/OpenHands/software-agent-sdk/blob/16decb4927eb39ec8d239b250e702a840657a0e0/openhands-sdk/openhands/sdk/workspace/base.py) | Workspace interface separates command/file/Git actions and lifecycle; records accumulated run cost for completion callbacks | Sub4 already separates native workspace tools, MCP and Rust Store; unknown provider prices must not be counted as measured zero-cost runs. |
| Dominion main / sub2 / sub3 | `src/engine/mod.rs`, `src/engine/jobs.rs` at each branch's 2026-11-17 HEAD | Alternative module boundaries and job-dispatch structure | Sub4 is a separate Rust-native kernel. Copying entire adapters would create parallel engines; compare individual invariants and real execution paths instead. |

## Implemented in this pass

1. **Startup write reduction:** `Store::open` records a semantic before/after diff for recovery and migration, fsyncing only changed runs. Unmodified historical JSON is not rewritten merely because the process started.
2. **Scoped descendant reads and startup selection:** `Store::has_children`, `children_of`, `family_of` avoid cloning every historical transcript during plan/catalog selection, direct-child admission, and descendant settlement. `family_of` captures two levels under one store lock to avoid mixed-generation family snapshots. `Store::recovery_candidates` likewise returns small ID lists for SDK startup instead of full Run snapshots.
3. **Execution responsibility split:** `src/engine.rs` owns the scheduling, model/tool drive and status transitions. `src/engine/delegation.rs` owns child spawning, patch integration, authorization and durable descendant budget settlement. `src/engine/tests.rs` keeps the same native regression tests in a separate test-only module.
4. **Added regression coverage:** unchanged completed-run disk content must survive a restart untouched, and direct/grandchild family queries must exclude unrelated runs. Existing crash, approval, cancellation, and child-budget tests remain in the crate.

## Performance model, not measured performance

- Let **N** = persisted historical runs and **M** = runs modified by recovery/migration. Previously startup called `Store::put` for **N** runs after loading them; now it calls `persist` for **M**. If **M = 0**, this change performs **0** per-run replacement/fsync cycles at startup. Initial reading and JSON parsing of all **N** records is unchanged.
- Let **K** = relevant child/grandchild runs. Prior model-step and settlement paths cloned approximately **N** complete `Run` records for each `Store::list` call. Scoped scans remain O(N) lookups, but clone at most **K** records; `has_children` clones none.
- These are implementation and asymptotic observations, **not measured elapsed time, memory, IOPS, or model token savings**. A repeatable before/after run is required for numeric claims.

## Existing safeguards preserved

- An in-flight mutating/MCP operation with uncertain outcome cannot be replayed automatically.
- User approval, cancellation, paused recovery, and child budget settlement still use the same durable `Run` state.
- No second scheduler, auto-editing RSI, or embedded video execution subsystem is introduced.
- Existing JSON store formats and `/api/v1` / TypeScript SDK contracts are unchanged.

## Validation and next gap

- Review changed files against `cargo test --all-targets`, especially `startup_does_not_rewrite_unchanged_historical_runs`, `child_lookup_does_not_include_unrelated_runs`, `child_budget_settlement_is_durable_and_idempotent`, and `uncertain_tool_is_not_replayed_without_reconciliation`.
- Run existing 256-Run crash-recovery and 30-second recovery-soak suites only when requested/available, and capture timing and IO deltas before claiming any speedup.
- Check release builds and independent Windows/macOS/Linux recovery semantics. Explicit external MCP reconciliation requires a remote provider fixture before production claims.
- Long-term architectural work: shrink the remaining model/tool kernel by responsibility **after** its actual state invariants and tests are understood; avoid arbitrary file-size caps or adding a new orchestration loop.
