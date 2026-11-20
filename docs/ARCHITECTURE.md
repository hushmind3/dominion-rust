# Dominion sub4 — current harness architecture (November 20, 2026)

## Scope

Dominion's active execution loop is native Rust. Neither the web UI nor TypeScript SDK owns a second decision engine. External services are available through explicit MCP/connected-tool adapters without embedded media producers or experiment schedulers.

```
User / Chat / TypeScript SDK / HTTP
                  |
           src/sdk.rs + src/api.rs
                  |
         src/engine.rs (one Run)
          /      |       \
   Provider    Tool/MCP   Delegate
       \         |       /
    Durable Journal + Store (fsync)
                  |
       Evidence-based verification
                  |
          Run status + artifacts
                  |
    Broadcast event → React SSE
```

## Real execution path

1. A request creates a `Run` in `src/domain.rs`. `Store::put` writes the JSON record with fsync before it becomes visible.
2. `Engine::launch` is the sole work scheduler. It uses an unbounded wake channel and active-task notifications; no four-worker 400ms full-job polling loop exists in this branch. The implementation is split into `src/engine.rs` (main drive), `src/engine/delegation.rs` (child work and budget settlement), and `src/engine/tests.rs` (native regressions).
3. `Engine::drive` plans with the pinned root instruction and direct user amendments. `context::compact_strict` reduces old conversation/tool receipt material without silently dropping new user instructions.
4. `tool_schema`, `policy`, `hooks` and `loop_guard` check each requested operation before dispatch. Two identical consecutive failed calls lead to a corrective rejection; repeated uncorrected loops pause instead of continuing work blindly.
5. Read-only tools may run in a bounded, model-ordered parallel batch. Mutations and external MCP calls remain exclusive barriers. Shell, MCP and merges need explicit approval unless an operator policy explicitly permits them.
6. A pending side effect is journaled as `in_flight` **before** dispatch. After execution, one durable result receipt and its correlation ID settle the intent. Interrupted uncertain effects are never speculatively replayed; manual reconciliation is necessary.
7. `verification::verifier_messages` builds a bounded audit input from exact user requirements, current workspace/Git observations and real tool receipts. The verifier requires an unambiguous `PASS` to finish the Run; a correction returns to the execution loop.
8. After each committed Store update, the broadcast channel emits one small `RunUpdate`. `/api/events` forwards these updates by SSE. A lagged client receives a resync indication. Idle tabs do **not** clone the entire stored job history every 1.5 seconds.
9. The frontend and SDK read the same native `Run` and memory records. Planning checks only direct-child existence; delegation limits use direct-child snapshots; settlement reads one consistent two-level family snapshot rather than cloning every historical Run. Explicit scheduled tasks remain available via `src/automations.rs`; retired RSI campaigns are not scheduled.

## Data ownership

- `src/store.rs`: durable native Run metadata, messages, planned tool calls, receipts, token/cost measurements and status. Startup diffs the semantic recovered Run against the loaded record and fsyncs only changed snapshots; unrelated historical records are not rewritten. `recovery_candidates` returns only paused/safe Run IDs instead of cloning every stored transcript into the SDK on restart.
- `src/knowledge.rs`: user-managed notes, corrections and reusable procedures.
- `src/frontend_state.rs`: chat sessions, agents and user-visible messages.
- `src/group.rs`: group requests.
- `src/frontend_system.rs`: encrypted backup/restore and maintenance.

Old-format records are not automatically replayed or deleted. Native chat, Run and memory stores retain their ordinary on-disk formats. Historical experiment receipts are quarantined from startup recovery and explicit resume.

## Frontend source and runtime contract

The React workspace was synchronized from main commit `a026ce8e237fde2272b92cf929c7a944337f1d38` into `dominion-sub4` while leaving the sub4 Rust engine, adapters, and databases intact. The main conversation layout retains mounted chat and group views when switching between them, with an overlay settings workspace. New components include `ConversationSettings`, `ExecutionPermissions`, `GrowthDashboard`, and `LearningRecords` along with source styles/locales.

