# Native Rust SDK · Dominion Sub4

The Rust library is the primary interface, not a wrapper around a hosted agent
SDK. All execution decisions stay within the in-process Rust engine.

## Native usage

Add Dominion as a Rust dependency (for development, point the path to this
checkout). Use the same environment settings as the executable:

    use dominion::{Dominion, Settings, CreateRun};
    use std::time::Duration;

    # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    let app = Dominion::open(Settings::from_env()?).await?;
    let run = app.submit(CreateRun {
        instruction: "Create an HTML page and verify its output".into(),
        budget_krw: Some(1000.0),
        max_steps: Some(40),
    }).await?;
    let completed = app.wait(run.id, Duration::from_secs(120)).await?;
    println!("{:?}", completed.status);
    # Ok(())
    # }

Core operations:

- submit/submit_parallel: schedule durable Rust-owned agent workers.
- submit_from_repository: attach a Git source checkout to an isolated worktree.
- delegate/delegate_read_only: spawn an isolated child with persistent budget escrow. Read-only investigators cannot modify files, run shell or integrate Git patches. Parent completion is suspended until children finish and the unused reservation is settled.
- remember/search_memory: reusable non-weight knowledge, persisted per project or globally.
- list_skills/load_skill: on-demand project SKILL.md procedures; full content stays out of the default tool prompt.
- get/list/events/wait: inspect persisted progress and usage.
- instruct: inject user directives; the engine rejects stale model responses.
- approve/cancel/resume/reconcile: durable intervention and crash-side-effect repair.
- artifacts/read_artifact/read_subagent_file: workspace-confined results.
- create_group/group_turn, create_video/render_video, create_improvement/
  evaluate_improvement/adopt_improvement: the same product feature services.
- http_router: an optional API adapter; TypeScript Studio uses this, not an
  alternative model decision loop.

The Rust library API currently evolves with the implementation. The /api/v1
HTTP surface is versioned but not yet a frozen stable interoperability contract.

## Real project coding

A source repository must be beneath the configured DOMINION_WORKSPACE_DIR.
The detached worktree is created at workspace/<execution-uuid>. The initial
source commit and relative repository are saved in the durable Run record.
Exact file edits require one matching original snippet, avoiding risky blind
replacement. Read-only Git status/diff let the model inspect its edits.
Command execution requires shell_enabled and explicit user approval. Linux Bubblewrap or macOS Seatbelt isolation is enforced by default; DOMINION_SHELL_SANDBOX=off explicitly disables this boundary. An approved merge_subagent tool can apply a completed child's Git patch only after conflict checking in the isolated parent worktree.

## Important limits

Rust has tool-level durable write-ahead intent and a fail-closed OS sandbox path for approved shell and RSI commands, without comprehensive resource quotas or cross-platform parity. Subagents do not merge automatically; patch integration requires a separate approval. Model context projection remains a char-count heuristic, not an independently measured token reduction guarantee. Cache billing uses provider-reported read/write usage with configurable KRW rates; missing usage cannot be reconstructed.

No model-weight update, LoRA, or fine-tuning mechanism is linked into RSI.
There is no goals.rs or goals.api; objectives and mid-run directives remain
inside the existing Rust engine's execution loop.

## Validation status

Source and JS syntax were inspected during this refactor. Rust compilation,
provider E2E, desktop GUI navigation, OS sandbox evaluation and SWE-bench
comparison are still required before declaring frontier parity. Targeted Rust/TypeScript checks passed in GitHub Actions 43686024696;
full browser E2E and matched real-provider evaluation remain pending.

## Optional TypeScript integration

The TypeScript package now exposes delegate requests with read_only, per-run SKILL.md discovery and load, and project-scoped memory search/store. All calls go through the same independent Rust HTTP adapter. The package does not provide a separate model loop or agent policy.

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
Rust compilation and 64/64 tests passed for the focused tool orchestration
implementation (GitHub Actions 43686024696). Global product parity is still unverified.


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
5. Cancellation is durable and signaled to active model requests and pure reads with Tokio watch receivers. Unknown non-idempotent writes, external MCP calls and delegation remain receipt-safe and cannot be blindly replayed.
6. Environment controls: DOMINION_DENIED_TOOLS (exact tool names), DOMINION_TOOL_RESULT_MAX_CHARS, DOMINION_MAX_PARALLEL_READS and DOMINION_PARALLEL_TOOL_CALLS. Rust API, optional TypeScript SDK and the main React UI share the same runtime state.

Tests added for delayed model request cancellation, pre-hook veto preventing filesystem writes, approved-call re-veto, post-result caps, model-ordered concurrent reads, and durable crash recovery. Real-provider cancellation cannot reverse charges already incurred upstream.

Unfinished: full external-tool-specific cooperative cancellation and quiescence acknowledgements, persistence of dynamically registered native extension hooks across process restarts, and same-model/same-task frontier benchmarks.


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


## Multi-file RSI SDK and evaluation

