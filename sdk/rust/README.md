# Dominion Rust SDK

**A first-party Rust crate for the native Dominion Rust autonomous-agent engine.**
Source: `sdk/rust`. This is **not** the TypeScript SDK and does not call
Codex/Copilot/Claude SDKs to execute work.

## Native thread control

`DominionClient::create_sdk_thread`, `DominionThread::run_native_streamed`,
`DominionThread::run_native`, `DominionThread::steer`, and
`DominionClient::sdk_thread_history` talk to the server's native
`/api/sdk/threads` surface. The older generic `/api/chat` and sessions
APIs remain supported. The same Rust scheduler, tools, provider and
SQLite records execute both, without another third-party agent harness.
SSE interruptions and remote model errors are not retried as new runs.

## Verifiable code goals

```rust
let goal = GoalCreate::new("Repair the source and verify tests")
    .isolate_worktree(true).require_tests(true)
    .token_budget(100_000).max_iterations(16);
```

The native engine checks real tool outcome receipts including later failed tests; `token_budget` limits future model steps according to observed usage, not already billed tokens.

## Build / test

```bash
cargo test --manifest-path sdk/rust/Cargo.toml --all-targets
cargo run --manifest-path sdk/rust/Cargo.toml --example goal -- \
  "Inspect the repository, fix a bug, and test the change"
```

The crate is a separate Cargo package, currently `publish = false`.
Users can reference it via a local `path` or `git` dependency:

```toml
[dependencies]
dominion-sdk = { path = "/path/to/dominion-rust/sdk/rust" }
tokio = { version = "1", features = ["full"] }
```

It connects to the existing Dominion Rust service (default `127.0.0.1:4317`).
The service continues to own model selection, tools, policy, persistent
checkpoint state, execution evidence, and permissions; the SDK never replaces
its reasoning harness.

## Native Rust API

```rust
use dominion_sdk::{DominionClient, GoalCreate, ModelSelection, WaitOptions};

# async fn run() -> dominion_sdk::Result<()> {
let sdk = DominionClient::new("http://127.0.0.1:4317")?;
let goal = sdk.start_goal(
    GoalCreate::new("Implement a change and verify it")
        .max_iterations(16)
        .isolate_worktree(true)
).await?;

let final_record = goal.wait(WaitOptions::default()).await?;
println!("status={} evidence={:?}", final_record.status, final_record.evidence);

// Append a durable instruction; retries use the same request ID.
let _ = goal.append_instruction(
    "Read the sources and create a passing test", "instruction-check-001",
).await?;
// Replacing the entire plan is an explicit, separately recorded directive:
// let _ = goal.replace_plan("Investigate a different method","instruction-002").await?;
// A review-pending goal can resume after owner acknowledges unknown effects:
// let _ = goal.resume(true).await?;
# Ok(()) }
```

The SDK returns strongly typed `Goal`, `Job`, `Session`, `Message`,
`ChatEvent`, `ServerEvent`, and `DominionError` values. Extra server fields
are preserved by a `serde_json::Map` to remain compatible with new Rust
engine record attributes.

### Durable sessions + chat SSE

```rust
use dominion_sdk::{DominionClient, SessionCreate};
use futures_util::StreamExt;

# async fn run() -> dominion_sdk::Result<()> {
let sdk = DominionClient::new("http://127.0.0.1:4317")?;
let thread = sdk.start_thread(SessionCreate::default()).await?;
let mut events = thread.run_streamed("Summarize current task progress").await?;
while let Some(event) = events.next().await { println!("{:?}", event?); }
let messages = thread.messages().await?;
# Ok(()) }
```

Streaming a `POST /api/chat` has **no automatic retry**. If SSE ends without
`done`, the stream raises `DominionError::Interrupted` rather than rerunning a
potentially side-effecting tool. Drop the Rust stream to stop the client reader;
cancel a durable job/goal using its explicit cancellation API.

### Reconnectable server event feed

```rust
use dominion_sdk::{DominionClient, WatchOptions};
use futures_util::StreamExt;

# async fn run() -> dominion_sdk::Result<()> {
let sdk = DominionClient::new("http://127.0.0.1:4317")?;
let mut events = sdk.watch_events(WatchOptions {after: 12, ..Default::default()});
while let Some(event) = events.next().await {
    let event = event?;
    println!("seq={} kind={}", event.seq, event.kind);
    // Persist this cursor AFTER your side effects are committed.
}
# Ok(()) }
```

`GET /api/events?after=<seq>` is replayable and safe to reconnect.
The SDK resumes with the last delivered sequence, rejects inconsistent IDs,
and removes duplicate events on reconnection. If the client falls behind the
server's 500-record replay window, reconcile by fetching current goal/job state.

## First-party Rust MCP tool plugins and hooks

The SDK includes a **real Rust stdio MCP server**, not a vendor runtime or a JSON-only example. Build the bundled native executable:

```bash
cargo build --manifest-path sdk/rust/Cargo.toml --bin dominion-sdk-tool-server
```

Register it using Dominion's existing `POST /api/mcp` record API, with absolute `command` path:

```json
{"name":"Rust SDK Tools","enabled":true,"transport":"stdio","command":"/absolute/path/to/dominion-sdk-tool-server","args":[],"approval":true}
```

The Rust backend's `StdioSession` performs the handshake and resolves
`mcp__rust_sdk_tools__word_count` through `tools/list` and `tools/call`.
Use `ToolRegistry::register` to implement custom async Rust tools and
optional `before_tool`/`after_tool` host hooks. The model never gets to
register a callback or modify approval policy just by naming one.

The subprocess wire path is tested in `tests/mcp_stdio.rs`.

## Actual scope

- Native `DominionClient` Rust type, authenticated HTTP and error classification
- Autonomous goals: create/list/get, idempotent append/replace owner steering, explicit resume, cancel,
  bounded polling, persisted change/evidence retrieval
- Prompt jobs: create/get/list/wait/cancel
- Durable session/thread create/fork/resume/stream/persisted answer
- Read-only execution audit and operations, approval review/resolve
- Provider/agent/tool/capability discovery
- SSE for chat and reconnectable, cursor-based event monitoring

`sdk/rust/tests/sdk.rs` runs actual local Axum HTTP/SSE mock servers
and verifies the SDK's interaction contracts. Tests do not claim full production
model execution or genuine provider invoices.

### Still not implemented

This release is a **Rust SDK client for the native runtime**, not an in-process
embedded engine library. A separate Rust server must be running.
Native Rust callbacks are available via MCP stdio. In-process engine embedding\nand ACP transport are still separate enhancements.
