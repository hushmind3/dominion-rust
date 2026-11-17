# Dominion sub4 — current harness architecture (November 17, 2026)

## Scope

Dominion's active execution loop is native Rust. Neither the web UI nor TypeScript SDK owns a second decision engine. No separate generation-based recursive self-improvement process is started during boot, chat, or ordinary coding runs. Video/audio generation and publishing are external tool responsibilities via MCP, not built-in worker subsystems.

```
User / Chat / TypeScript SDK / HTTP
                  |
           src/sdk.rs + src/api.rs
                  |
         src/engine.rs (one Run)
          /      |       \
   Provider    Tool/MCP   Delegate
       \         |       /
    Durable Journal + Store (fsync)
                  |
       Evidence-based verification
                  |
          Run status + artifacts
                  |
    Broadcast event → React SSE
```

## Real execution path

1. A request creates a `Run` in `src/domain.rs`. `Store::put` writes the JSON record with fsync before it becomes visible.
2. `Engine::launch` is the sole work scheduler. It uses an unbounded wake channel and active-task notifications; no four-worker 400ms full-job polling loop exists in this branch. The implementation is split into `src/engine.rs` (main drive), `src/engine/delegation.rs` (child work and budget settlement), and `src/engine/tests.rs` (native regressions).
3. `Engine::drive` plans with the pinned root instruction and direct user amendments. `context::compact_strict` reduces old conversation/tool receipt material without silently dropping new user instructions.
4. `tool_schema`, `policy`, `hooks` and `loop_guard` check each requested operation before dispatch. Two identical consecutive failed calls lead to a corrective rejection; repeated uncorrected loops pause instead of continuing work blindly.
5. Read-only tools may run in a bounded, model-ordered parallel batch. Mutations and external MCP calls remain exclusive barriers. Shell, MCP and merges need explicit approval unless an operator policy explicitly permits them.
6. A pending side effect is journaled as `in_flight` **before** dispatch. After execution, one durable result receipt and its correlation ID settle the intent. Interrupted uncertain effects are never speculatively replayed; manual reconciliation is necessary.
7. `verification::verifier_messages` builds a bounded audit input from exact user requirements, current workspace/Git observations and real tool receipts. The verifier requires an unambiguous `PASS` to finish the Run; a correction returns to the execution loop.
8. After each committed Store update, the broadcast channel emits one small `RunUpdate`. `/api/events` forwards these updates by SSE. A lagged client receives a resync indication. Idle tabs do **not** clone the entire stored job history every 1.5 seconds.
9. The frontend and SDK read the same native `Run` and memory records. Planning checks only direct-child existence; delegation limits use direct-child snapshots; settlement reads one consistent two-level family snapshot rather than cloning every historical Run. Explicit scheduled tasks remain available via `src/automations.rs`; retired RSI campaigns are not scheduled.

## Data ownership and migration

- `src/store.rs`: durable native Run metadata, messages, planned tool calls, receipts, token/cost measurements and status. Startup diffs the semantic recovered Run against the loaded record and fsyncs only changed snapshots; unrelated historical records are not rewritten. `recovery_candidates` returns only paused/safe Run IDs instead of cloning every stored transcript into the SDK on restart.
- `src/knowledge.rs`: user-managed notes, corrections and reusable procedures.
- `src/frontend_state.rs`: chat sessions, agents and user-visible messages.
- `src/group.rs`: group requests.
- `src/automations.rs`: **user-created** recurring work only.
- `src/workflow.rs`: explicit workflow jobs.
- `src/frontend_system.rs`: encrypted backup/restore and maintenance.

The removed video/RSI execution modules are no longer compiled or initialized. Existing files in `DOMINION_DATA_DIR` are **not** deleted or rewritten as a side effect of module removal. A separate, reviewed migration would be required to export historical retired-format records for new UI viewing. Native chat, Run and memory stores retain their ordinary on-disk formats.

## Main SQLite read-only compatibility boundary

Main's `records(kind,id,data)` and `messages` tables are read through a **query-only SQLite snapshot**, never opened for schema updates or writes. `src/legacy_import.rs` converts supported agents, active private sessions and messages into `FrontendStore`'s copy-on-write/fsync snapshot, preserving original message IDs and inert tool metadata. The same loader also previews supported provider records, which `Registry::import_disabled` persists with **no credentials and enabled=false**. Existing IDs are never replaced.