The active `OperationsPage` exposes live execution status, settings and native backup/restore, without unmounted SQLite migration screens. The connected GitHub repository grant is in `ConnectionsPage`. The main event shell emits typed `dominion-refresh` events so the native `useRemote` hook can skip unrelated query refreshes.

The historical/research panels are frontend-only where an API is not implemented; they must not be mistaken for live native functionality. Growth panels are lazy-mounted on user expansion to avoid idle background API requests. Browser tests cover main chat/group navigation, settings and the repository authorization control against the native Rust API.

Follow-up frontend synchronization imports the latest main `web/src/refreshEvents.ts` scoped SSE router and its unit tests. Sub4 adds native post-fsync Run `job_event {id,status}` mapping so SDK, job and terminal session views update without forcing unrelated model/provider reloads. The `web` CI job runs `npm run test:refresh` before bundling.

## Provider and pricing boundary

`src/provider.rs` handles OpenAI-compatible chat completions and Anthropic Messages with selected models. A provider response reporting truncation or missing tool-call payload is not a final answer. The provider adapter applies a bounded 8MiB response limit. Only a definite HTTP 429 rejection is eligible for automatic retry/fallback: transport loss and HTTP 5xx may have already consumed billable tokens.

Configured KRW per-million-token prices are used for provider-reported usage; unknown third-party rates cannot establish a real zero cost. `policy::effective_budget_krw` reserves delegated child budgets and `Engine::account` durably records observed usage. These controls do not claim globally enforced exactly-once billing.

## SDK and API compatibility

- `Dominion::open(Settings)` in `src/sdk.rs`: native SDK entry and startup recovery.
- `src/api.rs`: native `/api/v1` entry including runs, approvals, reconciliation, skills, artifacts, groups and memory.
- `src/frontend_api.rs`: React UI `/api` adapter; no separate execution kernel.
- `sdk/typescript/src/index.ts`: typed, thin transport client.
- `web/src/App.tsx`: active Chat, Models, Agents, Groups, Connections, Memory, Operations and Developer navigation.
- `src/mcp.rs`: extensibility for external creative, search, code, data and other tools, with explicit consent and durable receipts.

## Validation and limits

Source-level changes were verified in the `dominion-sub4` branch, with new startup and child-family regression cases. GitHub Actions / live model success rates / elapsed-time and I/O deltas require separately observed runs. Correctness of migrated legacy data and Windows/macOS fsync semantics should be exercised on each target platform before declaring operational readiness.


Local and CI: `cargo check --all-targets`, `cargo test --all-targets`, TypeScript checks, Vite production bundle, browser Playwright against a real Rust service, external MCP interop, deterministic benchmark fixtures and 256-run crash-recovery/30-second continuous soak. Long-term success rate, cost and model quality need matched real-provider task sets.

The harness is an application runtime, not an OS security boundary. Approved shell tools require a properly configured sandbox. MCP remote side effects and provider spending require operator oversight; transcript evidence is not a substitute for remote reconciliation. Do not infer task success from model claims or omitted receipts.


## 2026-11-17 platform CI findings

macOS filesystem aliases (/var versus /private/var) and Windows extended canonical paths (\\\\?\\C:\\...) exposed real defects in the common workspace layer. Source enumeration now normalizes the workspace root and rejects paths that cannot be stripped to a relative name, rather than accidentally returning an absolute path. Git worktree attachment converts a validated Windows canonical target to the regular argument spelling for Git; child patch application retains the Windows PATH/SystemRoot while isolating HOME. Unit-test cleanup releases the native router/SDK owners and retries transient Windows sharing violations. These fixes concern the shared filesystem and workspace implementation. CI outcomes are reported separately from source changes.

## Development versus release validation policy (2026-11-17)

Per operator instruction, platform compile/build, frontend bundles/tests, soak and browser E2E are **release-only** and must not automatically run on branch pushes. `.github/workflows/check.yml` and `sub4-compile-smoke.yml` are both `workflow_dispatch` only. No build or test was invoked by the workflow policy change. Targeted native Rust development-profile tests are appropriate when requested and supported by the current environment; do not misrepresent source review as execution verification.

