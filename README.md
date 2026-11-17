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

## November 18, 2026 — isolated strategy experiments (Sub4)

**Removed** the earlier hand-weighted `direct/diagnose/decompose/verify` scorer and automatic file-list probes. Scoring labels against tool error counts did not measure competing approaches.

A root agent attached to a Git repository can request the native `compare_strategies` tool with **two distinct approaches**, an explicit per-worker KRW budget and a model-step cap. An **owner approval** is required before any candidate starts. This is an on-demand tool, not a mandatory extra inference step.

- Both candidates share the original task, model/provider and base Git commit, but run in separate worktrees with separate durable Run/tool receipts and reserved budgets.
- The `strategy_candidate` capability restricts workers to private workspace reads/edits and Git inspection, plus individually approved shell actions; no nested agents, external-account mutations, MCP calls, project-memory writes or patch adoption.
- Parent cancellation reaches both workers, preserving uncertain in-flight effects for reconciliation. Parent comparison waits until both workers have stopped **and child bills have settled**.
- A candidate needs a real file-edit receipt **before its last independent verifier PASS**, with matching model/provider, repository and base commit.
- Candidate selection uses **Pareto dominance** on actual KRW charges, model steps and tool errors, rather than invented static weights. Cost/speed/error tradeoffs yield **no winner**. Automatic patch adoption remains disabled; `merge_subagent` still needs human approval.
- The existing `repair-observations` SDK/API surface includes durable launch, partial-dispatch and comparison evidence with child IDs.

**Limit:** Model-verifier PASS and an observed edit are not an independent held-out acceptance test. Results are provisional operational comparisons. No real-world completion-rate, quality, time or token improvement is claimed until controlled same-task, same-model, same-tools, same-budget tests are performed. Development-profile Rust tests cover the changed paths; release builds are deferred.

External source code examined, not merely README descriptions:

- Google Research Harness²: `harness2/core/recursion.py` and `harness2/core/selection.py` (probe stages, execution contrasts, selection).
- Airbnb Agent Harness Optimizer: `optimizers/prism/loop.py` and `framework/acceptance.py` (train/holdout evaluation, reliability and Pareto).
- DeepSeek Harness: `packages/core/tool-executor-ptc/src/ptc.ts` and `ptc-calls.ts` (nested tool execution and cancellation settlement).

## November 17, 2026 sub4 maintenance

- Boot-time recovery rewrites only semantically changed Run records, not every historical snapshot. Uncertain write outcomes remain paused until explicitly reconciled.
- Model-step child detection, delegation limits, parent settlement, and SDK restart selection no longer clone unrelated Run transcripts. Parent→child IDs now share the Run map's metadata lock, so direct-child existence checks avoid scanning historical Runs and two-level family lookups visit only relevant descendants. Index changes publish atomically with the fsync-backed Run snapshot, including reparenting and restart reconstruction; SDK startup selects resumable Run IDs only.
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

## 2026-11-18 Native agent completion and operational-policy hardening

The shared Rust execution kernel now preserves the **exact full transcript whenever it already fits** the configured model context. Previously, `context::compact_strict` still clipped replies and silently dropped history after sixteen exchange groups even when no compaction was required; that reduced grounding, especially for longer multi-file and code-validation jobs. Over-budget projections retain all exact user changes and now also pin the **latest verifier correction as bounded advisory feedback**, with an explicit omission marker for long feedback; the durable transcript is never modified. Tool-call/receipt group integrity is still enforced. No second summarizer model, permanent prompt cache or independent inference engine was added.

Native skill name discovery scans the workspace and managed extension file on the first model execution turn only. The existing `list_skills` and `load_skill` tools still provide on-demand live discovery later, avoiding repeated filesystem reads and name-list prompt overhead for each coding step.

Tool admission now uses the same run read-only/operator denylist check in **model-visible schemas, the parallel-read barrier, the queued-call dispatcher, and the final dispatch after resumed approval**. This closes the gap where a previously queued or previously approved tool could be executed after the operator disabled it in the next process configuration. Rejected calls receive durable correlated `OPERATOR_TOOL_DENIED` receipts rather than silently running. Existing tool journal, approval, cost and recovery ownership remain unchanged.

