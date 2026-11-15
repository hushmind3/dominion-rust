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

All main-branch `web/` frontend source files are now preserved in sub4. The Vite production build is served by `tower-http::ServeDir` in `src/api.rs`; `/api/v1/*` stays under the original Rust approval/token middleware. The main React application shell, navigation, i18n and original Home dashboard are reused. The Home `NativeAgentPanel` and mounted `RustWorkspacePage` perform real requests for jobs, approval/recovery, groups, video, RSI and model config against the independently owned Rust SDK. When remote access is configured, the frontend supplies its access token through the `X-Dominion-Token` header; binary video fetching uses the same authenticated transport, not tokenized URLs. Dev proxy defaults to port 8787. Except for the native-backed original Home dashboard, this branch does not run the old main feature pages against unsupported legacy API endpoints; their implementation source remains available for capability-by-capability migration. The main branch's native desktop shell, state migration and advanced pages are not yet reconnected. Rust tests passed in Actions 43686024696; browser E2E and the legacy frontend migration remain outstanding.

The original main profile editor is available in the preserved workspace shell. Until the main profile storage format is migrated, its cosmetic owner display name and avatar are saved in browser localStorage (`dominion.sub4.owner-profile`); Rust run state, credentials and sessions are never stored through this UI preference.

## OS sandbox boundary (new)

`src/sandbox.rs` is the shared guard for arbitrary shell and RSI test/score commands. On Linux, the required default invokes trusted `/usr/bin/bwrap` or `/bin/bwrap` with a minimal executable/library view, no host home bind, a single writable workspace bind, isolated PID/IPC/network and bounded command runtime/output from the callers. On macOS it requires `/usr/bin/sandbox-exec` with read-only system paths, workspace write permission and no network; unsupported platforms fail closed. Explicit `DOMINION_SHELL_SANDBOX=off` is needed to disable enforcement, and approval is still required. This is not a seccomp/cgroup/credential-broker implementation; verified OS-level e2e tests are still required before claiming frontier sandbox parity.

## Delegated work wake and verification evidence

`Engine::new` creates a Tokio channel-driven parent wake scheduler that holds a Weak reference to the engine; the child completion callback settles durable budgets, releases unused escrow and queues the parent only after every child is fully finished and its tool side effects are settled. The parent worker can detach before relaunch, avoiding recursive async launches or time-window races. `src/verification.rs` extracts bounded real tool receipts. When a Git repository is attached, the verifier receives actual status and diff, and requires an exact `PASS` token to accept the answer. These improve grounding, but they are not independent execution of every user acceptance test.

## Focused acceptance: durable tool lifecycle

The production route is API or native SDK -> Engine::drive -> journal::begin ->
Store::mutate (fsynced intent) -> tool executor -> journal::finish ->
Store::mutate (receipt and intent removal atomically) -> API/SSE/SDK.
Denial uses journal::deny without executing or recording an intent.

Restart behavior:
- Queued and never started: resume persisted queue.
- Started pure local inspection: safely retry the queued read.
- Started mutating write, shell, MCP, delegation: UNKNOWN outcome and
  no automatic replay. User must observe external state and reconcile.
- Approved but not yet started: recover prior approval and resume.
- Undecided human approval: preserve it; never assume consent.
- Cancellation during a tool: terminate only after durable result or reconciliation.

Model tool-call IDs are checked against the durable transcript for duplicates;
invalid responses produce a corrective model turn without dispatch.
Store uses uniquely named create-new temporary snapshots, fsyncs data,
renames atomically, then syncs the directory on Unix.

Focused automated coverage: src/journal.rs transition and duplicate tests;
src/store.rs simulated before/after crash and write failure tests;
src/sdk.rs approved-but-unstarted restart with real file output;
src/engine.rs denial via real scheduler and no tool-start record.

This is NOT a claim of exactly-once external effects, complete crash recovery
of billed model calls, or parity with an entire frontier product.
The new durable tool and read-parallelism path compiled and passed Rust tests in
Actions 43686024696. Broad browser and real-provider benchmarks remain pending.

## Model tool-call orchestration (2026-11-15 implementation)

The production **Rust** code follows explicit source-level lessons from:
- Codex `codex-rs/core/src/tools/parallel.rs` (read/write exclusivity and dispatch barrier):
  https://github.com/openai/codex/blob/b016cd18472588606c04af565528c4cd6746a41f/codex-rs/core/src/tools/parallel.rs