## 2026-11-18 Common harness context and admission invariants

`context::compact_strict` is a model-request-only projection of immutable durable `Run.messages`. If the full serialized message list fits `DOMINION_CONTEXT_MAX_CHARS`, return it exactly, without the old unconditional 16-group truncation. If not, preserve every original direct user change verbatim (or fail before an expensive provider call) and pin the most recent verifier repair request as bounded **advisory** feedback; then use atomic assistant-call/tool-receipt exchange groups for the remaining budget. This prevents the verification→repair loop from losing its latest actionable failure while keeping source receipts in the durable journal. Tool-sourced content never becomes a user instruction.

The native engine discovers skills at first execution turn, and subsequently uses the preexisting `list_skills`/`load_skill` dynamic tools rather than rescanning directories and extension metadata for every tool receipt. The engine still retrieves up-to-date scoped user memory as before.

`policy::admitted_tool` unifies read-only and operator-deny admission. Model catalog selection, parallel-read prefix barriers, normal queued calls and immediately-before-dispatch checks on previously approved calls use that policy. A denied call is finished as a correlated durable error without triggering the shell, MCP, connected account or native filesystem executor; permission retraction on process restart is respected. These hooks do not create a new worker, policy engine or DB state.

Benchmark scope: matching request/model/tools/budget and captured model/provider token usage, number of context insertions, repeated skills directory reads, completed artifacts and recovery. Without those measurements, changes are correctness-oriented and not claimed as frontier superiority.

## 2026-11-18 general native Run admission and revision-aware memory recall

**Per-Run durable-memory projection.** The unchanged scoped `Knowledge` store still holds notes in `knowledge.json` and writes them atomically. Its new in-process `AtomicU64` revision advances after committed remember/revise/forget only; a failed disk write cannot update the counter. The engine's small `focused_memory` map caches a string of the top four scoped memories keyed by Run ID, memory revision, repository scope, original task and the two latest genuine owner changes. Tool output and model messages cannot change this key or masquerade as a correction. One model turn reuses the cached body only while all keys match, and the cache entry is discarded at worker detachment. There is no database schema change, deferred memory writer, embedding model, independent cache worker or extra model call. A process restart initializes a new empty cache against persisted owner notes.

Verification scope: focused memory, `knowledge::`, `context::`, `policy::` and workspace-regression tests. No release build, frontend test or multi-platform gate; no measured savings/benchmark advantage are claimed from the source diff alone.

## 2026-11-18 failure-specific recovery signals and provenance-based evaluation

`Engine::drive` uses the existing durable Run timeline as its single signal source. After a model turn yields a failed tool or verifier correction, `repair_guidance::advise` examines only subsequent `tool_error`, `tool_admission_rejected`, `tool_policy_denied`, `tool_denied`, `tool_hook_denied`, `mcp_remote_outcome_unknown` or `replan` events since the most recent `model` event. Simple bounded deterministic pattern matching yields a fixed classifier ID and fixed safe next-action hint. The model is never given a tool's untrusted failure text as higher-priority instructions. The hint is inserted only if it fits the existing prompt cap and changes none of the already-persisted direct user instructions, tool admission, approval, in-flight journal or effect-reconciliation rules.

An accepted model turn records the applied strategy and triggering receipt sequence alongside that turn's input/output token counters in its *existing* Store transaction. The `repair_guidance::observations` read model ties selected strategy events to subsequent real tool receipts and verified completion, capped at 64 rows, with `temporal_association_not_causal_lift` explicitly stated. SDK and HTTP expose this passive projection for offline experiments, not automatic model selection. An actual causal comparison requires held-out cases and unchanged model, tools, task contracts and budgets; no unmeasured success uplift or real-model savings are asserted. This preserves the 2026 project directive to avoid old RSI-style continuous self-tuning systems.


## 2026-11-20 — Optional Integration Host (Sub4)

The native Run kernel remains the sole executor and journal. Side-effecting
adapters no longer appear as independent service fields in `Engine`.
The engine owns only `Arc<addons::Host>`, and the module physically groups
MCP protocol, managed MCP/skills registry, OAuth, connected accounts (GitHub
and Google) and outbound Telegram/Discord under `src/addons/`.

