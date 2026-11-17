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

## November 17 — audit-driven runtime and GitHub connection improvements

- **Run persistence:** per-Run write gates keep fsync-backed atomic persistence, with no global Run-map mutex held during file writes. Durability and uncertain-effect safeguards remain.
- **GitHub workflow:** connected repository permission is requested separately from identity login. An authenticated native tool can list repositories, read an exact file+branch SHA, request approval for a file commit and verify the remote result with a fresh GitHub GET. Unknown remote write outcomes stop for reconciliation, not blind replay.
- **Google/MCP boundary:** Google identity OAuth is not Drive/Docs access; MCP retains independent per-resource tokens. OAuth app Client ID/Secret configuration is still required.
- **Regression:** same-Run serialization, independent-Run updates, permission separation, mock GitHub commit+readback and missing-approval protection.
- **Scope:** the source and tests, with limitations and unmeasured claims, are described in [Sub4 five-branch audit response](docs/SUB4_AUDIT_RESPONSE_2026-11-17.md).

## Main frontend integration (2026-11-17)

The `dominion-sub4` web frontend now adopts the **main branch at `a026ce8e237fde2272b92cf929c7a944337f1d38`** for the persistent chat/group workspace, conversation settings, execution-permission controls, settings overlay, rooms layout, connection tabs, memory views, CSS and Korean/English translations. Fifteen source files were merged without replacing the Rust SDK, backend or Run storage.

Sub4-specific endpoints and UI are preserved: the operator-approved main SQLite import, archived history and memory promotion panels, GitHub repository-permission OAuth action, precise SSE invalidation, backups, and Playwright/native tests. Files imported directly from main are tracked by the frontend sync commit; `App.tsx` and `Connections.tsx` include small compatibility patches.

The main frontend's older research/learning endpoints (e.g. `/api/synergy`, `/api/sessions/:id/learn`) remain outside the sub4 native backend contract. The memory growth panel is mounted only if opened, so an idle memory page does not query retired research services. No product capability is claimed until the native backend supports it.

The follow-up main UI merge adds scoped SSE event routing, inactive-view request gating, a faster model explorer, and native Rust `job_event` compatibility. The dedicated Node test suite is executed in the web CI job.

## Connected GitHub and Google account work

The main UI **Connections → Accounts** offers one-click GitHub and Google browser consent using centrally registered product OAuth apps (not user-entered client credentials). GitHub repository access supplies files, branches, issues, comments and pull requests. Google Workspace access supplies Drive search/text/folder tools, Docs reading/creation/revision-checked append and Sheets creation/range read/update/append.

The native `src/connected_tools.rs` broker advertises tools only when the encrypted vault has the grant. React and TypeScript SDK share the Rust engine. **Every remote mutation requires user approval**, persisted intent and verified remote read-back. Unknown write outcomes enter reconciliation, never blind retry.

A product operator registers OAuth apps once and installs their ID/secret as server deployment variables; end users enter neither. A local GitHub CLI import is also supported for people already signed in to `gh`; if no product OAuth app is configured but the CLI is installed, the same GitHub login button launches its official web/device authorization and imports the resulting account. Google Drive-wide access may require OAuth app verification. No provider-issued identity tokens or client secrets are bundled. See [Connected Accounts](docs/CONNECTED_ACCOUNTS_2026-11-17.md).

## Native run change feed and job resume

Native Store fsync receipts emit per-Run update notifications. The chat SSE transport and browser-independent session finalizer subscribe once before inspecting current state. Tool-free chat now uses the native Engine worker/cancellation and budget ledger rather than a second frontend model executor. Idle tasks no longer read complete Run/model transcripts every 250–350 ms. Lagged broadcasts trigger authoritative snapshot re-reads. The legacy jobs resume endpoint now invokes the native engine's guarded resume instead of returning HTTP 501.

See [Sub4 Runtime Review](docs/SUB4_RUNTIME_REVIEW_2026-11-17.md).

## Reproducible coding-task evaluation