References inspected: Qwen Code's 2026 prompt/tool selection and 2026 multi-agent session design, NousResearch Hermes' context-compression and retry issues from 2026, OpenHands' SDK agent-state verification, and the latest `main`, `dominion-sub2` and `dominion-sub3` code (failure classification, SDK and agent execution). We took only the state-retention and minimal prompt-replay principles; no external implementation code is copied into Dominion.

Tests: relevant Rust development-profile `context::`, `policy::`, Money, native reservation/recovery and workspace checks. This is a source-level remediation until the corresponding development CI completes; real coding success rate, model token reduction and CPU savings require controlled before/after runs with identical models and tasks. No release/build/check, frontend build/tests, or full platform validation is added to ordinary pushes.

## 2026-11-18 Money provenance and comparable pricing

The optional Money domain now keeps **the originally observed bounded RSS entry plus its full-entry hash/character count/truncation status**. For each source URL the existing SQLite database records immutable content revisions by SHA-256, with date and publisher, and updates the latest indexed FTS/facet snapshot only when content actually changes. The old latest-180-title filter that dropped separate-source reports was removed. Same-normalized-title links are retained for comparison and **explicitly do not establish independent reporting, transactions, or market demand**. Legacy SQLite source rows can still be inspected as single known snapshots without claiming missing historical revisions were recovered. Research/model evidence includes a stable source revision ID and exact saved-field citation status; arbitrary model quotes that do not match are preserved with mismatch labels for human review. The prior 3-card shortlist, saved lower-ranked ideas, owner review, and common agent approval remain unchanged.

A new pure Rust `money::pricing` module provides optional, zero-token customer/supplier input comparison and audit history in the **same** Money SQLite file. It normalizes amounts to KRW using the owner's explicit currency rates and date, checks product/service spec, unit, quote quantity and minimum-order conditions, and requires all shipping, fees, tax, labor, return and other charges to be explicitly known (enter zero only if genuinely zero). If anything is missing or incompatible, `spreadKrw=null` with reasons; negative spreads are shown as negative, never clamped. Each submitted scenario is attached to a saved candidate with deterministic deduplication, source-ID/quote checks, price kind (asking/indicative/claimed transaction), and `tradeVerified=false`. Assumptions are never promoted to actual contract prices by arithmetic.

HTTP endpoints: `GET /api/v1/money/sources/{id}/history`, `GET /api/v1/money/candidates/{id}/prices`, `POST /api/v1/money/candidates/{id}/price-compare`. Rust SDK: `money_source_history`, `money_compare_price`, `money_price_history`. TypeScript SDK: `money.sourceHistory`, `money.comparePrice`, `money.priceHistory`. The existing Money page offers expandable original revisions and same-headline links and an opt-in per-candidate KRW comparison form and durable review. No separate Money worker/model inference, scheduled price crawler, database server or marketplace API was introduced.

2026 reference patterns: [OpenHands Software Agent SDK's event/receipt semantics](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/base.py) and [Qwen Code's evidence-source and verification-oriented web search](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/web-search.ts). Latest main/Sub2/Sub3 Money source and implementation paths were compared. Runtime savings, real sales, independent factual corroboration and external model task-completion improvements are **not measured** and must not be claimed from code-only improvements. Ordinary push workflow runs only the relevant Rust development-profile tests; release, frontend and multi-OS validation remain deferred.

## 2026-11-18 general harness: domain receipt and focused memory correctness

After inspecting the current `dominion-sub4` Run lifecycle against `dominion-sub3`'s isolated Rust SDK, `dominion-sub2`'s common kernel and main's failure classifier, we hardened two **shared-kernel** paths rather than extending Money's separate business logic. External-domain approvals originally reserved a `Paused` native Run with a `reserved_unlaunched` event, then linked its ID inside a plugin's SQLite database. A generic SDK/HTTP `resume` could start the inert Run *before that link existed*. `Engine::launch` now rejects any still-unlinked reserved Run, regardless of the caller. A domain adapter that has persisted its candidate→Run reference uses `Engine::activate_reserved(id,domain,receipt)`: the common Store records `domain_receipt_committed` durably, **then** starts the existing Run. An interrupted reservation is therefore inert; an activation interrupted after its journal event remains resumable via the ordinary native SDK without a second paid worker. Money has been changed to this same generic adapter contract; its existing approval/worker IDs, UI, notes and data are preserved. The GUI first rechecks the Money receipt for a paused candidate, then uses the ordinary resume path only for an already-admitted Run.