The copy is operator-triggered from the existing Operations/Settings page, the native Rust SDK, or the thin TypeScript adapter. There is no automatic boot migration, secret substitution or replay of old jobs. Incomplete migrations remain retriable because each target store skips existing IDs. The main DB and WAL remain independent and unchanged.

Archived/room/channel sessions, the old vault, memory and execution artifacts are **not imported by this path**. The 8 MiB frontend JSON cap remains and prevents a too-large copy from committing. A full migration needs explicit compatibility semantics and separate tests for those historical formats.

## Historical archive and memory promotion

`src/legacy_archive.rs` provides a separate, indexed SQLite archive populated from the main `dominion.sqlite` through `legacy_import::open_legacy` (query-only read snapshot). Only specifically enumerated nonsecret history categories enter the archive: memory, room, job, execution, operation, workflow, experience, strategy, session and message. It is not mounted into `Store<Run>` or the scheduler, preventing old tool intents and stale queued jobs from firing. Existing keys are updated only when source bytes/timestamps change. A successful re-import can refresh completed job receipts.

`Dominion::list_legacy_history` and `legacy_history_item` expose paginated, bounded owner-facing queries; the same native functions back the React Operations archive panel and typed TypeScript SDK. `promote_legacy_memory` is the **only** crossing into live `Knowledge`: it requires a specific archived memory ID and target scope plus an API confirmation. Deleted, archived and superseded memory records are refused. The original archive remains unchanged.

The historical archive is separate from the 8 MiB chat frontend snapshot, keeping large job transcripts out of the ordinary UI/session write path. It is not an automatic prompt memory cache; no redundant background scan runs at startup. The browser panel only fetches requested archive pages and selected record details.

## Frontend source and runtime contract

The React workspace was synchronized from main commit `a026ce8e237fde2272b92cf929c7a944337f1d38` into `dominion-sub4` while leaving the sub4 Rust engine, adapters, and databases intact. The main conversation layout retains mounted chat and group views when switching between them, with an overlay settings workspace. New components include `ConversationSettings`, `ExecutionPermissions`, `GrowthDashboard`, and `LearningRecords` along with source styles/locales.

The preserved sub4 `OperationsPage` continues to mount the `LegacyImportPanel` and `LegacyHistoryPanel`; the connected GitHub repository grant is reattached in the main `ConnectionsPage`. The main event shell emits typed `dominion-refresh` events so the native `useRemote` hook can skip unrelated query refreshes.

The historical/research panels are frontend-only where an API is not implemented; they must not be mistaken for live native functionality. Growth panels are lazy-mounted on user expansion to avoid idle background API requests. Browser tests confirm main chat/group navigation, settings, legacy import/history visibility and the repository authorization control against the native Rust API.

Follow-up frontend synchronization imports the latest main `web/src/refreshEvents.ts` scoped SSE router and its unit tests. Sub4 adds native post-fsync Run `job_event {id,status}` mapping so SDK, job and terminal session views update without forcing unrelated model/provider reloads. The `web` CI job runs `npm run test:refresh` before bundling.

## Provider and pricing boundary

`src/provider.rs` handles OpenAI-compatible chat completions and Anthropic Messages with selected models. A provider response reporting truncation or missing tool-call payload is not a final answer. The provider adapter applies a bounded 8MiB response limit. Only a definite HTTP 429 rejection is eligible for automatic retry/fallback: transport loss and HTTP 5xx may have already consumed billable tokens.

Configured KRW per-million-token prices are used for provider-reported usage; unknown third-party rates cannot establish a real zero cost. `policy::effective_budget_krw` reserves delegated child budgets and `Engine::account` durably records observed usage. These controls do not claim globally enforced exactly-once billing.

## SDK and API compatibility