- DeepSeek Harness `packages/core/tools/src/batch.ts` (bounded pool, model-ordered results):
  https://github.com/deepseek-ai/deepseek-harness/blob/39163dd30f703a79079e2d2446545cff3c453738/packages/core/tools/src/batch.ts
- DeepSeek Harness `packages/core/tools/src/json-schema.ts` and `pipeline.ts` (argument admission, fail-closed schema):
  https://github.com/deepseek-ai/deepseek-harness/blob/39163dd30f703a79079e2d2446545cff3c453738/packages/core/tools/src/json-schema.ts

Actual Dominion call path:
1. `provider.rs` submits `parallel_tool_calls=true` by default (operator can disable).
2. `engine.rs` freezes the exact advertised catalog on the persisted run each model step.
3. `tool_schema.rs` validates every requested tool and JSON argument before any side effect.
   A missing/invalid/unsupported schema gives an ordered TOOL ERROR result, not silent execution.
4. `tool_batch.rs` groups only contiguous safe workspace reads in a rolling pool
   (limit 1..8, default 4), preserving the original model call order.
5. `journal.rs` persists batch intent before dispatch and all ordered receipts together.
   On crash, unfinished reads are safe to retry. Writes, shell, remote MCP,
   subagents and memory mutations remain exclusive and unknown effects are never auto-replayed.
6. `src/api.rs`, the native Rust SDK, TypeScript HTTP adapter and React UI
   still use exactly the same `Engine` and run state; no imported foreign agent runtime.

Configured by `DOMINION_PARALLEL_TOOL_CALLS` and `DOMINION_MAX_PARALLEL_READS`.
Backend tests include a real mock-model multi-tool turn reading two files
concurrently, executing one exclusive write and verifying model-ordered receipts;
bounded concurrency, schema rejection, rollback and crash restart also have
regressions. Benchmark parity, broad MCP schema coverage and real-provider cost
effects remain unproven until separately measured.


## Source-grounded tool lifecycle, policies and cancellation (2026-11-15)

Official source mechanisms inspected:
- OpenAI Codex typed tool runtime and cancellation: https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/registry.rs
- DeepSeek Harness staged tool runtime and caller-owned abort signal: https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/tools/src/contracts.ts
- DeepSeek tool execution and cancellation docs: https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/core/tools

Dominion remains an independently controlled Rust harness with an explicit tool state machine:
1. Engine::drive freezes model-advertised schemas and validates calls in src/tool_schema.rs.
2. Ordered native pre-hooks in src/hooks.rs can veto tools before dispatch/approval; even approved-but-unstarted calls are rechecked against current hooks. Denials become durable tool receipts, not dispatches.
3. src/tool_batch.rs overlaps only declared pure reads in a bounded pool, honors write barriers and returns results in original model order. src/journal.rs stores intent before dispatch and final result before clearing intent.
4. Native post-hooks run before durable receipt storage, with a configurable model-output cap. Hook implementations cannot approve operations or bypass the journal; the native SDK exposes register_tool_hook.
5. Cancellation is persisted before signalling active model requests and pure reads with Tokio watch receivers. Parallel read cancellation aborts workers, waits for full task quiescence and only then commits ordered receipts. Unknown non-idempotent writes, external MCP calls and delegation remain receipt-safe and cannot be blindly replayed.
6. Environment controls: DOMINION_DENIED_TOOLS (exact tool names), DOMINION_TOOL_RESULT_MAX_CHARS, DOMINION_MAX_PARALLEL_READS and DOMINION_PARALLEL_TOOL_CALLS. Rust API, optional TypeScript SDK and the main React UI share the same runtime state.

Tests added for delayed model request cancellation, pre-hook veto preventing filesystem writes, approved-call re-veto, post-result caps, model-ordered concurrent reads, and durable crash recovery. Real-provider cancellation cannot reverse charges already incurred upstream.

Unfinished: full external-tool-specific cooperative cancellation and quiescence acknowledgements, persistence of dynamically registered native extension hooks across process restarts, and same-model/same-task frontier benchmarks.

Focused verification: GitHub Actions run https://github.com/hushmind3/dominion-rust/actions/runs/43687107564 compiled Rust and passed 69/69 tests, plus the optional TypeScript SDK suite. This included real local HTTP-provider cancellation and hook-veto integration. A subsequent read-worker quiescence regression was added and must be verified separately; broader frontier task-success parity remains unmeasured.


### Tool policy projection and verification