Each native Run also now reuses its last scoped owner-memory recall while the query and in-process durable-memory revision remain unchanged. Before this change, each model turn repeated a full 1,024-entry-cap note scan and lowercasing even for identical instructions. `Knowledge::revision()` advances **only after** a successful persisted note insertion, edit or removal; the Run cache is invalidated by any such edit or direct owner correction, never by model/tool text, and is dropped at worker completion. This is an in-memory optimization, not another persistence file or always-on worker; the same notes are still presented as lower-priority reference material every model turn. CPU/latency savings are not assumed without controlled measurements.

Primary code paths: `src/engine.rs` (single native launch and bounded per-Run memory cache), `src/sdk.rs` (normal Run SDK unchanged; inert reservation tests), `src/knowledge.rs` (durable memory revision), `src/money/api.rs` (optional domain activation adapter), and `web/src/pages/Money.tsx` (owner receipt checked before resumption). This follows the ownership principles seen in OpenHands' versioned conversation state, Qwen Code's terminal tool-call events and Kimi Code's persisted plan identity; there is **no imported SDK, duplicate Run engine, model worker or second journal**. Development-profile Rust tests cover the new reservation and memory behavior; release, frontend and cross-platform test/build remain manual. See GitHub Actions for the observed pass/fail result of the matching commit.

## 2026-11-18 Sub4: receipt-guided recovery without a second agent

The existing native Rust Run loop now adds a **deterministic, evidence-scoped repair suggestion** after a failed real tool receipt or independent verifier correction. `src/repair_guidance.rs` classifies only persisted execution events **since the previous model receipt**: compiler diagnostics, named test/assertion failures, missing paths, invalid arguments, permission denials, ambiguous external tool outcomes and verification rework. The next model request receives a small, optional note pointing it toward a different action; it does not copy untrusted output as instructions, change the user's contract, retry calls or bypass approval. `loop_guard`, the original tool schemas, model routing, cancellation, durable Store and budget policy remain authoritative. No new planner, model invocation or RSI worker was introduced.

When the ordinary model turn accepts a repair hint, its existing Store mutation records `repair_strategy_applied` with the triggering event sequence, strategy ID and provider-reported token counts. `GET /api/v1/runs/{id}/repair-observations` (also `Dominion::repair_observations` and TypeScript `runs.repairObservations`) produces a **read-only 64-entry maximum** summary of later actual tool successes/failures and verified completion. This is **observational chronology only**, not a claim that the strategy caused success. For causal or token-efficiency results, compare identical tasks, model/tool/budget constraints, random seeds where supported, and disjoint holdout cases using the existing optional `dominion_coding_eval` runner; Airbnb's [Agent Harness Optimizer](https://github.com/airbnb/agent-harness-optimizer) provides a practical 2026 reference for held-out scoring and resource-bounded harness selection. Reuse existing Run journals rather than collecting another database or creating a continuous self-improvement loop.


## 2026-11-18 Sub4: native read-program, fused planning and external judge separation

**Default model-call path:** New native Runs no longer pay for a separate plan-only request. The first tool-capable model response does both planning and action. A durable `planning_fused` event records this; `plan=""` means no separate model-authored plan was generated. Restore legacy behavior only with `DOMINION_SEPARATE_PLAN_MODEL_CALL=true`.

**Separate judge:** A final response no longer triggers a duplicate model auditor by default (`DOMINION_SEPARATE_MODEL_AUDIT=false`). The Run stores the result and exact tool receipts and labels completion as *unaudited*, not an independent PASS. An operator may explicitly enable `DOMINION_SEPARATE_MODEL_AUDIT=true` to restore the original model correction loop. Isolated strategy-trial workers retain their earlier verifier contract. Expensive two-candidate experimentation is removed from the default model catalog (`DOMINION_ENABLE_STRATEGY_TRIALS=false`); legacy pending comparisons are denied at dispatch if disabled. It remains an opt-in, approval-gated and budgeted research feature.