- `Dominion::open(Settings)` in `src/sdk.rs`: native SDK entry and startup recovery.
- `src/api.rs`: native `/api/v1` entry including runs, approvals, reconciliation, skills, artifacts, groups and memory.
- `src/frontend_api.rs`: React UI `/api` adapter; no separate execution kernel.
- `sdk/typescript/src/index.ts`: typed, thin transport client.
- `web/src/App.tsx`: active Chat, Models, Agents, Groups, Connections, Memory, Operations and Developer navigation.
- `src/mcp.rs`: extensibility for external creative, search, code, data and other tools, with explicit consent and durable receipts.
- Video / RSI route families: retired and not advertised. Unknown native API calls return JSON errors.

## Validation and limits

Source-level changes were verified in the `dominion-sub4` branch, with new startup and child-family regression cases. GitHub Actions / live model success rates / elapsed-time and I/O deltas require separately observed runs. Correctness of migrated legacy data and Windows/macOS fsync semantics should be exercised on each target platform before declaring operational readiness.


Local and CI: `cargo check --all-targets`, `cargo test --all-targets`, TypeScript checks, Vite production bundle, browser Playwright against a real Rust service, external MCP interop, deterministic benchmark fixtures and 256-run crash-recovery/30-second continuous soak. Long-term success rate, cost and model quality need matched real-provider task sets.

The harness is an application runtime, not an OS security boundary. Approved shell tools require a properly configured sandbox. MCP remote side effects and provider spending require operator oversight; transcript evidence is not a substitute for remote reconciliation. Do not infer task success from model claims or omitted receipts.


## Money domain (Sub4-only)

The Money feature is a deliberately separate data/evidence module rooted at `src/money/`. Its `mod.rs` owns transactional SQLite source/candidate/metric snapshots. `sources.rs` and `catalog.rs` gather bounded allowlisted feeds and platform metadata. `intelligence.rs` labels lexical sector/change mentions without upgrading them to actual demand. `mechanism.rs` generates source-linked economic tension hypotheses. `api.rs` is a thin HTTP + native Run bridge; the only paid model path is the existing `Engine::launch` model-only worker and its native receipts. The existing main frontend and TypeScript SDK call those HTTP routes, with no alternate LLM service. Opt-in Rust SDK startup scheduler wakes at most every half hour, querying only the Money automation preference and never automatically invoking a model. The evidence DB does not share the Run journal's table or account credentials. See SUB4_MONEY_INTEGRATION_2026-11-17.md.

## Money tool implementation handoff (2026-11-17)

`MoneyPage` → `POST /api/money/candidates/{id}/implement` → `src/money/api.rs` bounded untrusted brief → existing `Dominion::prepare_run` and `Engine::launch` → `journal` and `Store` → existing native Run status/approval/cancel UI. No changes to the engine state machine or new Money worker are required. An `implementationRunId` is written into the already-persisted Money candidate before launch and is checked under the existing Money find/update guard on every request. A failed Money candidate update cancels the unlaunched native Run; missing historical receipts fail closed. The Money collector remains opt-in and does not launch billable inference automatically.

Domain evidence lives in `DOMINION_DATA_DIR/money.sqlite` and never becomes executable Run messages or approved commands without user action. Removing the Money route registration and menu does not modify the shared model registry, MCP, approval journal, or saved native Run records. The current main branch's `money_tool.rs` likewise uses a domain adapter over shared jobs rather than a second engine. See `docs/SUB4_MONEY_INTEGRATION_2026-11-17.md` for actual gaps and validation scope.

## 2026-11-17 platform CI findings

macOS filesystem aliases (/var versus /private/var) and Windows extended canonical paths (\\\\?\\C:\\...) exposed real defects in the common workspace layer. Source enumeration now normalizes the workspace root and rejects paths that cannot be stripped to a relative name, rather than accidentally returning an absolute path. Git worktree attachment converts a validated Windows canonical target to the regular argument spelling for Git; child patch application retains the Windows PATH/SystemRoot while isolating HOME. Unit-test cleanup releases the native router/SDK owners and retries transient Windows sharing violations. Money's opt-in scheduler now starts only when its preference is enabled; disabled SDK opens no longer spawn 30-minute Money background tasks. This change is common-core platform correctness, independent of the Money domain business heuristics. CI outcomes are reported separately from source changes.

## Development versus release validation policy (2026-11-17)

