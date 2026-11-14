# Native DOMINION Rust Agent SDK — Sub2 — 2026-11-14

## Responsibility and distinction

- **Canonical native SDK**: `src/lib.rs` and `src/sdk.rs`, public Rust library module `dominion_rust::sdk`.
- **Execution engine**: unchanged Rust modules under `src/engine/chat/`, `src/engine/tools.rs`, `src/engine/mcp.rs`, verification and durable jobs.
- **Production integration**: `src/api/chat.rs` -> `DominionSdk::from_state(...).run_transport(...)` -> `engine::chat::run` -> actual Rust model/tool runtime.
- **Web adapter**: `sdk/index.mjs` is explicitly optional, only a client-side network transport; it is not the native agent SDK or harness.

## Rust-first entry points

- `DominionSdk::open(EngineConfig)`: initialize standalone SQLite store, vault and migrations with the existing runtime state. The data directory must be owned exclusively by this process.
- `create_session`, `session`, `messages`, `session_usage`, `cancel_session`: durable conversation context.
- `register_provider`: persist provider configuration and encrypted API key via native Vault.
- `run(TurnOptions)`: execute complete Rust model/tool/verification cycle, return persisted assistant response and witnessed tool evidence.
- `stream(TurnOptions)`: run that native engine in a Tokio task and yield model, tools, errors and terminal done as JSON events via `TurnStream::recv`.
- `enqueue_prompt`, `job`, `start_job_workers`: existing job persistence, worker pool and startup recovery. Workers start once per embedded SDK instance.

The original user input travels unchanged into `TurnRequest::from_value`; there is no external agent policy engine or substituted third-party planner.

## Validation

The crate adds Rust tests for storage and encrypted provider secrets, an actual mock-provider chat turn that goes directly through the native model runtime and persists a message, SSE-equivalent Tokio event streaming and terminal errors, and durable prompt enqueue preserving the literal instruction. Prior JavaScript SDK tests are complementary, not evidence of native Rust agent completion.

No real paid-model full autonomous coding run is claimed by these tests. Goal planning, worktree promotion, RSI and video remain owned by the existing runtime and API; the native SDK currently exposes turn/session/job primitives, not all higher-level API surfaces.