The optional `cargo run --bin dominion_coding_eval -- --manifest coding-tasks.json`
runner measures real-model coding tasks in isolated Git worktrees. It requires
actual changed source paths and independently executed verifier commands; it
records real token and KRW estimates (marking unknown tariffs), approval gates,
timeouts and completion status. Mock-provider regression tests are not counted
as actual frontier-model performance.

See [Live coding benchmark](docs/SUB4_LIVE_CODING_BENCHMARK.md).

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


## Money research (2026 Sub4)

The **돈벌이 찾기** menu uses the existing Rust execution server and TypeScript SDK. An independently persisted SQLite source archive collects bounded multilingual market RSS and public GitHub/Hugging Face metrics (no model tokens), classifies changes, and pairs independent supply/buyer observations into **unverified commercial hypotheses**. A selectable AI agent may perform one extra evidence-bound model-only native Run; provider routing, cancellation, charging, and durable journal are shared with Chat/SDK. Optional half-hour source scans and six-hour catalog scans are off by default. Source mentions and stars/downloads are not proven paying customers. Money never automatically spends business capital or launches implementation. API: `/api/money/*` and `/api/v1/money/*`; details: `docs/SUB4_MONEY_INTEGRATION_2026-11-17.md`.

## Money as a removable tool — shared native execution (2026-11-17)

Money is **one optional research tool**, not a competing model engine or scheduler. `src/money/` owns only bounded public evidence collection, lexical/causal candidate hypotheses, a namespaced `money.sqlite` data file under the existing `DOMINION_DATA_DIR`, and presentation/API adapters. The shared `src/sdk.rs`, `src/engine.rs`, `src/journal.rs`, and `src/store.rs` remain the **only** model-and-tool execution path; existing MCP and connected tools are reused rather than reimplemented. `money.sqlite` is a domain-data file, not a separate database server or second Run journal, and can be retained or exported independently.

Selecting **이 후보 구현 승인** in the Money page starts an ordinary native Rust Run using the selected agent and user-visible KRW model budget. Candidate JSON is labelled untrusted, and the instruction is to build locally, verify evidence, and request **separate approval** for shell/connected writes, publication, outreach, or spending. The persisted `implementationRunId` prevents repeated clicks from launching duplicate workers; the existing `/api/v1/runs/{id}` status/cancel/approval contract drives progress. Money can be removed by unregistering its two HTTP route merges, SDK startup collector registration, three SDK convenience calls, and the frontend route/nav entry, without modifying the execution kernel or migrating user Run records.

Sub4 compares the current main's `src/engine/money_tool.rs` (thin shared-job dispatch) with Qwen Code's core/extension split and OpenHands' common SDK/agent/workspace pattern. The principle is to keep one execution policy and journal while letting Money supply data and a bounded task brief. Real-provider business conversion and Windows/macOS task-completion benchmarks are not established by source code alone. Release CI targets macOS and Windows by manual workflow_dispatch only. No push-triggered build/test jobs run during ordinary development. Browser, public MCP and soak tests remain optional release checks.

## 2026-11-17 developer-mode verification and Money handoff

Ordinary development uses **targeted Rust tests in the development profile**. No separate `cargo build`/`cargo check`, release build, frontend build/E2E, soak or broad platform matrix is part of the ordinary push path. Manual release workflows keep Windows/macOS packaging and full validation separate; Linux builds and tests are not scheduled. A targeted macOS developer Rust test workflow provides source-specific regressions (the local editing container has no Rust toolchain). Passing CI or production readiness is not claimed without an observed run.

Money remains a removable domain tool. Approval now uses the native SDK to persist an **inert reserved Run**, commit exactly one Money candidate→Run reference, then launch through the shared harness. Restart before the reference is saved leaves an inert Run, rather than silently executing a business implementation; retry after the reference is committed reuses the identical Run. When a previously running Run pauses, the UI resumes that same Run through the ordinary native `/api/v1/runs/{id}/resume` endpoint with journal/approval checks. Market research never authorizes publication, customer outreach or spending.

