# DOMINION — Rust Autonomous Agent Harness

**Branch:** `dominion-sub4` · **Architecture:** Rust execution kernel, thin TypeScript SDK, React UI, external MCP tools.

Dominion is a user-directed autonomous work platform, not a recursive self-modifying research engine. Its job is to **complete tasks**, inspect real tool results and produce verifiable artifacts while preserving approval, cancellation, cost budgets and crash recovery.

## Active architecture

| Layer | Rust source | Responsibility |
| --- | --- | --- |
| Execution kernel | `src/engine.rs`, `src/engine/delegation.rs`, `src/domain.rs`, `src/journal.rs` | Plan, execute, pause, approve, cancel, delegate, reconcile, verify |
| Durable state | `src/store.rs` | Fsync-backed Run records; commit only recovery-changed snapshots on restart, scoped descendant lookup and post-commit event notifications |
| Model adapters | `src/provider.rs`, `src/provider_registry.rs` | OpenAI-compatible, Anthropic Messages and local model endpoints |
| Tools and MCP | `src/tools.rs`, `src/mcp.rs`, `src/tool_batch.rs`, `src/tool_schema.rs` | Native workspace tools and external MCP, capability checks, bounded parallel reads |
| Reasoning constraints | `src/context.rs`, `src/policy.rs`, `src/verification.rs`, `src/loop_guard.rs` | Pinned user requirements, budgets, evidence checks, stuck-loop protection |
| Memory and collaboration | `src/knowledge.rs`, `src/group.rs`, `src/strategy.rs` | User-controlled project notes, explicit groups, retrieval of opted-in verified runs |
| Product adapters | `src/sdk.rs`, `src/api.rs`, `src/frontend_api.rs`, `sdk/typescript/`, `web/` | Single Rust engine reached through SDK, HTTP, web UI |
| Main legacy compatibility | `src/legacy_import.rs`, `src/provider_registry.rs` | Explicit, read-only SQLite preview; idempotent copy of active private chats, agents and disabled provider configurations |

**Deliberately retired:** built-in FFmpeg/ComfyUI/third-party video generation, audio narration and YouTube publishing pipelines; recursive RSI campaigns, multi-generation source experiments, independent self-scoring, source auto-adoption and automatic core changes. External video/audio/3D/CAD workflows may be connected by the operator over **MCP**. Their tool calls use the same approval, receipts, budget and recovery mechanisms.

**Retained:** chat, owned workspaces and files, user notes, provider selection, tool protocols, skills, agent delegation, groups, explicit scheduled work, task history and system backup. Existing local video and RSI archive files are not deleted by this migration; their old execution APIs no longer run. Prior Git history contains the removed implementation for auditing.

## Quick start

Requirements: modern Rust toolchain, Node.js 24.12+ for the frontend/TypeScript SDK. No FFmpeg, video model downloader or TTS runtime is required.

```bash
cp .env.example .env
# Export the variables you need (source the .env file in a trusted shell).
# Supply your model endpoint and key through the environment.
cargo test --all-targets
cargo run -- --home . --open
```

The default Rust HTTP address is `http://127.0.0.1:4317`. For frontend development:

```bash
cd web
npm ci
npm run dev
# Vite proxies API calls to the Rust service.
```

Key settings:
- `DOMINION_BASE_URL`, `DOMINION_MODEL`, `DOMINION_API_KEY`: selected native model endpoint.
- `DOMINION_ECONOMY_*`: optional explicit economy-model choice; no silent fallback after uncertain billable model failures.
- `DOMINION_DATA_DIR`, `DOMINION_WORKSPACE_DIR`: durable records and workspace files.
- `DOMINION_MAX_STEPS`, `DOMINION_CONTEXT_MAX_CHARS`, `DOMINION_MAX_PARALLEL_READS`: execution and context limits.
- `DOMINION_ENABLE_SHELL=false` unless explicitly configured; potentially mutating shell operations require approval and OS isolation.
- `DOMINION_MCP_SERVERS`: operator-supplied external MCP servers (JSON).
- `DOMINION_STRATEGY_LEARNING=false`: no default strategy learning or repeated background RSI.
- `DOMINION_INPUT_KRW_PER_MTOK`, `DOMINION_OUTPUT_KRW_PER_MTOK`: owner's actual KRW token prices. Zero/unconfigured is **unknown**, not free usage.
- `DOMINION_ACCESS_TOKEN`: mandatory on non-loopback binding.

## Native Rust SDK

