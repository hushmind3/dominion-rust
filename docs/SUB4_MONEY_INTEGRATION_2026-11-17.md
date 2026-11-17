# Sub4 Money integration (2026-11-17)

## Source of integration
Actual comparison: money branch src/engine/money_intelligence.rs, money_mechanism.rs, money_strategy.rs, money_sources.rs, and src/api/money_find.rs. Market classification, economic pairing operators, source rotation and one-action discovery were adapted into Sub4.

The money branch's old framework, separate job/DB layer, existing UI, retired media/RSI and provider loops were NOT merged. Native Engine, journal, OAuth, Rust SDK and React design remain authoritative.

## Durable flow
1. RSS/Atom intake: compiled-in 23 public sources across Korean, Chinese, Japanese and Western languages. Three feeds per scan. No arbitrary URL fetch, automatic redirect, model-token expenditure or external action.
2. SQLite money.sqlite records source IDs/links, title, publisher, observed tags and capture/publication fields. Writes are transactional and source IDs deduplicated.
3. A bounded query of 180 recent source rows performs cross-sector economic tension matching. Independent publisher domains and change roles constrain two-source pairs. Single-source news is labeled a weaker research hypothesis, not proof of actual demand.
4. Three selected leads and report metadata save in one transaction. Prior rejected opportunities retain rejection. Each card shows buyer hypothesis, revenue mechanism, implementation, first paid test, falsification condition and original evidence links.
5. Optional model analysis is exactly one tool-free native Engine Run. API calls share the same provider routing, durable events, model accounting and restart non-replay policy. Generated ideas must reuse captured source IDs; unverified claims are not promoted to sales.
6. Thirty-minute collector mode is explicit opt-in, persists the preference, and contains no automatic paid inference. Business implementation remains a separate user decision.

## API and SDK
GET /api/money/find — latest saved report and native inference status; POST /api/money/find — mode db or agent
GET /api/money/stats — database counts and scan metadata; POST /api/money/collect — manual source scan; POST /api/money/automation — enabled boolean
POST /api/money/candidates/{id}/reject — reject a saved lead; POST /api/money/inference/cancel — cancel native model call
All endpoints also available under /api/v1/money/*. TypeScript sdk.money wraps native routes; Rust SDK exposes money_latest, money_signals, and money_collect.

## Current scope and gaps
Unlike a literal full-port, this does not yet include Money's buyer-document excerpt crawler, GitHub/Hugging Face metric collector, Korean bigram/FTS5 archive search, long-running paid inference campaign scheduler, actual payment ledger, or full profit/conversion experiment tracking. It records RSS summaries with lower evidence confidence. The observed source and economic inference are never claimed to be a verified sale.

Testing scope: Rust parser/semantic matching and durable SQLite tests; TypeScript authenticated SDK contract tests. Latest-HEAD CI outcome is a separate artifact from this code change, and 27B 24-hour throughput has not been measured.

## Public software metrics added after the initial integration

A six-hour GitHub/Hugging Face collector now saves public repository/model listings to the same source corpus, plus deduplicated daily star/download/like metric snapshots in `money_metrics`. Metrics are **platform attention observations**, not buyer commitments or revenue. The normal scan endpoint runs RSS and these two fixed-origin APIs and reports their failures separately. Each of the two public collectors also has its own durable bucket, so repeated scheduling does not trigger repeat requests. URL redirects are disabled and payload sizes are capped.

### SDK parity

Rust SDK money_latest now uses the same idempotent native model-result finalizer as /api/money/find. Rust SDK money_collect and the REST collection endpoint both use bounded RSS and public catalog sources, with partial network errors retained in the response. First-time find uses both collectors when the evidence archive is empty.

## 2026-11-17 removable Money tool and user approval

The Money candidate card now exposes an explicit **이 후보 구현 승인** action, with agent and user-editable KRW execution budget. The adapter does not treat displaying or rejecting a lead as approval. An approval creates a normal native tool-agent Run; it does not turn the Money model-only inference into an autonomous secondary runtime. The candidate retains the returned Run ID, and the UI uses the existing /api/v1/runs/{id} for progress and cancellation and links to Activity for detailed native approvals. Duplicate user requests reuse the same persisted Run, even after server restart; a missing or malformed original receipt blocks replay. Source-originated candidate text is passed as untrusted JSON for inspection, not privileged instructions, verified sales, or an automatically authorized publication.

Mac/Windows CI has been reorganized to run each platform's Rust checks, plus one macOS web/TypeScript pass. Browser, live MCP and soak tests are opt-in extended workflows rather than repeating heavy Linux jobs on each push. This is source-level coverage; real model performance and true commercial sales are not inferred from build success.

## Release-only build gate

The Windows/macOS platform workflow and manual smoke workflow are `workflow_dispatch` only. Ordinary `dominion-sub4` pushes do not trigger build/check, web/frontend, browser, soak or integration workflows. This follows the owner's instruction to defer release validation. Source-only changes are not reported as passing platform tests.

### Crash-safe cross-store implementation approval

The owner-approved implementation flow now reserves a paused native Run, updates the existing candidate's Run ID (requiring an actual affected SQLite row), and only then launches the common harness. A crash before the candidate write never auto-starts the orphaned Run; a crash after that write allows re-entry through the same approved ID. The candidate's native execution Run remains retrievable independently through the standard SDK and HTTP API. The UI shows original native run status, cost and a resume action, not a separate Money worker or billing loop. This is source code behavior awaiting relevant development-profile test observation.

### Idle-path and concurrent-approval isolation

Unopened Money installations no longer create their SQLite file on every Dominion SDK startup; the scheduler first checks for an existing Money data file. The first user scan/collection or explicit enable action creates the file as before. The cross-store implementation claim now uses a single SQLite IMMEDIATE transaction to enforce an unchanged, non-rejected candidate with no previous implementation receipt. An in-process lock is still used for the user-facing selection path, while SQLite enforces the candidate reference against concurrent claims across DB connections. The corresponding Rust unit tests cover a missing candidate, duplicate claim and untouched idle initialization.

### 2026-11-18 judge-driven evidence and causal reasoning improvements

The 2026-11-17 assessment rated Sub4's Money capability 9/15 because it lacked earlier Money history indexing/cross-source recall. This branch now adds `src/money/retrieval.rs` with same-database FTS5, East Asian bigram search, indexed economic-role facets, bounded archive recall (120 source observations), publisher caps and an explicit zero-token source search API/SDK/UI. It also uses the existing `operators::panel_directive` in one native model invocation, feeding a small history of rejected ideas and requesting a causal chain, alternatives, buyer acquisition and falsifying evidence. The source-citation parser distinguishes independently hosted evidence from single-origin hypotheses while explicitly refusing to call either evidence of paying customers. The existing Money implementation approval and Rust execution kernel remain unchanged.

Sub2/Sub3 reference locations: `src/engine/money/retrieval.rs` (Sub2), `src/engine/money_retrieval.rs` (Sub3), `src/api/money/implement.rs` (Sub2). Only relevant indexed recall and evidence-selection concepts were adapted, not their execution runtime, schemas or scheduling model. Exact score gains and performance improvements are not measured or presumed. Relevant Rust development-profile regressions, rather than build/release/platform tests, are the intended check scope.