```text
Native SDK + Rust Run / policy / journal / budgets
            | authorized tool schema + approved dispatch
            v
    addons::Host  (optional, typed boundary)
       |-- MCP transport & lazy server tools
       |-- GitHub / Google connected-account tools
       |-- managed skills / OAuth registration
       '-- outbound channel adapters, delivery reconciliation
            |
       existing HTTP /api/* + existing Connections tabs
```

- `DOMINION_ADDONS_ENABLED=true` is the default; `false` excludes add-on
  API routes, remote schemas and outbound room notifications while retaining
  native models, chat, groups, filesystem and Run recovery. No external MCP
  env configuration or managed secrets are read in disabled mode.
- Existing `/api/mcp`, `/api/channels`, `/api/oauth` and skills URLs
  remain unchanged when enabled. Existing six sections under Connections
  remain unchanged. Subsection implementations load only on navigation.
- No second executor, channel-owned Run journal, implicit approval, or
  automatic remote mutation was introduced. Native journal and policy still
  authorize each model-requested tool effect.
- The UI and native SDK retain compatibility reexports for former
  `crate::mcp`, `crate::frontend_channels`, `crate::frontend_extensions`,
  and account connectors; their source ownership is now `src/addons/`.

This is a structural boundary and code-loading isolation, not evidence that
a channel delivery, an OAuth flow or a paid model response was tested in
production. Real HTTP and Chromium end-to-end checks remain separate gates.


## 2026-11-20 — Instruction kernel vs application services (Sub4)

The Rust `Engine` no longer owns a room collection, UI transcript
store, editable owner profile, provider registry, memory database, or MCP
registry as distinct execution-kernel responsibilities. The one
`ProductHost` field in `src/product/mod.rs` owns these optional product concerns.
Its `model_for_selection` adapts user-selected provider/local model settings;
`focused_notes` looks up opt-in memory and caches projections. Neither can
schedule or authorize a Run.

```text
Human language -> Core Engine (Run, provider request, tool plan,
                           admission, journal, verification/recovery)
                                  |
                           product boundary
                                  |
       +--------------------------+---------------------------+
       |                          |                           |
  Model catalog & UI      Memory/Owner profile        Rooms/Group UX
       |
     addons::Host  -> MCP / OAuth / connected accounts / outbound channels
```

Callers retain `engine.frontend`, `engine.groups`, and similar compatibility
field access through a Rust `Deref` to `ProductHost`, to avoid breaking
existing HTTP/SDK routing and stable data formats. **This is an ownership
extraction, not yet a separate library or subprocess**. Code review should
treat `src/engine.rs` as the Run executor and `src/product/mod.rs` as the
application adapter; new product state must not be added directly to Engine.
`DOMINION_ADDONS_ENABLED` continues to control external integrations.

No second scheduler, tool effect journal, approvals, independent agent
execution runtime, or new UI navigation were introduced by this change.

## Product-facing SDK surface extracted (2026-11-20)

`src/sdk.rs` now exposes the submission/control/recovery SDK methods.
Presentation session APIs, owner profile editing, room operations,
skill discovery, memory catalog and mixed external tool catalog are in
`src/sdk/product.rs`. These are separate `impl Dominion` blocks on
the same native instance, with the same single Store and execution journal.
No endpoint, tool authority, or persistence schema is renamed.

## Product adapter source-location boundary (2026-11-20)

All first-party `frontend_*.rs` modules, their nested session and room test
modules, and the product's group/store/catalog classes now live in
`src/product/`. Rust's public `crate::frontend_api`,
`crate::frontend_rooms`, `crate::group` and other compatibility paths
are re-exports from that package; existing API routes and React UI remain
unchanged. No detached renderer or second native executor exists.
The platform remains one binary by default; this is a **source ownership
boundary, not a separately distributed runtime or lower binary size claim**.

## Remaining optional services moved out of root (2026-11-20)

