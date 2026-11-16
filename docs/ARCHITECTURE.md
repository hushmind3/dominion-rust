# Sub4 architecture, recovery and migration map — 2026-11-15

This branch has an independent rebuilt Rust core with its own `/api/v1` HTTP and native SDK. **Its `web/` tree is byte-identical to the latest main frontend at commit `879dc23d`**, without the previously introduced Rust-specific replacement page. This restores main's actual route structure and UX source, not integration with the old main runtime. Main's `/api/*` backend contracts are not yet satisfied by sub4; do not count visual source parity as working product feature parity.

See [frontend exact-tree verification](FRONTEND_ALIGNMENT.md), [UI backend contract gaps](UI_FEATURE_PARITY_2026.md), and [native SDK](RUST_SDK.md).

| Operation | Native Rust API | Implementation |
| --- | --- | --- |
| Autonomous planning / tools / verification | `/api/v1/runs` | `api.rs → engine.rs → provider.rs → tools.rs` |
| Durable run history / restart | `/api/v1/runs/{id}`, `/resume` | `store.rs`, `domain.rs` |
| Approval and crash reconciliation | `/api/v1/runs/{id}/approval`, `/reconcile` | `engine.rs`, `journal.rs` |
| Group turns | `/api/v1/groups/{id}/next` | `group.rs` |
| Video storyboard and rendering | `/api/v1/videos/{id}/render` | `video.rs` |
| RSI isolated evaluation / adoption | `/api/v1/improvements/{id}/approve`, `/adopt`, `/descendant` | `improvement.rs` |
| Main frontend | Original `/api/*` endpoints | **Not ported to native Rust contracts; the identical main UI source alone does not make these routes work** |

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

## Main frontend source synced exactly (2026-11-15)

The `web/` tree is now **exactly equal** to the latest main `web/` tree (`9e2be9b7376c1eefb6195e3ea7f9463b952456c0`). All main pages, routes, components, scripts, styles and dependencies are restored. The sub4-specific `RustWorkspace` frontend and other `web/`-only additions are absent. The native Rust backend is preserved without claiming that its existing `/api/v1` endpoints implement the main frontend's legacy `/api` feature contract. Matching main UX and restoring working features are separate work streams; backend capabilities must be genuinely implemented rather than simulated in the UI. See [FRONTEND_ALIGNMENT.md](FRONTEND_ALIGNMENT.md).

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

## MCP 2026 tools/list freshness and pagination

`src/mcp.rs` fetches every advertised `tools/list` page using `nextCursor` (16-page and
80-tool fail-closed bounds) before exposing the final model catalog. Each page's `ttlMs`
and `cacheScope` controls client-side freshness; the shortest page TTL wins, zero or
missing/invalid hints prevent caching, and positive TTLs are capped at 5 minutes.
Cached private results remain inside one operator-configured server/token client.
Expiry is checked on demand, not polled; real SSE `subscriptions/listen` invalidation
is still an unimplemented extension. This follows MCP 2026-07-28 and SEP-2549.

## RSI durable experiment state (2026-11-15)

The `src/improvement.rs` experiment state now uses a unique create-new snapshot,
fsynced JSON bytes, atomic rename and Unix directory sync, like the Rust run store.
Visible statuses are committed before replacing the in-memory record. On restart,
previously `Running` trials are explicitly marked `Failed`, with an unknown outcome
notice, and that recovery marker is persisted before API/SDK observation. No incomplete
candidate may be silently adopted or automatically retried. This protects experiment
metadata; it does not replay interrupted provider calls, clean abandoned Git worktrees,
or implement a distributed transaction across Git cherry-pick and record adoption.


## Real multi-file RSI and frozen judge path (2026-11-15)

The native Improvements service (shared by Rust SDK, REST, TypeScript and Studio)
now supports 1-8 source files; old single-file trials and plaintext responses
remain compatible. Every file path is validated and named test fixtures,
test files and workflows are not available to candidate modification.

