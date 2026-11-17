# Dominion Rust SDK — Agent Harness

The Rust library is the primary runtime interface, and HTTP, TypeScript and React are thin adapters. `Dominion::open` starts storage recovery and the user-defined automation scheduler; it does **not** initialize a video production service, RSI campaign queue or source auto-adoption engine.

## Submit and verify real work

```rust
use dominion::{Dominion, Settings, CreateRun, RunStatus};
use std::time::Duration;

# async fn task() -> Result<(), String> {
let sdk = Dominion::open(Settings::from_env()?).await?;
let work = sdk.submit(CreateRun {
    instruction: "Inspect a repository, repair the failing source and show test evidence".into(),
    budget_krw: Some(1500.0),
    max_steps: Some(30),
}).await?;
let outcome = sdk.wait(work.id, Duration::from_secs(120)).await?;
match outcome.status {
    RunStatus::Completed => println!("{:?}", outcome.answer),
    RunStatus::AwaitingApproval => println!("Tool approval required"),
    RunStatus::Paused => println!("Inspect and reconcile outstanding work"),
    _ => println!("Run status: {:?}", outcome.status),
}
# Ok(())
# }
```

Use `submit_from_repository` to work inside a separate Git worktree. For file results use `artifacts`, `read_artifact` and `read_subagent_file`. The repo and run workspace identity are never inferred from a browser session alone.

## Account-linked external work

The Rust SDK dynamically includes OAuth-backed tools through `connected_tools::definitions`, using the same native approval/recovery kernel as chat. Connected GitHub tools include branch, PR, issue, comment and file actions. Google tools cover Drive, Docs and Sheets.

The TypeScript SDK's `frontend.accounts.list()`, `frontend.accounts.providers()`, and `frontend.accounts.toolNames('google'|'github')` report account grants and tools without exposing credentials. The authorization flow starts at `/api/oauth/{provider}/start?repository=true` or `?workspace=true` and returns through the one-use callback. OAuth app credentials come from the product's server environment, never an end-user form.

Read-only calls are replay-safe. Mutating remote operations are not implicitly replayed after an uncertain receipt. See [Connected Accounts](CONNECTED_ACCOUNTS_2026-11-17.md).

## Event-driven execution and resume

The SDK's wait contract and browser chat transports share the Store's post-fsync Run change feed. Read-only status listeners do not repeatedly clone full transcripts while a provider model or approval is pending. Native Engine::launch now handles both autonomous agents and model-only chat runs with the same cancellation watch and cost accounting; the TypeScript adapter does not own a separate model worker. Lagged listeners re-check a durable snapshot. The main-product Jobs resume path now calls Engine::launch under its existing guard: terminal runs, unknown remote side effects, unresolved approvals, and active delegated children remain blocked.

## Recovery and ownership

- `get`, `list`, `events`, `wait`: durable work status and receipt inspection.
- `instruct`: authoritative mid-task user amendment; stale model responses cannot overwrite it.
- `approve`: grants only the exact pending tool name and arguments.
- `resume`: restarts only work that does not have an unresolved mutating side effect.
- `cancel`: preserves cancellation intent when an external operation may already have started.
- `reconcile`: records an **observed** outcome of a previous uncertain operation, never replays it.
- `delegate`, `delegate_read_only`, `read_subagent_file`: worker hierarchy, reserved budget, reviewable artifacts and read-only investigator mode.
- `register_tool_hook`: operator-defined pre-dispatch and post-receipt policy.
- `remember`, `search_memory`, `revise_memory`, `forget_memory`: user-owned, durable non-weight notes.
- `search_strategies`: explicitly opted-in, independently verified Run evidence only; no recursive trial generations.
- `list_skills`, `load_skill`: project SKILL.md procedures on demand.
- `create_group`, `say_group`, `group_turn`: explicit collaboration.

The model-facing tool set is selected in `src/policy.rs`, validated against its actual advertised JSON schema and protected by the operator tool hook. External MCP mutations remain behind the same durable approval and crash-reconciliation contract.

## No mandatory background learning

`DOMINION_STRATEGY_LEARNING=false` is the default. The ordinary Chat interface sends `autoLearning:false`. Storing a note is explicit, and historical Run receipts remain available. There is no SDK method for creating a recursive RSI campaign, self-evaluating candidate generations, promoting source revisions, built-in video generation or uploading finished video.

## Budgets and telemetry

`Run.cost_krw` is computed from provider-reported token usage and the rate table configured by the operator. Subagents reserve budget before being scheduled and return unused capacity only after durable settlement. Unknown prices or undelivered model responses cannot be treated as verified free usage. The process can pause when context constraints cannot fit without deleting user changes.

After fsync, `Store::subscribe` publishes `RunUpdate` records to the React SSE adapter. A slow listener can resynchronize without requiring every idle browser connection to repeatedly request the entire Run store.

## Historical archive and owner-selected memory