GitHub collaborative operations and skill browsing now belong to
`src/addons/`. Reusable strategy evidence/transfer belongs to
`src/product/strategy/`. The execution journal, direct user instructions,
verification of real tool effects and actual Rust model calls remain in
the native kernel. No strategy evaluator or second executor was introduced.
Public Rust aliases retain existing source/API compatibility.

## HTTP and build metadata are product adapters

`src/api.rs` moved to `src/product/native_api.rs`;
`src/build_identity.rs` moved to `src/product/build_identity.rs`.
`Dominion::http_router` and staged browser backup restoration now live in
`src/sdk/product.rs`. Native SDK run submission no longer imports HTTP
router types, frontend restore handlers, local model picker metadata or
provider catalog pricing logic. Stable public paths and HTTP endpoints are
retained through reexports. Model choice validation still occurs before
launch; price uncertainty still emits the same persistent event.

Root-module allowlisting in `scripts/check-sub4-architecture.sh`
prevents new product or integration source files from silently appearing
beside the native execution modules. A genuinely new core primitive requires
an explicit architecture-guard change and code review.

## 2026-11-20 — External tool dispatch port (Sub4)

The Run kernel no longer branches separately over MCP transport and connected
account implementations. It recognizes an authorized external tool and
submits exactly one request to `addons::Host::invoke_external`, retaining
the existing native write-ahead tool intent, approvals, cancellation,
uncertain-outcome reconciliation and single durable Run journal.

`src/addons/execution.rs` now owns MCP progress streaming, transport
cancellation, connected-tool invocation and live progress projection.
The generic settled-versus-uncertain effect contract lives in
`src/domain.rs`, not the MCP protocol module, while the old
`mcp::McpCallOutcome` Rust symbol remains a compatibility alias.
No second agent loop, new effect authorization path or menu was added.

## 2026-11-20 — Storage implementation removed from core (Sub4)

**Storage is infrastructure, not the language execution kernel.**
The former `src/store.rs` and its file/serialization/fsync tests moved to
`src/infrastructure/persistence/`, preserving the on-disk
`data_dir/runs/*.json` format and crash-recovery compatibility. No
migration or deletion of existing run records is required.

`src/ports.rs` now defines an **object-safe `RunRepository` port** with
async operations and atomic mutation callbacks, and a `RunStorePort` facade
that preserves the native executor's generic transactional updates.
`Engine` holds `Arc<RunStorePort>`, not `Arc<Store>`.
JSON encoding, directory paths, atomic rename, fsync, startup record recovery
and per-run write locks belong only to the concrete infrastructure adapter.

Application composition (`src/product/bootstrap.rs`) chooses/opens the
production repository and injects it through `Engine::new_with_repository`.
The actual `Dominion::open` boot and product restoration live under
`src/sdk/product.rs`. The native execution kernel only sees the committed
Run port. Compatibility `crate::store::Store` remains a re-export, so
external SDK and HTTP signatures/paths remain stable.

The pure `src/journal.rs` **Run state machine** remains in the kernel:
approve/deny, begin/finish effect, replay-safety and uncertain external effects
are *execution semantics*, not database I/O. This distinction preserves
the original write-ahead-effect safety invariant while allowing future
storage backends without modifying the executor.

This is an interface/ownership separation within the existing binary;
it does **not** claim that the production storage is disabled, that files
are no longer written, or that crashes can safely replay mutating effects.

## 2026-11-20 — Model and OS adapters relocated

Native **model HTTP transport**, filesystem/CLI tools, sandbox and workspace
implementation now live under `src/infrastructure/` rather than at the core
root. Optional native `read_program`/`run_program` execution implementations
live under `src/addons/`, and process/env model configuration under
`src/product/config.rs`. Their previous `crate::provider`,
`crate::tools`, `crate::workspace`, `crate::sandbox`,
`crate::read_program`, `crate::program_session`, `crate::program_ir`
and `crate::config` paths remain reexports for external SDK compatibility.

The kernel continues to own **instruction/run sequencing, policy,
tool outcome journaling, context compaction and verification**. Moving source
implementation does not by itself make tools independently uninstallable or
create separate binaries. The optional `run_program` schema and existing
approval semantics are unchanged.

