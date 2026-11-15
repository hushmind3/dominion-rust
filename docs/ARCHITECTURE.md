# Sub4 architecture, recovery and migration map — 2026-11-15

This branch has an independent rebuilt Rust core and reuses the `main`
React/Vite frontend. The older `main` Tauri/SQLite engine and its data formats
are not fully migrated.

| Operation | API route | Rust implementation |
| --- | --- | --- |
| Autonomous planning / tools / verification | `/api/v1/runs` | `api.rs → engine.rs → provider.rs → tools.rs` |
| Durable history / restart | `/runs/{id}`, `/resume` | `store.rs`, `domain.rs` |
| Approval / crash tool reconciliation | `/runs/{id}/approval`, `/reconcile` | `engine.rs` with durable `in_flight` |
| Group turns | `/groups/{id}/next` | `group.rs → provider.rs` |
| Video storyboard and local rendering | `/videos/{id}/render` | `video.rs` with FFmpeg |
| RSI isolated evaluation and adoption | `/improvements/{id}/approve`, `/adopt`, `/descendant` | `improvement.rs` with detached Git worktrees |
| TypeScript SDK and GUI | `sdk/typescript/src/index.ts`, `web/src/App.tsx`, `web/src/pages/RustWorkspace.tsx` | Main React/Vite UI and SDK use the native Rust v1 routes |

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
memory, native Tauri integration, generative video backends, and a multi-file RSI
research system. The React UI source and original Home dashboard are preserved. Those features are not yet migrated to sub4's new schema.
The current `video.rs` renders local title-card MP4 clips and optional
espeak narration; high-fidelity generated scenes remain future work.
Legacy SQLite user data is not automatically imported into sub4 JSON stores.

2026 references are source-linked and compared in FRONTIER_2026.md.
The key separation is to keep session state, tool execution and runtime
authority within Dominion's independent Rust engine.

## Optional motion generation backend

`src/veo.rs` implements the current Gemini OpenAI-compatible asynchronous video endpoints. The video service records job IDs, polls completion, bounds the size of MP4 downloads, restricts download destinations, normalizes scene media, and keeps the FFmpeg local path as default. A job can retry from its recorded remote operation. Provider API charge estimation is configured separately from text-token rates. Local video and external video have different rendering fidelity, cost and failure modes.

## New primary entrypoint and coding workflow

`src/lib.rs` exports `Dominion` from `src/sdk.rs`. The application
`src/main.rs` constructs this native SDK and serves its `http_router()`.
Both REST and TypeScript call the same Rust engine and persisted records.

`/api/v1/runs/from-repository` resolves a Git checkout inside the configured
workspace. `src/workspace.rs` attaches a separate detached worktree at the
recorded revision before the agent executes. `src/tools.rs` provides exact
atomic file editing and read-only Git status/diff alongside the existing
file/search and user-approved shell tools.

`src/engine.rs` now checks for user directives added during model planning,
execution and verification. A stale answer cannot be adopted after those
interventions. Context projection via `src/context.rs` retains the complete
durable history and keeps tool call/receipt groups adjacent for the model.

Subagents are Rust workers with separate run IDs, budgets, saved histories and
workspaces. Sibling allocations reserve part of the parent's remaining budget;
an attached repository is inherited at the same base Git revision. Final
responses are visible to their parent through `check_subagent` and files via
`read_subagent_file`. No automatic merge/adoption is claimed.

Current native provider metering separates uncached and provider-reported
cached input tokens when configured with accurate KRW rates.

See `docs/FRONTIER_2026.md` for concrete frontier reference gaps and
`docs/RUST_SDK.md` for the supported host entrypoint.

## Main React frontend adopted (2026-11-15)

All main-branch `web/` frontend source files are now preserved in sub4. The Vite production build is served by `tower-http::ServeDir` in `src/api.rs`; `/api/v1/*` stays under the original Rust approval/token middleware. The main React application shell, navigation, i18n and original Home dashboard are reused. The Home `NativeAgentPanel` and mounted `RustWorkspacePage` perform real requests for jobs, approval/recovery, groups, video, RSI and model config against the independently owned Rust SDK. When remote access is configured, the frontend supplies its access token through the `X-Dominion-Token` header; binary video fetching uses the same authenticated transport, not tokenized URLs. Dev proxy defaults to port 8787. Except for the native-backed original Home dashboard, this branch does not run the old main feature pages against unsupported legacy API endpoints; their implementation source remains available for capability-by-capability migration. The main branch's native desktop shell, state migration and advanced pages are not yet reconnected. Build/test against actual Rust and browser remain pending the source-residue review and CI gate.

The original main profile editor is available in the preserved workspace shell. Until the main profile storage format is migrated, its cosmetic owner display name and avatar are saved in browser localStorage (`dominion.sub4.owner-profile`); Rust run state, credentials and sessions are never stored through this UI preference.

## OS sandbox boundary (new)

`src/sandbox.rs` is the shared guard for arbitrary shell and RSI test/score commands. On Linux, the required default invokes trusted `/usr/bin/bwrap` or `/bin/bwrap` with a minimal executable/library view, no host home bind, a single writable workspace bind, isolated PID/IPC/network and bounded command runtime/output from the callers. On macOS it requires `/usr/bin/sandbox-exec` with read-only system paths, workspace write permission and no network; unsupported platforms fail closed. Explicit `DOMINION_SHELL_SANDBOX=off` is needed to disable enforcement, and approval is still required. This is not a seccomp/cgroup/credential-broker implementation; verified OS-level e2e tests are still required before claiming frontier sandbox parity.

## Delegated work wake and verification evidence

`Engine::new` creates a Tokio channel-driven parent wake scheduler that holds a Weak reference to the engine; the child completion callback settles durable budgets, releases unused escrow and queues the parent only after every child is fully finished and its tool side effects are settled. The parent worker can detach before relaunch, avoiding recursive async launches or time-window races. `src/verification.rs` extracts bounded real tool receipts. When a Git repository is attached, the verifier receives actual status and diff, and requires an exact `PASS` token to accept the answer. These improve grounding, but they are not independent execution of every user acceptance test.
