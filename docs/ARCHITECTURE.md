# Sub4 architecture, recovery and migration map — 2026-11-15

This branch is a new small Rust-core rewrite, not a complete replacement for
the larger `main` React/Tauri/SQLite desktop product.

| Operation | API route | Rust implementation |
| --- | --- | --- |
| Autonomous planning / tools / verification | `/api/v1/runs` | `api.rs → engine.rs → provider.rs → tools.rs` |
| Durable history / restart | `/runs/{id}`, `/resume` | `store.rs`, `domain.rs` |
| Approval / crash tool reconciliation | `/runs/{id}/approval`, `/reconcile` | `engine.rs` with durable `in_flight` |
| Group turns | `/groups/{id}/next` | `group.rs → provider.rs` |
| Video storyboard and local rendering | `/videos/{id}/render` | `video.rs` with FFmpeg |
| RSI isolated evaluation and adoption | `/improvements/{id}/approve`, `/adopt`, `/descendant` | `improvement.rs` with detached Git worktrees |
| TypeScript SDK and GUI | `sdk/typescript/src/index.ts`, `web/app.js` | Both call identical Rust routes |

## Write-ahead tool state

1. Rust records planned tool calls and explicit approvals.
2. The engine atomically persists `in_flight` and a `tool_started` event
   before running any external operation.
3. On completion it stores the tool receipt, removes the queued call,
   and clears `in_flight` in one run-store mutation.
4. A crash in between leaves a paused run with uncertain tool side effects.
   A resume request is refused; the user inspects outcomes and sends a
   reconciliation receipt. Reconciliation itself never executes the tool.
5. Provider errors are paused and retryable. Excess model-token charges
   are persisted before terminal failure.

This protects against silent *replay* after a process crash; it is not an
exactly-once execution guarantee, distributed transaction protocol, or
operating-system sandbox. Shell commands need separate OS/container isolation.

## Comparison and gaps

Main has broader provider/model support, MCP/skills, channels, advanced
memory, React/Tauri UX, generative video backends, and a multi-file RSI
research system. Those features are not yet migrated to sub4's new schema.
The current `video.rs` renders local title-card MP4 clips and optional
espeak narration; high-fidelity generated scenes remain future work.
Legacy SQLite user data is not automatically imported into sub4 JSON stores.

2026 reference: Barbaste et al., *Harness Engineering* (July 2026,
arXiv:2609.00006), and OpenHarness's October 11 agent-client design.
The architectural lesson is to keep the tool loop and durable session
control within Dominion's own Rust runtime, independent of provider.