The baseline is measured once. Up to three bounded model proposals are applied
to a disposable candidate Git worktree. Each attempt is committed, then copied
into a third freshly reset Git judge worktree at the original base SHA. The
judge executes the original operator-supplied test and scoring commands without
exposing the frozen judge to the model. Failed test results and scores are fed
back into the next proposal; actual model costs and attempts persist. An
accepted detached commit is pinned at refs/dominion/rsi/<trial-id> until explicit
Git adoption, so worktree cleanup and garbage collection cannot lose it.
Descendants inherit the target file set and measured reflection.

The full real Git/model/command/evaluation/adoption regression runs as an
explicitly selected Rust integration test. Its own isolated Git fixture,
fixed local commands and loopback model use DOMINION_SHELL_SANDBOX=off only
for that CI test process. Production RSI continues to require a sandbox backend,
human approval and enabled shell. This is a proven pipeline test, not evidence
of real-provider coding quality, arbitrary-file RSI, or SWE-bench parity.


## Crash-idempotent RSI adoption

Explicit user adoption first persists status adopting, then cherry-picks the
accepted and pinned source commit. If the process crashes, a later adopt call
compares repository HEAD's parent to the frozen base revision and HEAD's Git tree
to the accepted candidate tree. Matching state finalizes the existing commit
without replay; if HEAD is still at the base with a clean working tree, the
pending authorized cherry-pick runs once. Divergent or dirty checkouts require
manual reconciliation. An adoption mutex serializes concurrent adoption calls,
and the UI exposes recovery from adopting. This closes the Git/JSON crash
window for the single-process path, not distributed side effects.

## RSI autonomous repository exploration (2026-11-15)

`POST /api/v1/improvements` -> `Improvements::create` stores a trial without a fixed `target_file`. Following explicit execution approval, `Improvements::evaluate` freezes the original Git HEAD and calls `rsi_discovery::discover`. The new Rust module reads tracked file names via `git ls-files -z` without shell evaluation, filters test/generated/sensitive paths, ranks source names against the user's goal, caps catalog and input size, and sends a bounded manifest with source signatures to the configured provider. It accepts only structured JSON paths that match catalog entries, within 1-8 sources and 180 KB total. Selection reason, selected paths and provider cost become durable experiment state before worktree construction. Existing source modification, third-worktree verification, retry, accepted commit pinning, recovery and explicit adoption paths remain authoritative. The repo checkout is never edited during discovery.

The user can override automatic selection with exact source paths. Studio, TypeScript and native Rust all use the same `NewImprovement` request and persisted `Improvement` result. Discovery is intentionally restricted to Git-indexed code files; it is not yet a general multi-step repo-wide code generation planner, strategy evolution, or a replacement for the ordinary Engine tool loop. **Automatic CI trigger removed for this branch on user direction, no execution tests performed for this revision.**

## Evidence-derived, cross-run strategy transfer (2026-11-15)

- Durable truth: `src/store.rs` keeps verified agent runs and tool receipts; `src/improvement.rs` keeps frozen baseline/candidate results, accepted Git SHA, reflective hypotheses and adoption state. `src/strategy.rs` projects these into compact `StrategyEvidence` without an independent mutable strategy cache that could falsely promote an unadopted trial.
- Promotion rule: RSI advice is labeled *adopted* only after successful explicit Git adoption; an exhausted measured candidate can contribute only a *rejected* caution. Agent advice is labeled *verified* only when run status is Completed, a strict verifier PASS event exists, and at least one durable positive tool receipt exists.
- Evolution: `Improvements::evaluate_worktrees` gathers relevant past evidence and adds it as lower-trust context before bounded candidate creation. After judging, `strategy::reflect` optionally uses the economy provider on a bounded Git diff and measured evidence to create a conditional method hypothesis, with metered cost added to the trial. It never affects frozen scoring or adoption criteria.
- Reciprocal transfer: `Engine::new_with_mcp` attaches its existing durable `Store` to `Improvements` once; `Improvements::strategy_evidence` projects compact verified agent histories and adopted/rejected RSI trials only within the exact repository. Both the initial planner and subsequent tool execution projection can consult these cases. This provides RSI -> agent -> RSI evidence transfer without model-weight changes.
- Token bounds: source histories are inspected in place while locked; full Run messages and execution logs are not cloned for prompting. The user/model gets at most 3-4 ranked cases with a global 1800-character guidance cap. Stale case evidence is explicitly subordinate to the current user instruction.
- Public surfaces: `GET /api/v1/strategies` uses existing API authentication, Rust `Dominion::search_strategies`, TypeScript `strategies.search`, and Studio's RSI strategies view. Status, provenance, attempted changes, measured scores and costs remain accessible through normal improvement records.
- Limits: The reflection model proposes a hypothesis rather than an independently proven general law. Group/video product-specific execution strategy transfer, model-prompt Pareto population search and cross-repository federated learning are not yet implemented. This revision has not been compiled or tested on user instruction.