Per operator instruction, platform compile/build, frontend bundles/tests, soak and browser E2E are **release-only** and must not automatically run on branch pushes. `.github/workflows/check.yml` and `sub4-compile-smoke.yml` are both `workflow_dispatch` only. No build or test was invoked by the workflow policy change. Targeted native Rust development-profile tests are appropriate when requested and supported by the current environment; do not misrepresent source review as execution verification.

## 2026-11-17 Money approval crash consistency

The previous candidate implementation adapter persisted a native `Queued` Run *before* saving its Money SQLite reference. A crash in that interval could cause the shared startup recovery to restart an unlinked task. The new flow uses the same native SDK and engine, but reserves the Run in `Paused` state with a durable `reserved_unlaunched` event. The Money SQLite candidate is then updated with its exact Run ID; the UPDATE requires one matching row. Only after that domain receipt exists does `Engine::launch` activate the job. A restart before the domain receipt leaves a harmless unlaunched reservation; a restart after receipt but before activation can be resumed using the original Run. The shared Run journal continues to own every tool call, approval, cancellation and paid-model receipt.

No new Money agent runtime, Run worker, scheduler or database server was introduced. The existing domain-scoped `money.sqlite` file remains under the shared data directory and is separately removable/exportable. This is a bounded, non-atomic cross-store handoff with fail-closed crash behavior, not a claim of distributed transactions or exactly-once remote effects.

Design comparison: Qwen Code's [2026-09-27 managed runtime tool contract](https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-09-27-ordinary-host-managed-engine.md) separates admitted durable tool intent from execution receipt; Hermes' [tools/registry architecture](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/architecture.md) keeps domain-specific tools outside the core model loop; OpenHands' [SDK/Agent Server](https://github.com/OpenHands/software-agent-sdk) has its own shared execution boundary. Dominion applies the narrow ownership principle to its pre-existing Rust journal and does not import any of these projects' execution engines.

### Optional-tool idle initialization and approval concurrency

`Dominion::open` does not create an empty `money.sqlite` file when the Money tool has never been invoked. The optional scheduler returns before opening SQLite unless a Money DB already exists, and it starts no timer unless an existing persisted automation preference is enabled. Candidate implementation approval uses an SQLite IMMEDIATE transaction to compare the exact candidate snapshot, reject stale/deleted/rejected claims, and commit one candidate→Run reference; losing claims cancel their inert reserved Run. Native model/tool execution remains in the common Engine and Run journal. These changes require relevant Rust development-profile regression results before being counted as runtime proof.

## Money read-path separation (2026-11-18)

`src/money/retrieval.rs` is a removable domain-search module, not part of the common execution kernel. It owns FTS5 + Korean/CJK n-gram source lookup and SQLite facets of economic roles on the same Money data file. Writer: `money::save_signals` commits both source row and FTS/index facets within one transaction; previous row IDs are never reinserted into the virtual table. Reader: `money::retrieval::discovery` lazily indexes legacy rows in bounded transactions, then samples indexed buyer/supplier roles, bounded text matches and a recent fallback with per-publisher caps. Returning a ranked article does not verify its factual or economic claim. No background history scan is scheduled by SDK startup.

`money::api::find` calls this bounded reader rather than the 180-most-recent SQL reader; it still builds 3 candidate cards via the existing deterministic mechanism and user-rejection history. A single user-selected native model-only Run may then apply `operators::panel_directive` inside the same prompt. Inference stores causal chain, competing alternatives, buyer approach and source-host diversity labels only when the model cites an existing archived ID. These fields have no privileged execution authority. `money::api::search_archive` reads the same index using a bounded user query; React, Rust SDK and TypeScript SDK share it. `money::api::implement` stays a separate explicit human authorization into the common tool-agent harness and is not implied by reading or searching archived evidence.

No separate Money scheduler, provider worker, memory engine, Run journal or external transaction manager has been created. Data ownership remains: optional market evidence and hypotheses in `money.sqlite`, human-run tasks and receipts in `Store<Run>`, user-owned notes in `Knowledge`. Detaching Money removes only its menu/routes/optional scheduler registration and SDK conveniences, not the native core. SQL role/FTS retrieval has a one-time migration receipt; no background FTS rebuild on app boot.

## Removable Money module: explicit activation and full research decision lifecycle