## 2026-11-18 Money retrieval and inference upgrade

The Money tool now accumulates observations in its existing `money.sqlite` domain file with a same-file FTS5 title/excerpt index, Korean/CJK character-bigram recall and indexed topic/role facets. New observations update the index in the **same SQLite transaction** as evidence insertion; pre-existing observations are backfilled in 256-row transactions at first user search and the migration completion receipt avoids repeated full-corpus scans. The normal Rust SDK boot still does not run Money queries or spawn its optional collector unless its persisted setting is enabled. This is a narrow tool-specific index, not another engine/database server or financial time-series model.

Money discovery no longer selects only the latest 180 headlines. It selects **at most 120** source observations across buyer/shortage/cost-rise/supply/launch/policy roles, plus bounded indexed recall from recent seeds; existing URL/origin and rejection guards still apply. The user-facing search is available through `GET /api/money/search?q=...` and `GET /api/v1/money/search?q=...`, the Rust SDK's `money_search`, TypeScript `sdk.money.search` and the existing Money page. No model token is spent on archived search. Optional AI inference uses one shared model-only Run, a concise strategy portfolio, previously rejected lead examples and a causal/alternatives/buyer-access JSON contract; citations must map back to archived source IDs. Two different publisher hosts remain *independent source mentions*, not evidence of payments or causal success. The normal implementation approval still uses the shared Rust tool-agent Run and native receipts.

Compared with the current `dominion-sub2` and `dominion-sub3` Money retrieval modules, Sub4 now adopts the useful principles of indexed multilingual recall, bounded evidence diversity and token-free historical context, without transplanting their legacy Jobs runtime or maintaining two schedulers. Runtime performance, real buyer conversion and 2026 frontier task-score improvements require measured verification; source-level changes alone are not reported as a scored improvement. The development Rust test workflow is separate from release-only macOS/Windows builds and frontend/browser suites.

## Optional Money tool configuration and retained candidate review (2026-11-18)

Money is now explicitly enabled from its existing settings page, independent of its 30-minute public-feed collector. A clean installation with no `money.sqlite` is disabled and opening the settings page does not create the database. Previously used Money installations stay enabled until the operator changes the setting. Turning Money off disables further finding, public collection, research and candidate implementation, while leaving existing evidence, native Runs, user decisions and file data untouched. The source journal is not a second harness, and work that was already explicitly approved remains governed by the native Run cancel/reconcile policy.

Instead of silently discarding everything outside the three surfaced opportunities, bounded discovery stores **up to 24** source-backed candidate records per invocation, including lower-ranked hypotheses, in the existing Money SQLite file. The Money screen and Rust/TypeScript SDKs expose archived candidates, search/filter/paging and durable user decisions (hold, restore and reject, with optional reasons). Stored decisions survive subsequent rediscovery and preserve up to 32 changes per candidate; previously rejected/held ideas are not presented as fresh findings. Explicit approval still creates an ordinary native tool-agent Run. Original bounded RSS entry XML is retained under `feedOriginal` when supplied; it is labelled as the **RSS source entry, not the full linked article**. The original article URL remains available for the operator to inspect.

Paths: `src/money/mod.rs` (persistent module config, decision transactions, archive), `src/money/api.rs` (thin handlers and common SDK), `src/money/sources.rs` (RSS source payload), `web/src/pages/Money.tsx` (settings, archive review), `web/src/components/WorkspaceShell.tsx` (latest-main navigation placement), and `sdk/typescript/src/index.ts` (public contract). The rest of the platform's tool engine, model routing, profile, chat, workspace, approval and recovery code remain unchanged. Like Qwen Code's 2026 core/adapter separation and OpenHands' SDK ownership, the boundary is a domain adapter over a common execution system rather than a Money-specific runtime.

Verification: associated Rust development-profile tests are scoped to Money, Run reservation/recovery and native workspace regression. **No release/cargo build/check, frontend build/test or OS-wide platform matrix was executed for this change.** No validated increase in business revenue, live-agent task-completion rate or token savings is claimed from the source diff alone.