Operator control: `Settings.strategy_learning_enabled` reads `DOMINION_STRATEGY_LEARNING` (default true); the Engine applies it during planning and execution, and Improvements during RSI context selection and optional metered reflection. The read-only strategy history endpoint remains usable with the switch off.

## Current main frontend identity (supersedes prior partial UI adaptations)

Source SHA: `879dc23dce309688691c200b001b180bead95a12`. Exact `web/` subtree SHA: `9e2be9b7376c1eefb6195e3ea7f9463b952456c0`. The `web/` tree is entirely from main. Previous descriptions of customized `MemoryPage`, RustWorkspace, and adapted profile/home screens are historical and no longer describe mounted frontend code; their Rust backend data models continue to exist separately. Unimplemented `/api/*` contracts are explicitly tracked in `docs/FRONTEND_ALIGNMENT.md`. Tests/CI were not invoked for this sync.

## Main API contract adapters without modifying the frontend (2026-11-15)

The exact main `web/` Git subtree remains intact. The frontend now consumes the Rust-owned `src/frontend_api.rs` compatibility router mounted at `/api/*` alongside native `/api/v1/*`. Both routers apply the same operator-owned token middleware. This is not the main branch's legacy harness, database or second execution loop.

- `Engine.frontend` is a `FrontendStore` for ChatSession/AgentProfile records (clone-before-persist, 8 MiB cap, exclusive temporary snapshot and atomic rename). Responses use the original main camelCase types. The default agent is always tied to the configured native model.
- Main chat turns allocate a real `Run`, persist the user message and run binding, use the recorded agent instructions, then stream real tool/approval events from `Store`. A separate native watcher persists the terminal result even after a browser SSE disconnects. Restart-time session reads reconcile terminal run IDs. It never invents an assistant reply for a failed run.
- `ModelClient` remains operator-owned; the legacy model selector cannot silently override the live model. Unsupported settings and missing UI endpoints return HTTP 501. Dynamic provider registry, tool-free chat, user per-run reasoning-effort overrides and full workflow/video/RSI GUI contract are future integrations.
- `GET /api/executions` and `/api/sdk/runs` project real Run snapshots/receipts, do not invent separate execution state. `/api/memories` projects native Knowledge records. `/api/settings/owner-profile` uses the existing durable profile store.
- Default HTTP bind 4317 matches the **unchanged** main Vite proxy. `--home` and `--open` are accepted by the Rust server's CLI. The original main frontend source itself was not edited.
- Current route coverage, deliberate rejections and auth/stream limitations are in [FRONTEND_RUST_CONTRACT.md](FRONTEND_RUST_CONTRACT.md). No compilation/tests/CI were invoked for this stage.

## Main workflow API execution integration (2026-11-15)

The unmodified main WorkflowsPage now calls the actual `src/frontend_workflows.rs` REST router backed by `Engine.workflows` and the durable `src/workflow.rs` snapshot. A workflow save validates nonempty ordered agent steps and active native model routing. Its run snapshots the definition, checkpoints each step's native Run UUID before launch, applies input/previous answer substitutions, uses the existing agent harness and native approval policies, tracks per-step cost/tokens and enforces a cumulative 10,000 KRW model-run budget. Tool-approval waiting does not duplicate calls. Canceled/failed/paused runs are shown as real states; after restart the same saved step can be explicitly resumed with `/api/jobs/{id}/resume`. A completed job stores a durable chat-session transcript under `workflow-{jobId}` so the main UI's result link resolves. Distinct API and core modules avoid introducing a second execution engine. Old frontend files have not changed; build/tests/CI were not run.

## Main Chat controls and no-tools branch

