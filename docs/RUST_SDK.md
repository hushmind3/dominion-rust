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