```rust
use dominion::{Dominion, Settings, CreateRun};

# async fn demo() -> Result<(), String> {
let sdk = Dominion::open(Settings::from_env()?).await?;
let run = sdk.submit(CreateRun {
    instruction: "Fix a failing test and report the observed result".into(),
    budget_krw: Some(1200.0),
    max_steps: Some(20)
}).await?;
println!("Run ID: {}", run.id);
# Ok(())
# }
```

Public API families: `/api/v1/runs`, `/api/v1/groups`, `/api/v1/memory`, `/api/v1/strategies`; the React UI uses compatible `/api/*` routes backed by the same Rust Store. The retired `/api/v1/videos` and `/api/v1/improvements` return JSON 404; removed studio/RSI routes return JSON errors, never a fake success or the app's HTML.

## November 17, 2026 sub4 maintenance

- Boot-time recovery rewrites only semantically changed Run records, not every historical snapshot. Uncertain write outcomes remain paused until explicitly reconciled.
- Model-step child detection, delegation limits, parent settlement, and SDK restart selection no longer clone unrelated Run transcripts. A consistent two-level family snapshot preserves the existing descendant settlement condition; SDK startup selects resumable Run IDs only.
- The native execution engine keeps one scheduler; delegated work and its tests are isolated in `src/engine/delegation.rs` and `src/engine/tests.rs`.
- Regression coverage was added for unchanged historical JSON bytes and direct/grandchild lookup isolation. Passing CI and measured latency improvement are **not** claimed until their results are inspected.
- See [sub4 2026 implementation comparison](docs/SUB4_IMPLEMENTATION_REVIEW_2026.md) for real source-level comparisons and remaining gaps.

## Import existing main-product chats (explicit opt-in)

In **Operations → Settings → 기존 메인 데이터 가져오기**, enter the absolute path to the original main app's `runtime/dominion.sqlite`, inspect the preview, and explicitly confirm a copy. The source opens **read-only**, including when the old app is using WAL. Nothing is imported on ordinary startup.

The importer copies non-archived private chat sessions, messages (including original tool/reasoning data as inert metadata), and agent definitions. Compatible OpenAI/Ollama/Anthropic provider configurations are copied **disabled, without API keys, OAuth tokens, or vault secrets**. Existing native IDs always win; repeated imports do not duplicate conversations. A failed native snapshot write does not replace the current state.

**Not migrated yet:** archived and room/channel-bound sessions, private credentials, original job/execution history, memories, unrelated SQLite record kinds, or legacy vault contents. These remain in the original SQLite file. Native frontend data is still limited to 8 MiB per atomic JSON snapshot, so an oversized import fails instead of partially overwriting the active snapshot. A provider-registry save is separately retryable if the session copy succeeded first.

The same operation is available through `Dominion::preview_legacy_sqlite` / `import_legacy_sqlite`, the authenticated `/api/system/legacy-sqlite/{preview,import}` transport, and `sdk.frontend.legacySqlite`. Import requires `confirm:true`, and the source path must be absolute and end in `dominion.sqlite`.

## Historical main-product archive and selective memory transfer

The existing **Operations → Settings** page now provides a separate **이전 데이터 열람** panel. After explicitly selecting the original `dominion.sqlite`, operators may:

- Copy current private sessions/agents and disabled provider configurations as before.
- Copy historical memory, group, job, execution, operation, workflow, experience, strategy, session and message records into **`legacy-history.sqlite`**. The source stays read-only; the archive has indexed, paginated browsing. Repeated archival only updates changes and does not execute or resume the original jobs.
- Open archived records and **explicitly promote a selected active memory** to the named current note scope. Memory is not automatically injected into the model context on import.

The legacy archive is a separate SQLite store with an indexed `(kind,updated_at,id)` lookup. Records are kept inert, including old tool call arguments. It is not a migration of the old job execution engine, encrypted secrets or live room participant state. Source snapshots and native Run journals remain independent. This minimizes duplication of event loops and isolates old records from current approval/verification contracts.

**Concurrency and failure:** SQLite transactions protect each archival batch. Frontend copy, provider import and historical archiving remain separate, explicitly retryable operations. No cross-database atomicity is claimed; failures show which operation did not complete. Users retain the original database.

## Verification

```bash
cargo test --all-targets
cd sdk/typescript && npm install && npm test
cd ../../web && npm ci && npm run typecheck && npm run build
# Browser end-to-end test requires Chromium:
npm run e2e
```

CI separately checks native Rust API, TypeScript, browser E2E, external MCP interoperability, fsync crash-recovery endurance and deterministic benchmark-runner invariants. These tests establish regression safety, **not** a measured victory over frontier models.

See [Architecture](docs/ARCHITECTURE.md) and [Native Rust SDK](docs/RUST_SDK.md). Older 2026 feature studies in `docs/` are historical research notes; the active runtime and API are defined by the current source tree.