## 2026-11-20 — Swappable reasoning/behavior algorithm, fixed Run runtime

`src/algorithms/mod.rs` introduces the object-safe **`AgentAlgorithm`**
strategy port. The kernel's `Engine` no longer owns its base system prompt,
fused/dedicated plan text, context projection, repair hints, local tool profile,
tool-discovery choice or independent auditor prompt. The concrete default
algorithm and its `context`, `tool_discovery`, `repair_guidance`,
`verification`, `loop_guard` and `local_task_catalog` source modules
are under `src/algorithms/`. They retain their original behavior by default.

The operator injects `Arc<dyn AgentAlgorithm>` via
`Engine::new_with_algorithm(settings,repository,groups,mcp,algorithm)`.
The normal `Engine::new` and `Dominion::open` constructors continue to
inject the default version. `DOMINION_AGENT_PROMPT_FILE` optionally selects
an operator-owned UTF-8 Markdown prompt (up to 16000 characters) at process
boot; its deterministic identity is checked against a persisted
`algorithm_bound` Run event on subsequent launches to prevent a resumed
Run from silently switching intelligence while its actions remain queued.

**Security direction**: `AgentAlgorithm::present_tools` receives only
already-authorized native/external schemas. The kernel rejects all invented,
rewritten or duplicate schemas and *still* enforces original `tool_schema`,
operator deny-lists, read-only child scope, approval hooks, durable
`journal::begin/finish` and reconciliation before any side effect.
No algorithm implementation can directly grant tool permissions or create
a second side-effect dispatcher. Neither the strategy nor its prompt owns
a filesystem or DB handle.

This is runtime/algorithm dependency injection in the existing Rust binary,
not yet a standalone distribution crate or generic dynamic plugin loader.

### Strategy purity and fail-closed authorization

`AgentAlgorithm::fused_messages` receives an **immutable Run** and returns
only outbound model messages. The runtime itself commits plan markers, run
status, journal/approvals and the durable `algorithm_bound` event. A strategy
cannot mutate pending approvals or bypass the event ordering through its API.

`Engine::admit_algorithm_catalog` rejects a proposed model-facing schema
unless it is byte-for-byte equal (as JSON `Value`) to a previously
operator-authorized catalog entry. Duplicate names are rejected even when
the schema is identical. The final answer's audit condition is also fixed
in the runtime rather than allowing a pluggable strategy to redefine PASS.

## 2026-11-20 — Cargo Workspace, actual agent loop ownership

The native package is now a **three-member Cargo workspace**. The production
`dominion` binary remains the product/SDK/gateway composition root (all HTTP
paths and UI remain unchanged). `crates/dominion-agent-core` is a no-dependency
foundation crate with the immutable `TurnSnapshot`, `LoopStep` and
object-safe `AgentLoop` contract. `crates/dominion-agent-algo` depends
only on that foundation and provides the independently testable `ReActLoop`
controller. The product depends on both; **neither crate depends on the
product, persistence, provider, UI or external tools**.

The *single real* `Engine::drive` and tool-free chat loop move from
`src/engine.rs` into `src/algorithms/runner.rs`, preserving all
receipts, approvals, billing and re-entry semantics. The algorithm determines
phase precedence (pending approval, native program continuation, queued tool,
fused/dedicated planning, max steps, model round), while the engine owns
`perform_tool` and its durable journal, policy and side effects.

The structure is intentionally honest: the real loop is now physically in
the algorithms module and its pure control policy is in an independent crate,
**but the loop's async body still implements `Engine` in the product crate**,
so complete process-level/ABI isolation is a later boundary. Moving it to a
separate distributable crate will require decoupling `Engine` from
`ProductHost`/Deref and moving `Run`/repository adapter types to foundation.
`Cargo.lock` lists all workspace packages, and CI checks the independent
crate contracts. No WASM sandbox is claimed or implicitly enabled.

### Resumption pins both reasoning and loop identity