The operator's DOMINION_DENIED_TOOLS is enforced twice: filtered from both
per-run model-facing schemas in policy::select_catalog and the native Rust
SDK catalog, and blocked again by the ordered pre-hook just before dispatch.
This avoids wasting model tokens on unavailable tools without weakening
approval/schema checks. Per-run filtering preserves the frozen catalog for
crash-safe replay; dynamic SDK hooks can only add further denials.

Focused code and regression checks: src/hooks.rs, src/tool_batch.rs,
src/tool_schema.rs, src/journal.rs, src/policy.rs, src/engine.rs,
src/sdk.rs. A previous full source check completed with 70/70 Rust tests
and TypeScript SDK success (GitHub Actions 43687318332). The additional
SDK catalog-mask regression is included in this revision and must pass
the new source check before it is claimed verified. Broader model-task
success rates remain unmeasured.


## 2026 Streamable HTTP MCP: cancellation, progress and audit

Specification (2026-07-28):
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/cancellation.mdx
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/progress.mdx

The Rust backend implements request-scoped SSE and application/json in src/mcp.rs.
For SSE, it incrementally parses progress and stops reading after the matching
JSON-RPC response frame, instead of waiting for the server to close an idle stream.
On a user cancel it drops the response stream. MCP 2026 Streamable HTTP does
NOT send a notifications/cancelled POST; that signal is stdio-specific.

Client disconnect is NOT proof the remote mutation was undone. src/mcp.rs
returns an explicit Uncertain outcome on cancelled requests, lost transport,
incomplete/malformed replies or remote HTTP server faults. src/engine.rs stores
mcp_remote_outcome_unknown while keeping the in_flight intent, requires explicit
human POST /api/v1/runs/{id}/reconcile, and never automatically repeats the call.
The remote JSON-RPC request ID is deterministic from the persisted run and
tool-call IDs (dominion-<run UUID>-<model tool-call ID>), and is recorded in
tool_started and uncertain-result events for external service log correlation.

The request carries _meta.progressToken bound to its JSON-RPC request ID.
Only matching progress notifications are forwarded; max 16 buffered and
32 persisted events per call prevent unbounded work and storage. The adopted
main React UI distinguishes an active MCP call with progress from a paused
call requiring result reconciliation; native REST and SDK share these events.

Checks: real local Streamable HTTP fixtures exercise a live SSE response that
stays open after its final frame, user cancellation, mismatched progress tokens,
incomplete HTTP/SSE responses, 5xx ambiguity, and the entire Rust approval →
remote side effect → interruption → human reconciliation path. These do not
pretend to guarantee rollback of remote services or verified exactly-once
execution. External service-specific idempotency and task-resumption APIs
are possible next extensions, not assumed by the generic client.

### MCP focused validation — 2026-11-15

Commit b339afc358dde99dcaba1a8dd56ca94710e1ad27 passed all three independent
checks in https://github.com/hushmind3/dominion-rust/actions/runs/43689315045:
- Rust compile plus 79/79 unit/integration tests, 0 failures.
- TypeScript SDK suite, successful.
- Adopted main React frontend: npm ci, strict TypeScript typecheck and
  Vite production build, successful.

The MCP tests exercised a real local Streamable HTTP server, approved
side-effectful call cancellation, durable request-ID correlation, matched
progress notifications, transport ambiguity, and human reconciliation
without a second remote dispatch. The React check also removed the obsolete
Home dashboard's legacy input.prompt reference and distinguishes ongoing
MCP progress from paused reconciliation.

No real external MCP account, real-provider task success benchmarks,
interactive browser E2E, or end-to-end user data migration was performed
in this focused validation. These are remaining product-wide acceptance gates.

## Durable source edits and command evidence (2026-11-15)

The Rust native tool path `Engine::perform_tool -> journal::begin -> tools::execute -> journal::finish`
now stages `write_file` and `edit_file` in a new sibling temporary file, preserves an existing
file's permissions, syncs the new bytes, then atomically renames and syncs the containing
Unix directory. Failed staging leaves the prior destination intact and removes temporary
files; replacement of directories is refused. The model never sees a partially truncated
source file from Dominion's own write implementation. A successful rename followed by an
I/O error on directory sync can have an uncertain durable result and must be inspected
before reattempting; this does not imply exactly-once external effects.

Nonzero approved shell exits now return a tool error containing exit status, stdout and
stderr, instead of a `tool_result` success event. The autonomous plan/verify/replan loop
receives failure evidence through the ordinary tool journal; unsuccessful commands are not
silently classified as successful tests. Focused Rust tests cover atomic replacement,
permission preservation, invalid destination rejection, temporary-file cleanup and
command error classification. The separate OS sandbox policy still governs shell dispatch.