**Native `read_program`:** A single ordinary model tool call schedules 1–12 ordered read-only steps in the Rust runtime. Steps support `list_files`, `read_file`, `search_files`, `git_status`, `git_diff`, optional conditional branching on the exact previous observation (`when.step`, `when.contains`, `when.not`), and bounded `read_listed_files` iteration over paths from a prior listing with suffix/needle filters. No model invocation occurs during the program. Each iteration is limited to eight files and the program has a total limit of 24 nested reads. Full argument admission, operator disabled tools, read-only policy, workspace boundary checks, cancellation, durable intent and crash-safe read replay use the existing native kernel. Each result retains its step ID, status, and compact evidence. The interpreter **cannot** execute nested writes, shell, remote MCP, account mutations or subagents; known local writes still use the original model multi-tool queue with individual write-ahead journals.

Example tool arguments (passed by the model to `read_program`):

```json
{
  "steps": [
    {"id": "files", "tool": "list_files", "arguments": {"path": "src"}},
    {"id": "evidence", "tool": "read_listed_files", "from": "files",
     "suffix": ".rs", "contains": "TODO", "max_files": 4},
    {"id": "focus", "tool": "search_files",
     "arguments": {"query": "read_program", "path": "src"},
     "when": {"step": "files", "contains": "engine.rs"}}
  ]
}
```

Rust SDK, HTTP/GUI and TypeScript SDK keep their existing Run API. Test `read_program_fuses_planning_and_skips_duplicate_model_audit` drives real temporary workspace files and a mock provider and asserts **two model requests** (program request + final answer), zero separate plan/auditor turns and one durable program receipt. Interpreter unit tests verify branching, iteration, policy denial and crash recovery. This is a protocol-level cost improvement, not live provider token/latency benchmarking or proof of frontier parity. The old plan+tool+final+auditor path remains in explicitly opted-in regression tests. Reference pattern: DeepSeek's Programmatic Tool Calling and OpenHands' durable agent event boundaries; this narrower Rust implementation is not a direct source copy. Release Windows/macOS and live-model comparisons remain independent.


### 2026 evidence-directed runtime extension (Sub4)

The independent Rust `read_program` interpreter has evolved from a sequential read chain into an **observation-typed, bounded partial-order execution kernel**. One model instruction produces a finite graph (up to 12 steps). The Rust scheduler computes the dependency-ready frontier and executes up to four independent inspection nodes concurrently, preserving declared order for returned receipts even when worker completion order differs. Model re-entry occurs only after the program emits its result. Each step may depend on preceding `after` nodes and typed `when.step/status/contains/not` observations; failed reads cannot satisfy positive or negated success predicates.

The graph can dynamically turn a prior `list_files` receipt into a bounded `read_listed_files` stage or a prior `search_files` receipt into `read_matched_files` (unique matching file paths, not one read per matching line). File bodies and step outcomes expose SHA-256 content identifiers, bounded excerpts and exact success/failure status. Optional `objective` clauses in `all`/`any` form short-circuit *only when actual prior observations satisfy their literal predicates*; model-authored assertions never count as a judge's grading or proof of task success. The kernel uses one existing durable read-only tool journal, enforces the current nested tool allowlist and operator denylist, caps nested reads at 24, and can replay the entire read-only program after interruption without duplicating side effects. A partially failed nested source inspection is a typed error, not a fabricated success.

The design is informed by 2026 programmatic tool execution, JIT agent harness synthesis, verified operational state and proof-carrying evidence practices, but is **not** copied source code from DeepSeek, Harness² or any other project. Its particular composition (typed observation guards, demand-driven graph frontier and bounded source-proof return) is Dominion-specific. It is currently **read-only**: tool writes, shell commands, connected accounts, Git merge, and other externally visible effects still go through the existing per-call write-ahead journal and human approval rules, not this interpreter. No external frontier ranking, complete programmatic side-effect interpreter, model-quality improvement or live-model token saving is claimed from a mock provider. Separate independent judge owns scoring.


### 2026-11-18 Sub4: selective composition rather than another harness

Sub4 now combines three **existing** mechanisms through one native Run execution path, rather than proposing another agent loop or evaluator:

