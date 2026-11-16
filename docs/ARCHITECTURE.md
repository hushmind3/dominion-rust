# Dominion sub4 — current harness architecture (November 17, 2026)

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