The first normal Run launch persists `algorithm_bound` (prompt/reasoning
strategy) and `loop_bound` (pure turn controller). A later restart that
selects a different component refuses to resume existing queued work before
any external effect is dispatched. Existing legacy Runs without a loop marker
bind to the currently configured default controller on their first upgrade.

## 2026-11-20 — Runtime capability port and product adapter

The async engine/agent loop now accesses model selection/rate-limiting,
external-tool advertisement/approval/invocation, durable knowledge search/write,
focused note retrieval and run-scoped cache disposal solely through the
object-safe `src/runtime_services.rs::RuntimeServices` capability port.
Concrete implementations live in `src/product/runtime_services.rs` and own
provider registry, MCP integrations and durable memory. The runtime contains
no direct `self.providers`, `self.addons` or `self.knowledge` calls.
The kernel continues to own all tool permission checks, approval journals,
in-flight intent, receipt persistence and effect reconciliation.

`Engine` currently retains an `Arc<ProductHost>` only for historical native
HTTP/SDK field-access compatibility. Its worker and algorithm depend on
`Arc<dyn RuntimeServices>` rather than the ProductHost implementation.
This is an incremental, real service-injection boundary: remove the
compatibility Deref once callers are converted to explicit product access
or an application facade, without changing HTTP contracts.

### Durable group turn work-board projection

`src/product/coordination.rs::RoomWorkBoard` now owns the scheduling
projection used by the existing group frontend handler. It derives ordered
task tickets, assigned speaker identities, completion status and
source-indexed result receipts from the **existing durable Room transcript**.
There is no duplicate persistence or chat-to-chat agent chatter. Replaying
the same room after restart produces the same pending agent set; a new
owner message creates a separate ticket generation.

A round-robin frontend owner turn now freezes its selected speaker in
`Room.recipients` **on initial `Groups::say_once` admission**, before
any model billing or responses occur. A retry after partial completion
cannot change speaker as `Room.rounds` increments. Existing routes,
room UI, request/response contracts and sequential write serialization
remain intact. This is a durable task board for room turns, not a generic
parallel agent DAG executor or a new database.

### Direct runtime service injection (native SDK embedders)

`Engine::new_with_runtime_services(settings,repository,groups,mcp,algorithm,
loop_driver,services)` accepts an operator-implemented
`Arc<dyn RuntimeServices>`. The concrete product host remains a compatibility
surface for existing HTTP routes, while model resolution, request throttling,
tool advertisement/invocation and memory retrieval/save used by workers are
exclusively dispatched through the supplied runtime service port. A regression
test injects a deny-only provider and an isolated mock memory result and
asserts neither request falls through to the normal ProductHost.

### Agent inference execution also goes through host port

The native `src/algorithms/runner.rs` no longer constructs a
`ModelClient` or calls `ModelClient::complete` directly. Planning,
normal tool-capable decisions, tool-free chat and optional independent
verification all invoke `RuntimeServices::complete`. The default
product adapter supplies the original model routing, stable UUID-bound
provider affinity and metered Reply. Alternate operator-provided
implementations may use local or remote inference without rewriting
the agent scheduler. Existing `Engine::model_for_run` stays available
for source-compatible SDK/HTTP clients, separate from worker execution.

## 2026-11-20 — Canonical core / harness / strategy ownership

Source of truth for the native sub4 execution path:
- **Core/kernel:** `src/engine.rs`, `src/journal.rs`, `src/policy.rs`,
  `crates/dominion-agent-core` own durable transition authority, strict tool
  admission, budgets, cancellation, approvals and recovery.
- **Mandatory harness:** `src/harness/runner.rs` is the sole implementation
  of `Engine::drive` and `Engine::drive_model_only`. All model and external
  tool I/O passes through injected `RuntimeServices` while the kernel owns
  execution authorization.
- **Optional/reasoning strategies:** `src/algorithms` and
  `crates/dominion-agent-algo` contain pure prompts, context projections,
  turn selection and guidance; they DO NOT own or instantiate a runner.
- **Coordination:** `src/product/coordination.rs` schedules room speaker
  work through the existing group engine. An independent multi-agent chat
  channel is not to be confused with a separate privileged tool harness.