`Dominion::archive_legacy_sqlite(path)` copies only historical records from the main SQLite file into the standalone `legacy-history.sqlite`. It does **not** schedule old jobs, load old room workers or execute saved tool calls. Records remain accessible using `list_legacy_history(kind,offset,limit)` and `legacy_history_item(kind,id)` (maximum page size 50). The archive covers old memories, room/group settings, jobs, execution/operation records, workflows, experience, strategies, sessions and messages. The source stays read-only.

Old learned facts do not automatically become instructions. A user can inspect an archive entry then call `promote_legacy_memory(id,scope)` for a specific active record, which creates a separate `Knowledge` note under the chosen scope. Inactive memories and oversized/missing content are rejected; there is no implicit global promotion.

## Legacy data and main SQLite copy

The main app stores chat and agent records in `runtime/dominion.sqlite`; this branch's native runtime uses separate JSON journals. Migration is **explicit**: no old data is silently overwritten or submitted as a new executable job.

```rust
# async fn migration(sdk:&dominion::Dominion)->Result<(),String>{
let old_db = std::path::PathBuf::from("/absolute/main/runtime/dominion.sqlite");
let summary = sdk.preview_legacy_sqlite(old_db.clone()).await?;
println!("{} personal chats ready",summary.importable_sessions);
// Operator reviews the summary and consents before calling import.
let copied = sdk.import_legacy_sqlite(old_db).await?;
println!("{} chats, {} disabled providers copied",copied.imported_sessions,copied.imported_providers);
# Ok(())
# }
```

Supported: active private chats, agents and compatible provider configurations. Imported provider records are disabled and carry no secret. Existing IDs remain untouched, including on retries; source SQLite stays read-only. Old rooms, archived chats, memories, encrypted tokens, job history and other legacy record kinds remain solely in the original SQLite database. The native frontend snapshot has an 8 MiB atomic size limit. See `src/legacy_import.rs` tests for source preservation and duplicate import handling.

The pre-refactor video/RSI archives are not executed. Historical code remains in Git history, not in live runtime endpoints.

See [Architecture](ARCHITECTURE.md), [project README](../README.md) and [TypeScript SDK](../sdk/typescript/README.md).


## Money evidence and commercial research

`Dominion::money_latest()` finalizes and reads the same report as GET /api/v1/money/find; `Dominion::money_signals(limit)` reads a bounded source archive; `Dominion::money_collect(force)` triggers bounded RSS and public GitHub/Hugging Face collection with separate error reporting. The TypeScript `sdk.money` API additionally exposes DB-only three-lead discovery, the optional single native model-only inference, user-rejected leads, and collector opt-in. Cost/abort/Run journals are shared with the native agent engine; the Money SQLite file is only evidence and proposed opportunities, not a second worker scheduler.

### Money approval to native work

The Money HTTP adapter exposes `POST /api/money/candidates/{id}/implement` (also `/api/v1/money/...`), accepting an explicit `{approved:true,agentId,budgetKrw}`. It looks up a saved, non-rejected candidate, persists a single native Run ID in the same Money candidate record, and launches that Run via `Dominion::prepare_run` + `Engine::launch`. The request cannot start an independent provider or job runtime; replay returns the recorded Run. Approvals for later shell and connected writes remain owned by the existing native Run workflow.

Use `GET /api/v1/runs/{id}` to read status and cost; `POST /api/v1/runs/{id}/cancel` for cancellation and `POST /api/v1/runs/{id}/approval` for exact-tool confirmation. An absent or corrupt prior Run receipt blocks automatic re-launch. No customers are contacted or payments initiated merely by selecting a research candidate.

Native SDK parity: `Dominion::money_implement(candidate_id,agent_id,budget_krw,approved)` forwards to the **same** idempotent user-approved handoff as the REST endpoint. The TypeScript SDK uses `sdk.money.implement(id,{approved:true,agentId,budgetKrw})`; neither adapter invents a Money-only execution loop.

### Inert domain-approved Run reservation

`Dominion::reserve_unlaunched_run` is an internal helper shared with the Money adapter, not a new runner. It constructs a configured normal Run under the common SDK but persists it as `Paused` with the `reserved_unlaunched` event. The domain then saves an approval link and calls the existing `Engine::launch`. The ordinary public `prepare_run`, `submit`, `resume`, `cancel` and `reconcile` contracts remain unchanged for other SDK consumers. The Money UI resumes paused tasks via the native `/api/v1/runs/{id}/resume` route; an interrupted mutating operation remains blocked until reconciliation as before.

### Indexed Money evidence read (2026-11-18)

`sdk.money_search(query,limit)` performs a bounded FTS5/Korean-CJK-bigram search of the existing `money.sqlite` source archive. The shared HTTP adapter adds `GET /api/v1/money/search?q=...` and the TypeScript adapter adds `sdk.money.search(query)`; frontend users can search archived materials without invoking a provider model. Returned records remain observations and are not treated as verified contracts. A one-time lazy index backfill supports earlier saved source rows. `money_latest`, `money_signals`, `money_collect` and `money_implement` retain their original contracts. No SDK startup background indexer is added.

