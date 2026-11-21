# DOMINION Rust

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
- **P3 remote native investigator:** `src/product/remote_worker.rs`, `src/addons/remote_worker.rs`, `src/ports.rs`. Opt-in scoped-token remote transport creates idempotent read-only native Runs on another Dominion instance. Both sides retain independent cost receipts; remote execution cannot modify files or recursively delegate. No new UI menu or independent execution loop.
- **Main user interface:** `web/` is synced to the current main React navigation and pages, including model exploration, accounts, memory, rooms and chat. Hugging Face public GGUF/MLX search, revision-pinned downloads, pause/resume/cancel, local file registration, installed-model removal and local OS pickers are bridged into the Rust API. Imported GGUF models run via the loopback Ollama adapter inside the same Rust Run orchestration; a native MLX execution driver, automatic model-engine installers and other main-backend-only integrations still require separate work.

## Native execution UX in the existing Activity screen (November 21, 2026)

The existing **Activity / Operations** view now connects directly to Sub4's Rust `/api/v1` contract. It includes an expandable owner-authored Task Team DAG composer (roles, dependencies, per-worker step/budget ceilings, optional repository), a paused draft with explicit start, live native Run status and usage, pending tool decisions, safe manual reconciliation of uncertain effects, worker navigation, and SHA-256-pinned patch preview/owner approval. No new top-level navigation or second execution engine is introduced.

The UI consumes the same durable Run receipts used by the Rust SDK. SSE `job_event {id}` invalidates the matching run/operations detail and active Team board, and the native `/api/executions` adapter now returns the actual bounded instruction rather than empty task labels. For an intentionally paused Team coordinator with active children the UI renders child-derived activity, not a misleading stopped label. New UI controls are compile/contract-tested; real external-provider sessions, production approvals and multi-host workflows need separate integration checks.

## Sub4 native-first UX integration (November 21, 2026)

The original Chat, Rooms, Home, Connections and Activity navigation is retained, with Rust-native controls added instead of a parallel browser executor:

- Transcript-linked Run evidence for chat and each room speaker, plus a shared native Run inspector for the Home/Activity panels, including actual tool receipts, per-call human approval/denial, interrupted-effect reconciliation, step/token/KRW budgets, run-scoped Skills and artifacts.
- Mid-run owner corrections through the durable `/api/v1/runs/{id}/directive` contract; native subagent delegation uses the existing scheduler, per-child budget reservations and tool profiles.
- Operator-scoped and secret-redacted `/api/v1/integrations/status` for configured external coding CLIs and opt-in parallel remote Worker capacity probes; no worker token or binary path is exposed.
- Explicit MCP `/api/mcp/status?probe=true` diagnostics reuse paginated `tools/list`, reporting per-server real tool counts or bounded errors, never executing a discovered tool.
- Nonfunctional blanket-approval UI controls were removed because the native Rust policy does not grant them. The UI now describes the actual one-time per-effect approval workflow.

External CLIs, separate remote nodes, production deployment, and model-cost benchmark success depend on their real configurations and are **not** implied by a green TypeScript/Rust CI.

The main Chat's manual compact command persists a bounded excerpt of older messages without deleting the original transcript. The manual Learn command saves explicit owner messages as provenance-labeled global memory. These are native projections, not hidden model summarization or a second agent scheduler.

[Repository duplication audit and preserved contracts](docs/SUB4_DEDUP_AUDIT_2026-11-20.md) · [Retired subsystem and dependency audit](docs/SUB4_RETIRED_CODE_AUDIT_2026-11-20.md) · [Frontend and account-sign-in cleanup](docs/SUB4_FRONTEND_AUTH_CLEANUP_2026-11-20.md)

## P3 distributed native Harness workers (opt-in, November 21, 2026)

The coordinator advertises `remote_worker_consult` (read-only) and, if a separate Writer credential is configured, `remote_worker_edit` (owner-approved isolated Team Writer). Both reuse the **single native Rust Run/approval/SQLite journal**. The remote server accepts only scoped capability tokens, not the coordinator's unrestricted bearer credentials. No automatic replay is performed after a potentially billable or mutating request.

**Per-node configuration:** Set `DOMINION_REMOTE_WORKER_ACCEPT_TOKEN` (32+ characters) on each worker. Writer nodes additionally require `DOMINION_REMOTE_WORKER_ENABLE_WRITER=true`, `DOMINION_REMOTE_WORKER_WRITER_TOKEN` (separate secret) and an explicit relative-repository allowlist in `DOMINION_REMOTE_WRITER_REPOSITORIES`. The same Git source revision must exist on the remote node. The coordinator can keep the legacy single `DOMINION_REMOTE_WORKER_URL` / `DOMINION_REMOTE_WORKER_TOKEN`, optionally adding `DOMINION_REMOTE_WORKER_WRITER_TOKEN`, or configure a 1–16-node **static discovery registry**, such as:

```json
[
  {
    "id": "node-a",
    "url": "https://node-a.example.com",
    "token": "<worker-read-token-32-plus-chars>",
    "writerToken": "<distinct-writer-token-32-plus-chars>",
    "weight": 2
  },
  {
    "id": "node-b",
    "url": "https://node-b.example.com",
    "token": "<worker-read-token-32-plus-chars>",
    "weight": 1
  }
]
```

Put the JSON string into `DOMINION_REMOTE_WORKERS_JSON` on the coordinator; do not check real secrets into Git. The gateway probes each remote `/api/v1/remote-worker/capabilities` (auth, available slots, Writer permission, allowed repository) and selects a live matching node by deterministic weighted rendezvous. The destination is bound in the originating Run journal **before dispatch**. A disconnect or uncertain outcome never triggers automatic rerouting.

**Remote Writer:** The native journal first approves `remote_worker_edit` for an owner-authored Task Team **Writer**. The source coordinator ships the bounded exact Git seed from the Team's original revision to the remote node's detached worktree. The remote native Harness produces a SHA-256-pinned regular-file patch (max 128 KiB), which is transferred back and installed in the originating **Writer child worktree only**, under an already journaled effect intent. The original user checkout and Team coordinator remain untouched. A separate existing `preview_task_team_patch` / `approve_task_team_patch` owner approval is still mandatory to merge those edits into the Team coordinator.

**Cost and cancellation:** Remote reported KRW charges are journaled exactly once into `Run.remote_cost_krw` and reduce the available hierarchical budget; these are distinct from the local model's `Run.cost_krw`. A remote Run may still incur an uncertain provider invoice after a disconnect. Cancellation is forwarded to the original remote Run ID and remains uncertain until the source journal confirms it. The remote node and coordinator keep independent provider receipts.

**Two-machine smoke test:** `node scripts/sub4-distributed-smoke.mjs` checks **at least 2 authenticated real endpoints** from `DOMINION_REMOTE_WORKERS_JSON` without executing a billable task. Set `DOMINION_REMOTE_SMOKE_EXECUTE=true` explicitly to submit a bounded native task to both nodes and check their distinct Run IDs, statuses and reported KRW. This cannot provision remote devices, TLS certificates, Git repository clones, or provider credentials for the operator; CI mocks do not establish real hardware readiness.

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