1. **DeepSeek Harness PTC runtime pattern** (reviewed the actual 2026 `packages/core/tool-executor-ptc/src/ptc.ts`): one model-authored program tool request invokes multiple admitted tools and returns condensed results without intermediate model turns. Dominion uses the existing `read_program` tool for a bounded **read-only** graph, not the external TypeScript runtime or unreviewed nested write execution.
2. **Dominion main Program IR** (`main/src/engine/program.rs`): strict `{from,path}` dataflow and three-valued `exists/equals/compare/not/all/any` observation predicates now live in reusable `src/program_ir.rs` and are invoked by the Sub4 read program. Missing/null result paths fail closed instead of guessing values; `Unknown` cannot satisfy or negate a predicate into apparent success. Tool arguments are re-admitted against the same tool schema *after* resolving observed values. Dynamic bindings can feed a prior `search_files` match directly to a subsequent `read_file` path without another model request.
3. **Existing Sub4 Run, journal, policy and SDK**: `src/engine.rs`, `src/journal.rs`, `src/policy.rs`, `src/tools.rs` and `src/tool_schema.rs` remain the only execution, replay, approval and authorization owners. The evidence graph uses actual tool receipts, partial-order execution and SHA-256 content IDs; it is not a replacement for the common executor or the external judge. Nested writes/shell/MCP are explicitly not admitted.

Key development regressions: `program_ir::tests`, `read_program::tests`, and `read_program_fuses_planning_and_skips_duplicate_model_audit` (real temporary files, mock provider). These prove dataflow, typed predicates, the existing native journal integration and the absence of internal model calls for tested paths; they do not prove frontier benchmark superiority, general side-effectful program execution, or live-provider savings. The prior isolated two-worker experiment remains off by default.


### 2026-11-18 Sub4: PTC plus main IR plus the original journal for real edits

The model-facing `run_program` tool now schedules a bounded **mixed native-workspace operation program** (up to 12 steps) without introducing a second agent scheduler or tool dispatcher. Its design selects compatible advantages from the 2026 DeepSeek programmatic tool calling code path, Dominion main's typed `{from,path}` inputs / three-valued `exists/equals/compare/not/all/any` conditions, and Sub4's existing write-ahead Run journal, operator policy, approval gate and restart handling.

One model request submits the program by itself. A pure, crash-replay-safe `run_program` installation saves `program_session` as a backward-compatible nullable field on the **existing** `Run`, then the original `Engine::drive` enqueues each native step as an ordinary tool call. Every `write_file`, `edit_file`, `read_file`, `search_files`, Git read or explicitly enabled `shell` step follows the same `journal::begin` / tool execution / `journal::finish` ledger as an ordinary model request. Shell steps still require the ordinary explicit owner approval. MCP, connected accounts, memory writes, nested program runs, subagents and recursive strategies are rejected from this program contract. Read-only investigators are not allowed to run mixed-effect programs.

The program cursor is persisted **on the same Run** together with each planned call and observed receipt; a restored run can advance from a completed write without repeating the write, while an interrupted effectful step remains uncertain and needs ordinary reconciliation. An unreceipted program step is paused rather than replayed or polled. The operator denylist is rechecked before each actual step and a dynamically resolved `{from,path}` input must satisfy the exact original tool schema. Failed steps return to model decision by default; `on_error=continue` allows an explicit typed error-handling branch. Unknown predicates never silently take a false/default path. The `read_program` tool remains the bounded, optimized DAG for purely parallel inspections; `run_program` covers *effectful, dependency-ordered work* without another execution kernel.

For example, a single `run_program` can create `result.rs`, search the created source, bind the returned `search_files` file path into `read_file` and `edit_file`, then read the final artifact again. A mock-model + real-filesystem native Run integration regression verifies five real workspace tool operations with **two total model requests** (initial program and final response). Unit regressions check invalid/external calls, unknown predicates, durable cursor restoration and missing receipt fail-closed behavior. This confirms the protocol shape only; real-model token reductions, overall task-completion scores, external-agent capabilities and release packaging have **not** been measured by these tests. Separate independent judging remains independent.

Relevant files: `src/program_session.rs`, `src/engine.rs`, `src/domain.rs`, `src/journal.rs`, `src/tools.rs`, `src/program_ir.rs`, `src/engine/benchmark_tests.rs`.