`Run.strategy_learning` defaults true for all existing runs and uses serde default for older durable checkpoints. The real Chat entrypoint sets the flag from main's `autoLearning` and `Engine` gates strategy evidence retrieval by that flag. `strategy::from_run` refuses to learn from opted-out runs. When main Chat disables tools, `src/frontend_api.rs` creates a durable native Run without launching the autonomous tool harness; instead its worker issues one actual `ModelClient::complete(messages, [], false)` call, records real usage/cost, checks cancellation/terminal state before writing an answer, and uses existing session reconciliation/SSE. Both modes share native profile, policy, auth, and persistent stores, with no fake completion. Unsupported model reasoning controls are rejected. This is source-level work; no tests/CI were executed.

### Model-only chat restart and cancellation fence (2026-11-15)

Native `Run` now durably records `model_only`. The native tool runner refuses to launch such runs. On process recovery, an unfinished model-only request is durably failed with an interruption receipt instead of being replayed as an autonomous tools execution. The model-only provider HTTP future is dropped when the user cancels or its Run otherwise ceases to be running, and late responses cannot overwrite cancelled results. The existing conversation transcript reconciler sees only a confirmed completed answer. This fixes a mode-confusion risk in the previous Main Chat compatibility adapter. Changes are source-only; tests and CI were not run by user direction.

## Sub4 video production and artifact lifecycle (2026-11-16)

Entry: POST /api/video/productions accepts the existing NewProduction request and drives the same native Rust engine/model. ComfyUI duration 3-90 seconds is split into no more than 12 independently submitted jobs, each 3-9 seconds, and every returned child ID is durably recorded in data_dir/video-productions/jobs.json. The serde-defaulted rendered_shot_ids and completed_shots fields preserve compatibility with earlier journals. Each scene is inspected before advancing.

Completed scenes are normalized and concatenated using a process-bound FFmpeg filter graph, with ffprobe validation and a temporary output renamed into data_dir/video-productions/outputs/{production-id}.mp4. The final production ID resolves via GET /api/video/jobs/{id}/content and POST /api/video/jobs/{id}/reveal. The existing YouTube uploader accepts that same ID through POST /api/video/jobs/{id}/upload, reusing OAuth and upload journal logic; no alternative YouTube transport has been added.

On startup, in-flight productions are marked needs_review; no remote scene generation or upload is blindly replayed. Video model GPU charges and scene-synced narration are still outside this implementation. Generated ComfyUI child jobs are preserved rather than silently deleted. The product UI reports completed scene count and distinguishes script-model API cost from GPU compute cost.

### Sub4 video scene revisions, commit pointer and remote reconciliation (2026-11-16)

Routes: POST /api/video/productions/{id}/shots/{index}/regenerate accepts a replacement visualPrompt for a completed, unpublished ComfyUI production. The Rust journal first persists regeneratingShot, pendingRevisionShot and pendingRevisionPrompt, then launches a single new ComfyUI clip. The returned child job ID is durably written to pendingRevisionJobId. Each clip is independently checked with ffprobe before it can participate in the final timeline.

The scene compositor includes unchanged existing clip IDs, but swaps only the revised clip, normalizes aspect and frame rate, and writes a new immutable video-productions/outputs/{revision-uuid}.mp4. After validation, one journal mutation adopts the new artifactRevisionId and scene ID; the stable production ID resolves to the new output. A failed revision leaves the prior artifact pointer intact. Concurrent publication of a still-rendering revision is rejected.

On restart, regeneratingShot is cleared with a visible review error but the pending remote clip ID, original shot index and prompt remain durable. On explicit retry, frontend_comfy::reconcile_remote queries GET /history/{prompt_id} and downloads an existing successful artifact. A terminal remote error may permit a fresh submission; unacknowledged/uncertain work remains blocked. The system does not POST a new generation in reconciliation. Original main-stage ComfyUI queue jobs and YouTube uploads remain under independent journals.

Agent harness context comparison: 2026 long-horizon work recommends evidence-verified external task state and recoverable execution rather than trusting the model's own assertions; see https://arxiv.org/abs/2608.01964 and https://arxiv.org/abs/2608.14380. The present video journal implements this narrower verified-side-effect pattern; it is not evidence of matched frontier benchmark success.