Moving the async body out of `src/algorithms/runner.rs` changes physical
ownership, not the runtime semantics or existing SDK/HTTP contracts. This
is a modular-monolith layout, NOT a new binary, WASM module or microservice.

### Indexed Room read cursors (source of truth: append-only Room journal)

Each successful accepted group response now commits a
`RoomReadCursor` keyed by participant within the same atomic Room
snapshot as the original utterance and provider usage receipt. The
cursor stores the *next unread source message index*, prior accepted
reply index and latest owner instruction index. The work-board
projects unread index spans separately for each participant.
Read positions do not advance on upstream provider failure or a
billed response rejected by room budget controls. `Room::unread_range`
derives the same read floor from original messages for legacy Room
snapshots with no cursor field. Reset clears cursors. Serialization
uses defaults and preserves compatibility with existing room JSON.

This is a deterministic *index* on source events, not a synthetic
LLM-memory replacement and not an instruction to forward every
speaker message into every other speaker's prompt. Room context
projection retains existing bounded/cacheable source excerpts.

### Capability-aware local/API inference (2026-11-20)

The host ports now advertise typed `ModelCapabilities`, with
`Unknown` / `Supported` / `Unsupported` states. Capabilities are never
invented from model names or vendor marketing. The provider registry
optionally stores `toolCalling: true|false` for the exact selected
endpoint/model; older entries remain unknown and backward compatible.
An explicit `false` rejects tool-capable agent requests before charging
a model call and persists a `model_capability_denied` Run event.
Tool-free chat continues to work. A changed endpoint, protocol or model
does not inherit an old tool capability declaration.

## 2026-11-20 — Durable Task Team on one native Harness

The team coordinator is a non-executable native Run with a versioned
\`team: Option<TeamGraph>\` containing at most 16 owner-authorized tasks.
Each task is assigned its own stable Run UUID, dependency list, role,
logical workspace lease, model-step ceiling and reserved KRW budget
before any model request. Team creation rejects dependency cycles,
unknown/duplicate task IDs, invalid scopes, and over-budget plans.

\`Engine::advance_task_team\` and native SDK counterparts schedule
dependency-ready workers through the **existing** Engine::launch and
journal. There is no second agent loop. Investigators and reviewers
are read-only, and only one Writer in a given logical workspace
is admitted at once. Normal Run approval, runtime tool admission,
cost receipts, cancellation and recovery are unchanged. A crashed
coordinator reuses persisted task Run IDs; only safely queued
workers are auto-launched, not paused/uncertain/approval-held work.

A Task Team root is never runnable as a model agent. The root only
reaches completed when all workers completed and delegated budget
settlements are durable. Failed prerequisites block dependants.
Owner cancellation is propagated to live workers with in-flight
reconciliation preserved. This native API intentionally adds no
menu or HTTP route. An event driver must call advance_task_team
after worker completion; this is not yet a permanently scheduled DAG.

### Automatic durable Task Team wakeups

Once an owner calls \`advance_task_team\` for the first time, \`team_started\`
is durably recorded. Child settlement posts the coordinator Run ID to the
existing Engine wake channel; the same scheduler then plans the next DAG
layer. No nested model execution or second worker loop exists. On restart,
\`Dominion::open\` rechecks started Team roots and resumes only the safe
ready/queued work. Draft teams without a start marker are never launched
by simply opening the application.

### Source-typed common event projection (Room / Run / Team / Chat)

\`domain::events::UnifiedEvent\` provides a versioned, typed envelope with
origin, source ID, stable event ID, actor, kind and original source payload.
Run/Task Team and append-only Room events expose source-local \`seq\`.
Editable presentation Chat messages retain their persisted message IDs and
a separate timestamp, never an invented monotonic sequence. The native SDK
offers \`unified_run_events\`, \`unified_room_events\` and
\`unified_chat_events\` for one consistent observer schema. These are
READ-ONLY projections: Run journals and original Room transcripts remain
the authoritative effect/receipt sources; a Chat message is NOT a tool
execution receipt and no UI/HTTP contract is changed.
