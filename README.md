# DOMINION
### Rust-native autonomous agent execution, with durable evidence and operator control

**한국어:** 도미니언은 모델이 실제 파일·코드·도구 작업을 수행하고, 승인·취소·비용·복구·결과 검증을 **하나의 Rust 실행 커널**에서 관리하는 자율 에이전트 플랫폼입니다. 현재 문서는 독립 개발 브랜치 `dominion-sub4`를 설명합니다.

**Development status — 18 November 2026:** Active development branch, **not a production-readiness or frontier-benchmark claim**. The native targeted Rust workflow passed at `0ebb0851`. Full Windows/macOS release validation, browser E2E, live-model recall/latency and paid-provider cost benchmarks are separate gates.

[Architecture](docs/ARCHITECTURE.md) · [Rust SDK](docs/RUST_SDK.md) · [TypeScript SDK](sdk/typescript/README.md) · [2026 Sub4 audit](docs/SUB4_CODE_AUDIT_2026-11-18.md) · [Latest targeted Rust CI](https://github.com/hushmind3/dominion-rust/actions/workflows/sub4-dev-rust.yml)

## Why DOMINION

**One native Run instead of competing harnesses.** Chat, the React interface, HTTP routes, the Rust SDK and the TypeScript client reach the same Rust-owned work loop. Durable tool intents and receipts are shared across execution, approval and recovery.

**Completion evidence, not just plausible answers.** Workspace changes, Git observations, tool results and explicit acceptance requirements can be examined before declaring work finished. Independent model auditing is **opt-in**, and a tool receipt or model assertion alone is not a benchmark-grade proof of correctness.

**Token-aware context.** A stable task prefix, bounded historical projection, lazy skill references, permission-scoped tool disclosure and provider-reported cache usage reduce unnecessary context and catalogue overhead without rewriting the persisted conversation.

**Controlled side effects.** Shell operations, connected-account writes and remote effects follow tool admission, operator policy, approval, cancellation, durable receipts and uncertain-outcome reconciliation.

## Architecture

~~~text
React / Chat / TypeScript SDK / REST
                  |
          Native Rust SDK + API
                  |
            Engine::drive (Run)
         /          |           \
   ModelClient   Native tools    Delegation
                 /    |    \
         Workspace  MCP  GitHub / Google
                  |
          Policy + hooks + approval
                  |
         Durable Run journal / Store
                  |
        Evidence, artifacts, SSE events
~~~

| Component | Implemented path | Current role |
| --- | --- | --- |
| Native execution | `src/engine.rs`, `src/engine/delegation.rs`, `src/domain.rs` | Single scheduler, tool/model steps, delegation, cancellation, budget accounting |
| Durability | `src/journal.rs`, `src/store.rs` | Fsync-backed intent/receipt records, restart selection, explicit uncertain-effect reconciliation |
| Provider adapters | `src/provider.rs`, `src/provider_registry.rs` | OpenAI-compatible Chat Completions, Anthropic Messages and native Ollama chat; per-request selected model |
| Tool admission | `src/policy.rs`, `src/tool_schema.rs`, `src/hooks.rs`, `src/tool_batch.rs` | Schema validation, deny rules, owner approval, bounded independent reads |
| Context and memory | `src/context.rs`, `src/knowledge.rs`, `src/repair_guidance.rs` | Pinned owner corrections, optional scoped recall, evidence-directed repair hints |
| Programmatic tools | `src/read_program.rs`, `src/program_ir.rs`, `src/program_session.rs` | Bounded read graph and journaled mixed local steps, without a second model executor |
| Connected capabilities | `src/mcp.rs`, `src/tool_discovery.rs`, `src/connected_tools.rs` | 2026 Streamable HTTP MCP and authorized GitHub/Google actions |
| Product adapters | `src/api.rs`, `src/frontend_api.rs`, `src/sdk.rs`, `sdk/typescript/`, `web/` | Native API, existing React navigation, typed SDKs and Run progress |
| Optional Money | `src/money/` | Source archive, search, hypothesis/provenance review and approval-backed implementation |

The original chat, settings, memory, Run, approval and file stores remain under operator-owned data directories. Legacy main-app SQLite import is **explicit**, source-read-only and limited to supported record types. Historical jobs are archived inertly; they are not re-executed on import.

## What works in this branch

- **Autonomous workspace work:** read/write/edit/search, Git inspection, approved shell, model-to-tool rounds, correction, cancellation, restart and artifact access.
- **Program execution:** `read_program` can combine bounded dependent/parallel inspections in one model tool request; `run_program` advances a bounded mixed local-tool program through the **existing** Run journal. Shell still requires explicit permission; remote account mutation is excluded from nested programs.
- **Multi-agent work:** budget-reserved child Runs, bounded delegation, persisted group/room turns and per-speaker usage receipts. Rooms are not a second scheduler.
- **MCP and accounts:** Streamable HTTP server discovery, operator-scoped `tool_search`, endpoint TTL/authorization-generation cache, singleflight concurrent listing, and connected GitHub/Google tools when the owner has configured authorization.
- **Memories and skills:** scoped user notes, failure evidence and lazily loaded project skills; no unseen skill script gains executable permission merely by being listed.
- **Money research:** opt-in source collection, SQLite/FTS5 multilingual evidence retrieval, versioned source records, pending/rejected candidate review and explicit price scenarios. A candidate can start a shared native Run **only after user approval**.

### Tool discovery and context efficiency

When more than **8** external tools are eligible, `tool_search` exposes up to **3** intent-relevant original schemas and discovers others on request. Every discovered tool still passes the usual admission and approval pipeline. Small catalogues remain eager; tool disclosure is not an authorization grant.

Context is compacted **only when the configured character budget requires it**. The original Run transcript remains durable, direct owner amendments survive context pressure, and complete tool-call/receipt pairs are retained. Optional notes are appended after the stable dialogue prefix instead of reordering earlier messages. **Room limits are separately scoped:** `maxTurns` is the maximum number of agent responses to ONE owner message (0 = no per-message cap). `lifetimeMaxResponses` is the optional lifetime cap across the entire room (0 = 10,000 native ceiling). A 9-member discussion with `maxTurns=10` can complete 3 owner messages × 9 agent replies = 30 durable transcript messages. Retry with the same clientMessageId cannot reset a per-owner cap. A reached per-owner cap leaves the room open for the next owner message; lifetime budget and paid tokens remain cumulative.

**Coding usage is now reported consistently:** actual provider prompt/cached/uncached/cache-write/output counters are exposed on native jobs, SDK runs, execution listings and benchmark receipts. OpenAI-compatible gateways that return `usage.input_tokens/output_tokens` are supported alongside `usage.prompt_tokens/completion_tokens`. Missing upstream token counts are marked `partial_or_unreported` rather than presented as zero-cost model work. `completed` remains a native execution state; artifact-change evidence and independent semantic acceptance are separate reported outcomes.

**All group entry points are cache-isolated:** `/api/rooms`, native `/api/v1/groups/{id}/next`, and Rust SDK `Dominion::group_turn(id)` now use the same per-room UUID-derived model affinity. The default process-wide ModelClient ID is used only for direct unbound calls, never to merge different room sessions.

**Rust lifetime correctness:** the native cache-key selector explicitly ties the `ModelClient` and `ModelSettings` borrows to the same request lifetime; this preserves both caller-provided cache keys and per-chat affinity without leaking borrowed config strings.

**Native chat cache continuity:** individual UI chat messages continue to use separate durable Rust Runs, but those Runs now retain the same hashed UI-session routing affinity through persisted `provider_session_affinity`. OpenCode Go uses the scoped `x-opencode-session` header; the official OpenAI endpoint uses that stable conversation identity as `prompt_cache_key` unless the operator explicitly configured a key. Other gateways receive neither vendor-specific field. Tests cover two consecutive (including cancelled) UI Runs, restart serialization and exact-host isolation.

**2026 Sub4 session and memory integration:** first-party OpenCode Go receives a stable `x-opencode-session` identifier scoped to the durable Rust Run UUID or the room UUID. No unrelated Runs share a branch-global ID; the header is never sent to lookalike or non-OpenCode hosts. The group prompt maintains its unchanged shared cacheable prefix while appending a bounded, source-indexed excerpt of the active speaker's latest **aged-out prior answer** as a private suffix. This avoids losing earlier self-consistency without copying every peer turn. The complete Room journal is unchanged. Group usage now exposes `selfRecallChars` separately from actual provider-reported cached/uncached tokens. Cache-hit performance must be measured upstream, not inferred from text sizes.

**Selective exact-source preservation:** stable rolling windows grant longer deterministic excerpts to indexed peer decision/evidence/failure statements than to general chatter, while keeping the original durable transcript unchanged. This guards the previously observed early failure/decision recall case after removing unnecessary 16-message cache resets. Tests check that the important quote survives under a tight 1,800-character room budget.

**Deferred group checkpointing (2026):** earlier versions reserved 38% of the context envelope for a non-existent checkpoint and discarded the append-only prefix every 16 messages, even if 30 short 9-speaker × 3-turn messages would fit. The first window now uses the available envelope and up to 48 append-only messages, leaving space for private and mutable suffixes. The 38% indexed-evidence reservation starts only after a real checkpoint is sealed; later windows stay bounded. This reduces forced cache resets without promising actual provider cache savings. Unit tests assert unmodified prefix across 30 short messages, and separately force rollovers to check own-reply recall and owner corrections.

**First-load-once provider continuation (Sub4):** for the exact official OpenAI Responses endpoint (or an explicitly selected `responses` protocol), each room participant gets an independent durable `previous_response_id` chain in `Room.remote_continuations`. On the **first** response, Dominion sends the shared room instructions, source-indexed history, and that speaker's developer-role persona; subsequent responses upload **only newly recorded owner/peer quotes and changed notes**, without repeating the common instructions, previous own answer, or persona. Reopening the Room resumes its serialized server response chain; selecting a different provider/model or editing the speaker/topic starts a clean full-context chain. The chain rebases after the configured context budget requires it. Mock Responses HTTP tests enforce both a one-speaker restart and **9 speakers × 3 owner turns = 27 provider completions / 30 transcript entries**, including 18 actual delta-only network requests. The Room usage ledger now separates `wirePromptChars`, `serverContinuationCalls`, `fullPromptCalls` from provider-reported billable `promptTokens` and `cachedPromptTokens`. **Important:** OpenAI documents that `previous_response_id` still bills previous chain input tokens; reduced HTTP payload is *not* 0 billable prompt tokens. Real savings require verified prompt-cache hits or a stateful local KV cache. Chat Completions / Anthropic / Ollama remain explicit full-prompt+cache adapters because they do not promise Responses-style remote continuation. Responses uses `store:true` so the operator should consider the provider's data-retention settings.

**Post-inference budget integrity:** if the first/next group model reply exceeds the owner token/cost budget, the provider call has still been billed. The durable `RoomUsageReceipt.accepted=false` ledger entry now captures that exact call; `rounds` and transcript messages do **not** gain a fabricated response. Both room-level and per-agent usage distinguish `deliveredCalls` and `billedUndeliveredCalls`. Old receipts default to accepted for backwards compatibility. An end-to-end fake-provider test verifies 100 billed tokens, 0 delivered answers and intact Room persistence.

**Group provider usage authenticity:** every successful speaker reply persists `provider_usage_reported` alongside its native input/output/cache counters. Room-level and per-agent usage expose `unmeteredCalls`, `providerUsageReported`, and `usageStatus`; an old journal receipt or missing upstream counters can no longer masquerade as zero-token inference. Mock-provider tests check 27 metered replies, observed cached/uncached totals and a legacy-unreported receipt. These are metering-path tests, not observed provider-side cache performance.

**Auto-learning cache isolation:** `learned_context` can change after every generated room reply. It now occupies a bounded, explicitly lower-trust suffix **after** the stable indexed owner evidence and append-only peer/owner transcript, preventing a changed note from rewriting the cached prefix. `selfRecallChars` and `mutableMemoryChars` in room usage describe context composition, not guessed provider cache hits. The true `cachedPromptTokens`/`uncachedPromptTokens` continue to come exclusively from the provider's usage receipt.

Room context is now an **append-only provider prompt within a bounded window**: a stable indexed checkpoint of prior owner decisions and evidence-labelled peer quotations comes first, followed by individual recent, chronologically appended utterances. Fixed per-utterance excerpts avoid invalidating prior cached message boundaries. **The entire shared room context precedes speaker-specific metadata**, allowing participants to reuse a common prompt prefix instead of invalidating it at the first persona token. First-party Anthropic receives bounded explicit shared-prefix breakpoints; incompatible providers never receive those fields. the checkpoint is recomputed only after its bounded rolling window fills. Owner corrections and saved notes stay labelled, original source indices remain traceable, and the complete conversation remains durably stored. **This is selective verbatim recall, not guaranteed lossless memory or a measured provider cache win.**

Provider usage records distinguish **gross input tokens**, **cached input tokens**, **uncached input tokens**, **cache-write tokens**, **output tokens** and configured **KRW cost**. Serialized-character reductions are labelled separately; they are *not* billed-token measurements. OpenAI-specific cache controls and first-party Anthropic automatic caching are endpoint-gated. Local Ollama usage is counted from the model's native response; a provider cache hit is never invented.

Read the [Sub4 context/MCP review](docs/SUB4_SINGLE_HARNESS_2026-11-18.md) and [source-based harness comparison](docs/SUB4_HARNESS_SURVEY_2026-11-18.md).

## Quick start

**Requirements:** current Rust toolchain; Node.js **24.12+** for the React frontend. The server defaults to `127.0.0.1:4317`. Linux development may work but the project release matrix targets **macOS and Windows**.

~~~bash
git clone --branch dominion-sub4 https://github.com/hushmind3/dominion-rust.git
cd dominion-rust

# Build the actual React assets that the native Rust server serves:
cd web && npm ci && npm run build && cd ..

# Choose your own working model and endpoint:
export DOMINION_BASE_URL="https://api.openai.com/v1"
export DOMINION_MODEL="YOUR_AVAILABLE_MODEL"
export DOMINION_API_KEY="YOUR_API_KEY"

cargo run -- --home . --open
~~~

Open **http://127.0.0.1:4317** or check `GET /api/v1/health`. For Vite development run `cd web && npm run dev` alongside the Rust server. `.env.example` is a reference configuration: **Rust does not automatically load a copied `.env` file**. Export environment variables through your OS, shell or service manager.

For an OpenAI-compatible local endpoint, set its actual address/model and omit the API key only when that endpoint accepts unauthenticated local requests. An Ollama-native adapter is also available through registered provider routing; do not assume every OpenAI-compatible gateway speaks native Ollama protocol.

### Selected configuration

| Environment variable | Meaning |
| --- | --- |
| `DOMINION_BIND`, `DOMINION_ACCESS_TOKEN` | Listen address; a token is required when binding outside loopback |
| `DOMINION_BASE_URL`, `DOMINION_MODEL`, `DOMINION_API_KEY` | Main model endpoint |
| `DOMINION_ECONOMY_*` | Optional explicitly selected economy-model settings |
| `DOMINION_DATA_DIR`, `DOMINION_WORKSPACE_DIR` | Durable records and isolated workspaces |
| `DOMINION_MAX_STEPS`, `DOMINION_CONTEXT_MAX_CHARS` | Run/model-input limits; context is measured in serialized **characters** |
| `DOMINION_ENABLE_SHELL`, `DOMINION_SHELL_SANDBOX` | Shell is off by default; required OS isolation fails closed when unavailable |
| `DOMINION_DENIED_TOOLS`, `DOMINION_MAX_PARALLEL_READS` | Operator policy and parallel read bound |
| `DOMINION_MCP_SERVERS` | Owner-configured Streamable HTTP MCP server list |
| `DOMINION_SEPARATE_PLAN_MODEL_CALL` | Optional extra plan-only request; default `false` |
| `DOMINION_SEPARATE_MODEL_AUDIT` | Optional independent model review; default `false` |
| `DOMINION_ENABLE_STRATEGY_TRIALS` | Approval-gated two-candidate comparison; default `false` |
| `DOMINION_STRATEGY_LEARNING` | Optional verified evidence reuse; default `false` |
| `DOMINION_PROMPT_CACHE_KEY`, `DOMINION_PROMPT_CACHE_TTL` | First-party OpenAI cache routing where supported |
| `DOMINION_*_KRW_PER_MTOK` | Owner-supplied token tariffs in Korean won per million tokens |

**Price note:** a zero or absent tariff does not establish that model requests are free. Fill in the actual provider contract to estimate KRW spend correctly. Provider-reported cache reads/writes and other input tokens have different charging rules.

## SDK and API

**Rust** — requests, tool decisions and persistence stay on the native runtime:

~~~rust
use dominion::{CreateRun, Dominion, Settings};

#[tokio::main]
async fn main() -> Result<(), String> {
    let sdk = Dominion::open(Settings::from_env()?).await?;
    let run = sdk.submit(CreateRun {
        instruction: "Inspect the repository and fix the failing test".into(),
        budget_krw: Some(1200.0),
        max_steps: Some(20),
    }).await?;
    println!("Run: {}", run.id);
    Ok(())
}
~~~

**TypeScript** — a thin client; it does **not** implement another agent loop:

~~~typescript
import { Dominion } from '@dominion/sdk';

const sdk = new Dominion({ baseUrl: 'http://127.0.0.1:4317' });
const run = await sdk.runs.create({
  instruction: 'Inspect files, fix the issue and report observed evidence',
  budget_krw: 1200,
});
for await (const event of sdk.events(run.id)) console.log(event.kind, event.message);
~~~

The native API includes `/api/v1/runs`, `/api/v1/groups`, `/api/v1/memory` and `/api/v1/money/*`. The existing React frontend uses compatible `/api/*` routes backed by the same Rust stores. Authenticated approval, cancellation, owner amendments and reconciliation are explicit operations; clients must not infer success from a queued or paused Run.

See the [Rust SDK contract](docs/RUST_SDK.md), [TypeScript SDK reference](sdk/typescript/README.md), and [frontend/API implementation notes](docs/FRONTEND_RUST_CONTRACT.md). Some older integration documents are dated snapshots; this README and actual source describe the current Sub4 branch.

## Money: evidence before spending

The optional **돈벌이 찾기** screen retains original source IDs, bounded excerpts and revisions, historical candidates and review decisions. It searches Korean/CJK/English material with indexed retrieval; extracted demand/supply, friction and price signals are **hypotheses**, not verified transactions.

Price comparison distinguishes asking/indicative/claimed transaction figures and requests compatible currency, dates, quantities, specifications and costs. Unknown values remain unknown, with no invented positive margin. An implementation approval reserves one native Run and binds the candidate to that Run before activation, preventing duplicate paid workers across retries.

The Money collector/automation is separate from autonomous agent execution and **off unless enabled by the operator**. No model is needed for ordinary archive searches; optional analysis consumes the chosen native model and its stated budget.

Read [Money integration](docs/SUB4_MONEY_INTEGRATION_2026-11-17.md).

## Validation and honest limits

**On every Sub4 source push:** [targeted development-profile Rust tests](.github/workflows/sub4-dev-rust.yml) run on macOS, including tool policy, crash states, group receipts, context, Money, SDK/Run and program regressions. A passed CI run validates those cases only.

**Manual release workflow:** [macOS/Windows checks](.github/workflows/check.yml) cover broader Rust, TypeScript/build and optional browser/public-MCP/soak checks. They are **not** part of the ordinary development push. Use:

~~~bash
cargo test --lib context::
cargo test --lib tool_discovery::tests
cargo test --lib money::
cd sdk/typescript && npm install && npm test
cd ../../web && npm ci && npm run typecheck && npm run build
~~~

**Known boundaries (do not treat them as shipping features):**

- **No built-in video/audio generation, YouTube publishing or autonomous RSI campaigns.** Operators may attach separate MCP capabilities. Existing legacy data files are not deleted.
- **No arbitrary stdio MCP installer or ACP external-agent host** in this native branch. Remote MCP is validated Streamable HTTP.
- **Not every frontend setting is supported.** Some per-room tool/reasoning options, custom verifier commands and extensions return explicit unsupported errors instead of pretending to work.
- **Desktop packaging is not release-ready:** `web/package.json` still refers to absent `scripts/prepare-desktop.mjs` and `scripts/build-desktop.mjs`. The actual React web build works independently of these advertised desktop helpers.
- **No proven frontier parity, real-model long-run recall win, latency reduction or paid-token saving** from mock tests or static source inspection. Compare the same tasks, models, tools and budgets before making performance claims.

The code and data model are published under **Apache-2.0** as declared in `Cargo.toml`. Developer review details, priority gaps and verified scope are recorded in the [2026-11-18 code audit](docs/SUB4_CODE_AUDIT_2026-11-18.md).
