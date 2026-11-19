# Dominion sub4 — current harness architecture (November 20, 2026)

## Scope

Dominion's active execution loop is native Rust. Neither the web UI nor TypeScript SDK owns a second decision engine. No separate generation-based recursive self-improvement process is started during boot, chat, or ordinary coding runs. Video/audio generation and publishing are external tool responsibilities via MCP, not built-in worker subsystems.

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

## Data ownership and migration

- `src/store.rs`: durable native Run metadata, messages, planned tool calls, receipts, token/cost measurements and status. Startup diffs the semantic recovered Run against the loaded record and fsyncs only changed snapshots; unrelated historical records are not rewritten. `recovery_candidates` returns only paused/safe Run IDs instead of cloning every stored transcript into the SDK on restart.
- `src/knowledge.rs`: user-managed notes, corrections and reusable procedures.
- `src/frontend_state.rs`: chat sessions, agents and user-visible messages.
- `src/group.rs`: group requests.
- `src/automations.rs`: **user-created** recurring work only.
- `src/workflow.rs`: explicit workflow jobs.
- `src/frontend_system.rs`: encrypted backup/restore and maintenance.

The removed video/RSI execution modules are no longer compiled or initialized. Existing files in `DOMINION_DATA_DIR` are **not** deleted or rewritten as a side effect of module removal. A separate, reviewed migration would be required to export historical retired-format records for new UI viewing. Native chat, Run and memory stores retain their ordinary on-disk formats.

## Main SQLite read-only compatibility boundary

Main's `records(kind,id,data)` and `messages` tables are read through a **query-only SQLite snapshot**, never opened for schema updates or writes. `src/legacy_import.rs` converts supported agents, active private sessions and messages into `FrontendStore`'s copy-on-write/fsync snapshot, preserving original message IDs and inert tool metadata. The same loader also previews supported provider records, which `Registry::import_disabled` persists with **no credentials and enabled=false**. Existing IDs are never replaced.

The copy is operator-triggered from the existing Operations/Settings page, the native Rust SDK, or the thin TypeScript adapter. There is no automatic boot migration, secret substitution or replay of old jobs. Incomplete migrations remain retriable because each target store skips existing IDs. The main DB and WAL remain independent and unchanged.

Archived/room/channel sessions, the old vault, memory and execution artifacts are **not imported by this path**. The 8 MiB frontend JSON cap remains and prevents a too-large copy from committing. A full migration needs explicit compatibility semantics and separate tests for those historical formats.

## Historical archive and memory promotion

`src/legacy_archive.rs` provides a separate, indexed SQLite archive populated from the main `dominion.sqlite` through `legacy_import::open_legacy` (query-only read snapshot). Only specifically enumerated nonsecret history categories enter the archive: memory, room, job, execution, operation, workflow, experience, strategy, session and message. It is not mounted into `Store<Run>` or the scheduler, preventing old tool intents and stale queued jobs from firing. Existing keys are updated only when source bytes/timestamps change. A successful re-import can refresh completed job receipts.

`Dominion::list_legacy_history` and `legacy_history_item` expose paginated, bounded owner-facing queries; the same native functions back the React Operations archive panel and typed TypeScript SDK. `promote_legacy_memory` is the **only** crossing into live `Knowledge`: it requires a specific archived memory ID and target scope plus an API confirmation. Deleted, archived and superseded memory records are refused. The original archive remains unchanged.

The historical archive is separate from the 8 MiB chat frontend snapshot, keeping large job transcripts out of the ordinary UI/session write path. It is not an automatic prompt memory cache; no redundant background scan runs at startup. The browser panel only fetches requested archive pages and selected record details.

## Frontend source and runtime contract

The React workspace was synchronized from main commit `a026ce8e237fde2272b92cf929c7a944337f1d38` into `dominion-sub4` while leaving the sub4 Rust engine, adapters, and databases intact. The main conversation layout retains mounted chat and group views when switching between them, with an overlay settings workspace. New components include `ConversationSettings`, `ExecutionPermissions`, `GrowthDashboard`, and `LearningRecords` along with source styles/locales.

The preserved sub4 `OperationsPage` continues to mount the `LegacyImportPanel` and `LegacyHistoryPanel`; the connected GitHub repository grant is reattached in the main `ConnectionsPage`. The main event shell emits typed `dominion-refresh` events so the native `useRemote` hook can skip unrelated query refreshes.

The historical/research panels are frontend-only where an API is not implemented; they must not be mistaken for live native functionality. Growth panels are lazy-mounted on user expansion to avoid idle background API requests. Browser tests confirm main chat/group navigation, settings, legacy import/history visibility and the repository authorization control against the native Rust API.

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
- Video / RSI route families: retired and not advertised. Unknown native API calls return JSON errors.

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