Money remains a domain module at `src/money/` with no independent engine, job journal or model worker. The persisted `money_state.module_enabled` key controls optional activity separately from `money_state.automation`. Fresh empty installations start off without creating SQLite; the first explicit enable creates the existing domain file. A legacy Money file without this new key defaults on, preserving previous product behavior. HTTP, Rust SDK, TypeScript SDK and Money page use the same setting API. A native handler lock serializes disable/enable against Money finding, candidate decisions and implementation handoffs so newly disabled research cannot launch another paid Run after the disable operation commits. Existing native Runs retain their own journal/cancellation policy and are never silently destroyed by toggling this tool.

The domain candidate database keeps a bounded shortlist and a longer archive in the **same Money SQLite schema**; rediscovery uses INSERT OR IGNORE to preserve user-edited candidate states. `money::candidate_archive` filters persisted candidate status and user text, with bounded pagination, while `money::decide_candidate` writes each owner hold/restore/reject transition and reason in a single IMMEDIATE transaction. Prior approved native `implementationRunId` receipts cannot be overwritten by subsequent decisions. All stored RSS source entries retain source names, dates, original URL and a bounded raw feed payload; archived source text is explicitly unverified, and a linked article remains an external original URL unless fetched through an existing permitted tool.

End-to-end: React Money toggle → `GET/POST /api[/v1]/money/settings` → Money SQLite state; enabled search → indexed read-only research tools → 3 ranked cards + persistent archive; review and decision → `GET /money/candidates` and `POST /money/candidates/{id}/decision` → transactional decision history; explicit implementation approval → existing `Dominion::reserve_unlaunched_run` → SQL candidate receipt → `Engine::launch` → common policy/journal/tool receipts → native Activity page. This adds no second model scheduler, no independent DB server and no new core approval authority. Development profile tests cover data retention, module idle state and archive transitions; production and model-cost measurements are separate future evidence, not source-level claims.

## 2026-11-18 Common harness context and admission invariants

`context::compact_strict` is a model-request-only projection of immutable durable `Run.messages`. If the full serialized message list fits `DOMINION_CONTEXT_MAX_CHARS`, return it exactly, without the old unconditional 16-group truncation. If not, preserve every original direct user change verbatim (or fail before an expensive provider call) and pin the most recent verifier repair request as bounded **advisory** feedback; then use atomic assistant-call/tool-receipt exchange groups for the remaining budget. This prevents the verification→repair loop from losing its latest actionable failure while keeping source receipts in the durable journal. Tool-sourced content never becomes a user instruction.

The native engine discovers skills at first execution turn, and subsequently uses the preexisting `list_skills`/`load_skill` dynamic tools rather than rescanning directories and extension metadata for every tool receipt. The engine still retrieves up-to-date scoped user memory as before.

`policy::admitted_tool` unifies read-only and operator-deny admission. Model catalog selection, parallel-read prefix barriers, normal queued calls and immediately-before-dispatch checks on previously approved calls use that policy. A denied call is finished as a correlated durable error without triggering the shell, MCP, connected account or native filesystem executor; permission retraction on process restart is respected. These hooks do not create a new worker, policy engine or DB state.

Benchmark scope: matching request/model/tools/budget and captured model/provider token usage, number of context insertions, repeated skills directory reads, completed artifacts and recovery. Without those measurements, changes are correctness-oriented and not claimed as frontier superiority.

## 2026-11-18 Money source revision, syndication and pricing boundaries

Owner data path: configured RSS/catalog public inputs → bounded source parsing retaining RSS item snippet and original hash → `money::save_signals` SQLite transaction → `money_signal_revisions` (immutable content/metadata version by source id), `money_signal_stories` (same title link only), `money_signals` latest row + FTS5 reindex → `money::retrieval` bounded indexed selection → deterministic draft/candidate evidence and model-only optional inference → candidate archive → explicit approval into native Rust Run. No writes to other agent subsystems. The DB's latest source record can be refreshed without deleting a previous version; a timestamp-only rescan leaves the revision and index unchanged. Cross-source identical headlines are preserved, not marked as 2 independent first-hand reports.

