# DOMINION — Rust execution harness (Sub4)

**[한국어 README](README.ko.md)** · [Architecture](docs/ARCHITECTURE.md) · [Rust SDK](docs/RUST_SDK.md) · [TypeScript SDK](sdk/typescript/README.md)

Dominion Sub4 is a **Rust-native, local-first AI agent execution harness** with the current main-branch React/Vite interface. The Rust SDK owns model/tool rounds, durable run receipts, policy checks, user approvals, recovery and bounded context; the browser is an adapter rather than an independent executor.

> **Branch scope:** `dominion-sub4` is an independent implementation, not a merge of the main Rust backend. The frontend source is synchronized from main commit `93dcee8c5d385528d886662abd694abf339c8ee8` (2026-11-19). The Rooms send endpoint is switched to a Sub4 background endpoint to avoid long HTTP requests during 9-speaker fanout. The redundant vendor discovery tab has also been consolidated into the existing provider exploration/connection screen; other main UI navigation remains. New main UI actions without a native backend adapter return an explicit error; they are not represented as working integrations.

## Features

- **One native execution kernel:** `src/engine.rs`, `src/harness/runner.rs`, `src/domain.rs`, `src/infrastructure/persistence/mod.rs`, `src/journal.rs`. Agent steps, delegated work, policy and approval decisions share durable records.
- **Provider choice:** `src/infrastructure/model_transport.rs`, `src/product/provider_registry.rs`. OpenAI-compatible API, Anthropic, Ollama and configured models. Provider credentials are not inherited when the target endpoint changes.
- **Group rooms:** `src/product/group.rs` and `src/product/frontend_rooms.rs`. Nine-person owner turns use bounded provenance-aware context, persisted speaker receipts, cancellation-safe turn reservation, background execution, progress snapshots and same-client-message-ID retry. Both HTTP transports share one message-admission contract; stable agent/knowledge projections are not rebuilt for every speaker.
- **Tools and MCP:** `src/infrastructure/local_tools.rs`, `src/addons/mcp.rs`, `src/algorithms/tool_discovery.rs`; explicit scope and approval for side effects. Provider replies claiming a terminal `stop` while containing executable tool calls are rejected before the native Run can dispatch them; no speculative retry is performed after a possibly billed response.
- **Persistent context and memory:** `src/algorithms/context.rs`, `src/product/knowledge.rs`; user corrections and durable evidence survive restarts.
- **SDKs:** `src/sdk.rs` and `sdk/typescript/`; typed native or HTTP entry points rather than duplicated task schedulers.
- **Main user interface:** `web/` is synced to the current main React navigation and pages, including model exploration, accounts, memory, rooms and chat. Hugging Face public GGUF/MLX search, revision-pinned downloads, pause/resume/cancel, local file registration, installed-model removal and local OS pickers are bridged into the Rust API. Imported GGUF models run via the loopback Ollama adapter inside the same Rust Run orchestration; a native MLX execution driver, automatic model-engine installers and other main-backend-only integrations still require separate work.

The main Chat's manual compact command persists a bounded excerpt of older messages without deleting the original transcript. The manual Learn command saves explicit owner messages as provenance-labeled global memory. These are native projections, not hidden model summarization or a second agent scheduler.

[Repository duplication audit and preserved contracts](docs/SUB4_DEDUP_AUDIT_2026-11-20.md) · [Retired subsystem and dependency audit](docs/SUB4_RETIRED_CODE_AUDIT_2026-11-20.md) · [Frontend and account-sign-in cleanup](docs/SUB4_FRONTEND_AUTH_CLEANUP_2026-11-20.md)

## Develop and verify

Prerequisites: Rust stable, Node.js >= 24.12.0, npm.

```bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
git checkout dominion-sub4
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
node --experimental-strip-types --test web/tests/*.test.mjs
cargo check --locked --all-targets
cargo test --locked --lib
```

Start the backend with `cargo run -- --home .` and Vite in another terminal with `npm run dev --prefix web`. By default the Rust API is at `http://127.0.0.1:4317` and Vite at `http://127.0.0.1:5173`.

## Room contract and 502 diagnostics

The standard UI posts `/api/rooms/{id}/messages/background`. A successful admission returns HTTP **202** and a running job. Actual speakers, model usage, cached/uncached input tokens and context provenance are written to durable room state. Read `/api/rooms/{id}/state` and `/api/rooms/{id}/messages` to observe completion; reuse the original `clientMessageId` to resume an interrupted owner turn without creating another owner message. Provider transport failures are distinguished from browser/proxy timeouts by `lastProviderFailure`. An already-billed request that loses its transport connection can still have uncertain external billing.

The original room transcript is preserved locally. A stateless model endpoint still receives a projected history each call; this is **not** the same as model-side KV-state reuse. Provider-reported prompt cache hits and OpenAI Responses continuation are accounted for separately. See [measurement protocol](docs/SUB4_ROOM_FANOUT_MEASUREMENT_2026-11-19.md).

DOMINION Cloud free inference uses a shared D1 daily request allowance. Explicit validation rejections release quota; ambiguous network, 429 or 5xx failures retain the in-flight allowance, and free-auto does not replay those calls to another supplier. This prevents an automatic double dispatch when the original provider may have billed. It does not establish exactly-once external billing or a token-cost spending cap. See the [Cloud gateway contract](cloud/README.md).

## CI and status

[Sub4 frontend and Rust development CI](.github/workflows/sub4-dev-rust.yml) runs frontend and SDK TypeScript checks, SDK/Worker/UI tests, all-target Rust checks, native library tests and the benchmark contract. Production bundles and real-browser end-to-end runs are reserved for a manually dispatched gate, keeping branch-push feedback focused. [Manual macOS/Windows checks](.github/workflows/check.yml) remain a separate release/platform gate.

**Verification must be read from the specific GitHub Actions run.** Source alignment, tests present in the repository, and locally verified execution are different claims. This branch does not claim release readiness or full parity with the main backend.