### Rule-based context projection and trace integrity (2026-11-16)

src/context.rs projects durable messages for outbound model calls, never rewriting the stored history. For historical assistant tool_calls with argument strings exceeding 1,200 characters, the projection substitutes a well-formed JSON history_compacted marker but preserves tool function names, IDs and adjacent tool receipts. The same projection caps long tool text and excerpts of older directives, retaining user corrections. Tests verify that the original transcript is unchanged, the contracted tool-call arguments remain JSON parseable and projected characters fall below the chosen fixture budget. This is character accounting, not model-provider token billing. Reference: https://arxiv.org/abs/2609.20804 (September 2026 controlled harness experiments).


## Sub4: bounded task state and independently inspected evidence (2026-11-16)

User → /api/v1/runs or TypeScript runs SDK → Rust Engine → native model/tools → durable journal → independent verification → API/GUI. Engine remains the single owner of execution and state transitions.

The planning phase now passes the root request and exact subsequent direct user instructions through context::planning_directives, excluding raw tool/model histories from the paid planning prompt. context::compact_strict projects later model turns with system/root prefix, pinned direct user changes, and bounded whole assistant/tool exchanges. If the exact requirements cannot fit the configured context budget, execution pauses; requirements are never silently shortened. Optional memory/RSI strategy/skills are admitted only within the cap. Verifier-model feedback stays advisory and does not receive pinned user authority.

verification::verifier_messages owns the independent audit view: current read-only file and Git observations, durable tool result/failure/reconciliation events, root instruction and exact direct user amendments. Receipts are bounded and omission is reported as missing evidence. Strict PASS or REVISE remains the existing engine decision. A verified receipt is not invented from model narrative.

provider.rs bounds HTTP response bodies at 8 MiB and automatically retries or falls back only when the endpoint explicitly returns 429, denoting a rejected request. Transport failure, 5xx or incomplete bytes have uncertain billing and are never auto-replayed. An operator can inspect usage/previous outcomes before resuming. This is a conservative policy, not a provider-enforced exactly-once guarantee.

Sub2 and Sub3 implement scoped RSI read/edit decision checkpoints in src/engine/chat/runtime.rs; Sub4 keeps its own native Engine, durable journal and frozen independent RSI evaluator, while upgrading the shared input, verification and provider boundaries. Compare under identical models, budgets and task sets before asserting a frontier benchmark win. Source research: https://arxiv.org/abs/2610.31778 and https://arxiv.org/abs/2608.01964.


### Sub4 deterministic loop breaker and provider completion semantics (2026-11-16)

Native Agent path: Engine.drive → loop_guard::action (only prior durable tool-error receipts and exact tool names/JSON arguments) → tool_schema::admit / hooks → journal::begin → tools::execute → journal::finish → resumed Engine. Tool errors correlate to provider IDs and never fabricate execution. With two consecutive identical failures, the next unchanged model request is journal::reject_admission(REPEATED_TOOL_FAILURE); if it continues to four consecutive failed receipts, the following repeat is rejected and the Run status is atomically set to Paused with loop_guard_paused. A successful receipt, changed function input or direct user amendment breaks the streak. The pure check never mutates the stored Run and acts as a parallel read-batch dispatch barrier, avoiding a speculative rerun.

Provider path: ModelClient parses a bounded body and requires a usable terminal finish reason for OpenAI-compatible and Anthropic protocols. OpenAI stop/tool_calls and Anthropic end_turn/stop_sequence/tool_use are allowed when consistent with nonempty returned text or finished calls. Explicit truncation, filters, remote pauses, missing required calls, and empty completions fail with a recoverable error. Protocol errors are never auto-retried because the provider may already have billed for generating those tokens. Missing stop fields are accepted for compatible local providers when the answer otherwise has content. This conservative validation does not claim charge reconciliation for an incomplete response.

Evidence: src/loop_guard.rs unit tests and engine.rs local end-to-end test verify read I/O count, bounded model requests and persisted Paused state. provider.rs tests verify explicit truncation/final-status semantics and no replay. Follow Qwen Code 2026 loop protection and tool-call lifecycle principles without incorporating third-party runtime dependencies.