Stored-field quote checks are deterministic and intentionally weaker than confirming the linked primary source. Candidate evidence includes revision ID, exact quote and a label saying what was matched. If a model cites a source not present in the provided shortlist it is rejected; if its claimed excerpt mismatches, the hypothesis remains in the archive with an explicit quote mismatch. Later source revisions do not rewrite an older candidate's frozen evidence snapshot. The optional source-history endpoint reads version lineage and related same-title URLs, and the React panel shows both.

Optional comparison: candidate review form → Money HTTP adapter or Rust/TypeScript SDK → pure `pricing::evaluate` (unit/spec/as-of/FX/full-cost gates) → same `money.sqlite` price-comparison receipt → candidate's last-comparison reference. A user-entered 'transaction' quoteKind is not verified payment evidence. An incomplete source quote, unknown tax or absent exchange rate yields unresolved evidence or a null comparison, not a fabricated rate/margin. There are no remote effects, purchases or paid inference as a consequence of comparing prices. Native Run journal/approval/cancellation remain exclusively shared.

## 2026-11-18 general native Run admission and revision-aware memory recall

**Generic two-store admission invariant.** The optional domain constructs a regular native Run via `Dominion::reserve_unlaunched_run` with status `Paused`, then commits the candidate↔Run association in its own durable store. A public `Engine::launch` and the SDK `resume` now fail closed if the Run has `reserved_unlaunched` but lacks `domain_receipt_committed`. Only after confirming the domain commit does its internal adapter call `Engine::activate_reserved`, which journals the exact nonempty domain/receipt identity and launches through the existing Run state machine. After a process crash: (A) before domain commit, an orphaned Run is inert; (B) after domain commit/before activation, explicit domain re-entry can activate the *same* Run; (C) after activation event/before dispatch, ordinary SDK resume is allowed because the Run journal already records the approval. At no point is an unlinked reservation silently auto-started. A missing or malformed domain association remains an explicit error rather than a reason to allocate a new Run. The kernel does not know the names or schema of any external tool or database.

**Per-Run durable-memory projection.** The unchanged scoped `Knowledge` store still holds notes in `knowledge.json` and writes them atomically. Its new in-process `AtomicU64` revision advances after committed remember/revise/forget only; a failed disk write cannot update the counter. The engine's small `focused_memory` map caches a string of the top four scoped memories keyed by Run ID, memory revision, repository scope, original task and the two latest genuine owner changes. Tool output and model messages cannot change this key or masquerade as a correction. One model turn reuses the cached body only while all keys match, and the cache entry is discarded at worker detachment. There is no database schema change, deferred memory writer, embedding model, independent cache worker or extra model call. A process restart initializes a new empty cache against persisted owner notes.

Verification scope: relevant Rust development-profile `money::`, reservation/recovery, focused memory, `knowledge::`, `context::`, `policy::` and workspace-regression tests. No release build, frontend test or multi-platform gate; no measured savings/benchmark advantage are claimed from the source diff alone.

## 2026-11-18 failure-specific recovery signals and provenance-based evaluation

`Engine::drive` uses the existing durable Run timeline as its single signal source. After a model turn yields a failed tool or verifier correction, `repair_guidance::advise` examines only subsequent `tool_error`, `tool_admission_rejected`, `tool_policy_denied`, `tool_denied`, `tool_hook_denied`, `mcp_remote_outcome_unknown` or `replan` events since the most recent `model` event. Simple bounded deterministic pattern matching yields a fixed classifier ID and fixed safe next-action hint. The model is never given a tool's untrusted failure text as higher-priority instructions. The hint is inserted only if it fits the existing prompt cap and changes none of the already-persisted direct user instructions, tool admission, approval, in-flight journal or effect-reconciliation rules.

An accepted model turn records the applied strategy and triggering receipt sequence alongside that turn's input/output token counters in its *existing* Store transaction. The `repair_guidance::observations` read model ties selected strategy events to subsequent real tool receipts and verified completion, capped at 64 rows, with `temporal_association_not_causal_lift` explicitly stated. SDK and HTTP expose this passive projection for offline experiments, not automatic model selection. An actual causal comparison requires held-out cases and unchanged model, tools, task contracts and budgets; no unmeasured success uplift or real-model savings are asserted. This preserves the 2026 project directive to avoid old RSI-style continuous self-tuning systems.