### Optional Money state and candidate decisions (2026-11-18)

- `Dominion::money_settings()` returns the module's persisted enabled state; `money_set_enabled(bool)` toggles it using the same HTTP handler lock and preserves all native Run records and accumulated Money data.
- `Dominion::money_candidates(status,query,offset)` returns a bounded page including held, rejected and lower-ranked candidates; `money_decide_candidate(id,action,reason)` records hold/restore/reject without discarding earlier decisions.
- HTTP: `GET/POST /api/v1/money/settings`, `GET /api/v1/money/candidates?status=held&q=...&offset=0`, `POST /api/v1/money/candidates/{id}/decision` with `{"action":"hold"|"restore"|"reject","reason":"..."}`.
- TypeScript: `sdk.money.settings()`, `sdk.money.setEnabled(true)`, `sdk.money.candidates({status:"held",q:"...",offset:0})`, `sdk.money.decide(id,"restore","new evidence")`.
- Fresh installations are disabled until explicitly enabled, with no idle Money SQLite initialization. Pre-existing Money databases stay enabled for backward compatibility. When disabled, the Money scan, find, archive search and implementation endpoints reject new work; previously authorized native Runs stay in the shared Run journal and can be separately cancelled or reconciled.

All entries remain research observations or model hypotheses unless independently confirmed. RSS `feedOriginal` contains the original feed item (up to 2,400 characters), not a live copy of the destination article. Release and frontend validation are outside this development step.

### Native execution context and tool admission (2026-11-18)

All regular Rust SDK/HTTP/TypeScript/GUI `Run` submissions still share `Engine::launch` and one persisted `Run.messages`/tool journal. Context projection now leaves full within-budget history intact; over-budget history pins exact direct owner changes and the latest bounded verifier repair feedback, keeping paired tool-call/receipt groups. No changes are made to public Run payloads, model credentials, user files, or existing journal serialization.

The configured denied-tool list is re-applied at actual dispatch, including queued calls restored from a prior process and tool calls explicitly approved earlier. A rejected call produces a durable `TOOL ERROR [OPERATOR_TOOL_DENIED]` correlation receipt; on-demand skill tools remain available after the one-time initial skill catalog hint. These are internal execution-policy upgrades and introduce no new client SDK endpoints.

### Money source revisions and optional price comparisons (2026-11-18)

`Dominion::money_source_history(source_id)` returns the current saved source, up to 16 preserved earlier/identical content snapshots, and up to 12 alternate URLs with the same normalized title. Legacy stored observations without revision rows are labelled `legacyRevisionUnavailable`; matching headlines are labelled not independent proof. `Dominion::money_compare_price(candidate_id, Value)` evaluates the owner's buyer/supplier amounts, currencies, same-day FX, spec/unit/quantity, and known shipping/fee/tax/labor/return/other costs, then saves an idempotent Money-domain receipt. `money_price_history(candidate_id)` reads up to 12 saved comparisons. Missing or mismatched fields return `spreadKrw:null` and explanatory issues; an algebraic difference never implies an executed sale. The same routes are available at `GET /money/sources/{id}/history`, `GET /money/candidates/{id}/prices`, and `POST /money/candidates/{id}/price-compare`, with the standard HTTP `/api/v1` prefix and TypeScript Money namespace. No extra Run/worker, automatic FX fetch or trading permission.

### Cross-domain reserved Run admission (2026-11-18)

An optional module using `Dominion::reserve_unlaunched_run` persists a deliberately inert `Paused` job with the `reserved_unlaunched` receipt. A normal `Engine::launch` / `sdk.resume` **rejects** that job until the owning module has committed its candidate/job association. The module then calls the internal `Engine::activate_reserved(id,domain,receipt)`, which records `domain_receipt_committed` in the **same native Run journal before** launch. A crash before activation leaves the job inert; a crash after activation leaves a durable receipt, allowing the standard Run resume path. The kernel owns all tool effects, model billing, cancellation and recovery, with no Money-special execution state. Money's user-approved candidate API alone calls this activation path; the GUI checks the Money candidate receipt before falling back to ordinary native resume.

### Memory recall and owner correction invalidation

A normal tool-agent Run still uses `Engine::drive` and the same shared persisted messages and owner-scoped `Knowledge` store. The engine caches its rendered top-four memory note context for the lifetime of an active Run. This avoids repeating the same lexically scored memory scan each model step. Cache entries are keyed by task, repository scope, recent direct user changes and committed memory revision; `remember`/`revise`/`forget` invalidate them only after successful disk writes. `model_only` remains a single non-tool request; the TypeScript SDK, external HTTP and GUI do not acquire a second memory API or execution loop. The cache is never used as proof that a real source or tool succeeded.
