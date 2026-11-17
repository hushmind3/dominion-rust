# Sub4 lean-code and runtime hot-path cleanup — 2026-11-17

Scope: `dominion-sub4` only. Main and competing branches unchanged.

## Split long active files by responsibility

- `src/frontend_api.rs` was 1,046 lines mixing 8 independent API areas. Now an ~98-line router plus `frontend_api/{legacy,system,agents,sessions,chat,operations,memory,sdk_endpoints}.rs`, with each handler module at most ~231 lines.
- `web/src/pages/Extensions.tsx` was 1,398 lines. Now a 10-line compatibility entry, `extensions/ExtensionsController.tsx` (625 lines) and `ExtensionsView.tsx` (~837 lines). All four tabs and their existing actions remain.
- Rust regression tests were extracted from `store.rs`, `mcp.rs`, `journal.rs`, and `engine/tests.rs` into dedicated child modules, keeping production code separate from tests.
- The CI source-size check rejects Rust, TypeScript and TSX source modules over **900 physical lines**, except the two translation dictionaries under `web/src/i18n/`. Code size is a maintainability budget, not a benchmark score.

## Lower the hot-path cost, retain durability

1. UI state mutations now hold a separate writer mutex for serialization order and release the global snapshot mutex before JSON serialization and fsync. No new state is published until file+directory sync completes. Serialize up to 8 MiB in `spawn_blocking`, avoiding blocking Tokio's asynchronous executor.
2. `Store::stats` and `FrontendStore::counts` supply the `/api/status` 12-second heartbeat without cloning every historical Run, transcript, agent, or session.
3. `Store::headers` projects the lightweight job and execution listing fields while skipping large model tool messages, tool schemas, and journal receipt cloning. `Store::pending_approvals` and `FrontendStore::pending_run_sessions` only materialize active approval relationships.
4. Extension tabs no longer trigger initial MCP discovery on every unrelated tab and gate initial catalog discovery to relevant tabs.

## Verification and limits

- New Rust tests assert the UI remains readable while a durable writer is queued, rejected changes publish no partial state, and lightweight Run headers exclude 200k-char model receipts.
- The existing native crash-recovery and browser E2E remain required; no full Run journal, fsync or tool receipt retention was dropped.
- The front-end snapshot is still a complete JSON rewrite up to 8 MiB and individual Run writes remain full durable snapshots. The changes target **blocking, cloning, and unnecessary remote discovery**, not an unimplemented incremental storage protocol.
- Actual CPU, disk bytes, allocations, and latency improvements require before/after measurement. CI passing alone is not a performance percentage.