Dominion::create_improvement accepts NewImprovement with primary target_file
and optional target_files (a total of 1-8 explicit source paths). The same
target_files field is supported by POST /api/v1/improvements, the TypeScript
improvements.create method and Studio. The old target_file-only request is
still valid and stored experiments without target_files retain single-file
behavior. An experiment runs under explicit evaluation approval; it tries
up to three model revisions against a separate immutable-base judge worktree,
retaining real test outcomes, measured score and billed cost in the record.
Only a demonstrably improved candidate can be adopted into the original
checkout; descendant keeps the target set and the predecessor's reflection.

One candidate ref is kept under refs/dominion/rsi/<trial-id> until successful
explicit adoption. In the fixed-command CI fixture, DOMINION_SHELL_SANDBOX=off
is passed only to the isolated test process. This is not a supported production
security shortcut and does not imply measured real-provider accuracy.

## Repository-aware RSI without manual file input

`NewImprovement::target_file` can now be an empty string, or omitted from JSON API requests. `NewImprovement::target_files` may be `None` to enable native Rust Git source discovery after approval. The response persists `target_files` and `selection_reason` after the model identifies relevant modules. The frozen judge and existing adopting state continue to control actual source changes. No external agent SDK is involved.

## Evidence-based strategy retrieval (2026-11-15)

`Dominion::search_strategies(repository, goal, max)` returns bounded, project-specific `StrategyEvidence` with source run/trial UUID, provenance, measured scores where available, attempts, actual provider charges and a conditional method note. Adoption and strict completion, not model opinion, determine whether a candidate counts as positive evidence. The native agent plan and RSI candidate generator query the same evidence internally; the REST adapter exposes `GET /api/v1/strategies`. This non-weight strategy layer is implemented within Rust and is independent of the configured provider.

Strategy reuse is controlled by `Settings::from_env` / `DOMINION_STRATEGY_LEARNING=false`; the getter still allows users to inspect the recorded evidence without executing new model calls. `PublicSettings.strategy_learning_enabled` reports its effective value.

### Project-scoped memory management

The native Dominion SDK exposes list_memory(scope), memory_scopes(), revise_memory(scope,id,title,content) and forget_memory(scope,id) in addition to remember/search_memory. Changes affect the same durable Knowledge dataset used by agent tools; revisions do not move notes across project boundaries. The HTTP and TypeScript SDK equivalents are listed in docs/UI_FEATURE_PARITY_2026.md. Deletion requires an explicit memory ID and matching scope.

## Owner profile for the canonical frontend snapshot

Native Rust `Dominion::owner_profile()` and `Dominion::save_owner_profile(NewOwnerProfile)` use `OwnerProfileStore`, owned by `Engine`. Rust REST provides `GET/POST /api/v1/profile`. Profile edits use the original `879dc23d` OwnerProfile frontend component with the endpoint remapped to Rust. The server enforces 1–60 printable name characters, a data URL with PNG/JPEG/WebP magic bytes, and a 1 MiB decoded avatar limit. Profile is durable under `Settings.data_dir/owner-profile.json` and is not an instruction to the agent.

## Unchanged main frontend adapter

Rust `Dominion::chat_sessions()`, `chat_session(id)`, and `agent_profiles()` access the same durable records used by `/api/sessions` and `/api/agents`. The existing `Dominion::submit`, `approve`, `cancel`, `reconcile`, and `tool_catalog` own execution while `src/frontend_api.rs` projects the original main `/api/sdk/runs`, `/api/chat`, `/api/approvals`, `/api/executions` protocols. The main front-end remains unmodified, and unimplemented main actions return explicit 501. The default bind/TypeScript SDK base URL is now `http://127.0.0.1:4317`.


### Sub4 run-context and billing guarantees (2026-11-16)

No SDK endpoint changes. runs.create, runs.resume, runs.cancel, runs.reconcile and event streaming still use the native Rust Run. When the exact task instructions cannot fit DOMINION_CONTEXT_MAX_CHARS, the worker enters paused state with paused_error; there is no additional model call. The operator can raise the limit and resume the same durable Run.

Model retries and economy-to-main fallback are allowed after an explicit HTTP 429 rejection only. HTTP 5xx, dropped connections and incomplete response bodies are uncertain outcomes: the client does not silently initiate another billable generation. Reported KRW model costs still depend on configured token prices and actual provider usage; a zero configured rate is not evidence that the external inference was free.


### Sub4 stuck-loop and incomplete-generation signals (2026-11-16)

No new SDK methods: existing create/get/events/instruct/resume/cancel endpoints use the native Rust Engine and journal. If a model repeatedly issues an identical failed tool call, run events include loop_guard (the execution was blocked and the model may correct it) or loop_guard_paused (the Run is Paused without dispatching the latest tool). The original correlated provider call ID remains in the tool receipt. Resuming can be accompanied by a new direct user instruction through the existing instruct action; this resets the guard's consecutive-failure streak. A successful tool response or changed tool arguments also resets it.

Explicit provider finish reasons indicating truncated or filtered output produce paused_error before the response is considered a completed plan, tool invocation or final answer. This avoids false completion; actual provider cost for an incomplete billed response may require separate operator inspection and is not represented as a successful Run charge.
