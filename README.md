# DOMINION · Rust-native autonomous agent platform

DOMINION is a local-first **Rust/Axum agent engine** with a React UI, SQLite
durable work ledger, native tool execution, memory, group conversations,
model/provider routing, isolated coding goals, video production and a
source-level non-weight RSI experiment loop. The React front end and
TypeScript SDK are **clients of the Rust engine**, not replacement agent
runtimes or imported Copilot policies.

## Quick start

Requirements: Rust toolchain, Node.js for the front-end build, and a supported
provider or local inference backend. Keep API keys in the app's encrypted
vault; do not commit secrets or runtime state.

```bash
npm ci --prefix web
npm run build --prefix web
cargo build --release --locked
DOMINION_PORT=4317 DOMINION_WEB=web/dist DOMINION_DATA=runtime \
  ./target/release/dominion-rust
```

Open `http://127.0.0.1:4317`. For development use
`./start-rust.command`, or run `npm run dev:server` and `npm run dev`
from `web/` in separate terminals. macOS desktop packaging and startup
instructions: [desktop guide](docs/DESKTOP.md).

The Rust server binds to localhost. It exposes the API and built UI on
**4317** by default. Rust-managed llama.cpp may serve local GGUF models
on **4319**; MLX and other runtimes are optional integrations. Node is a
front-end build tool, not a required separate agent policy server.

## Architecture

```text
React / external Rust & TypeScript SDK clients
            │ HTTP + SSE
            ▼
Rust Axum API ── authentication / approvals / safe tool gateway
            │
    Rust execution harness
    ├─ chat/runtime ── providers, streaming, tool selection, recovery
    ├─ goals/ ─────── planning, execution, review, CAS finalization
    ├─ rooms/ ─────── multi-agent roles, token budgets, group context
    ├─ jobs ───────── durable queue, retries, cancellations
    ├─ workflows ──── typed reusable work
    ├─ rsi/ ───────── isolated source candidates and frozen evaluation
    └─ video/ ─────── ComfyUI / paid-provider approvals and shot quality
            │
    SQLite records + audit + event log + vault
```

| Source | Responsibility |
| --- | --- |
| `src/engine/chat/` | Native model/tool event loop, streaming, context and recovery |
| `src/engine/goals/mod.rs`, `src/engine/goals/` | Goal module entry, independent runner, effect lineage, verification and finalization |
| `src/engine/verification.rs` | Evidence-bound acceptance, independent of agent prose |
| `src/engine/tools.rs`, `src/engine/tools/` | Rust tool dispatch, dynamic discovery, bounded delegated workers |
| `src/db.rs`, `src/db/usage_tree.rs` | Transactional SQLite, goal ancestry and descendant cost attribution |
| `src/engine/rooms.rs`, `src/engine/rooms/` | Durable room exchanges and token-cache telemetry |
| `src/engine/video.rs`, `src/engine/video/` | Multi-shot generation, FFprobe metadata and FFmpeg pixel checks |
| `src/engine/maintenance.rs`, `src/engine/maintenance/artifacts.rs` | Encrypted rolling backups and safe generated-artifact pruning |
| `src/engine/rsi/` | Frozen candidate/control evaluation, bounded adoption, learned strategies |
| `sdk/rust/`, `sdk/typescript/` | Native client APIs for goals, models, rooms, video, memory, skills and RSI, using the same Rust server policy as React |
| `web/src/pages/home/QuickGoal.tsx` | Nontechnical goal composer, route confirmation, owner steering, live goal progress | 
| `web/src/` | React UI, models, goals, group discussions, memory, video, operations |

## Main capabilities

**Agent execution.** The Home screen now starts with a natural-language goal
composer and a read-only model preview. Owners can launch a durable goal using
a configured provider even without creating an agent manually. The native
`POST /api/goals/prepare` resolves an optional registered specialist without
calling a model, queuing work, or assuming free API usage. Confirmed goals use
the same Rust executor, checkpoint, approval and verification system.

Tool calls and approvals are persisted as operations, not
trusted from assistant claims. The local engine supports source editing,
command execution, HTTP, memory, tools registered through MCP, and
bounded subagent delegation. A cancelled or ambiguous side-effecting operation
is not blindly replayed. Owner-steered goal plans are versioned, and the
`reviewPolicy=acceptance_ready` option avoids a model review only while actual
Rust acceptance criteria remain unmet. Independent final review stays mandatory.
A `command_succeeded` criterion can require a fresh post-edit test receipt.
Each goal can use an isolated Git worktree with an owner-reviewed diff and
explicit apply/discard decision. Git review runs with bounded subprocess output
and bounded untracked-file previews; oversized changes require splitting before
approval, and links outside the checkout cannot be previewed as ordinary files.

**Model/provider choice.** One capability- and owner-KRW-price-aware
native Rust router serves chat and beginner goal selection. Per-provider
history input now reserves the routed model's output, prompt, retrieval and
tool schemas using a conservative byte estimate; the owner task survives
history truncation. No extra model call is required for this calculation.

Connect cloud providers or local GGUF/MLX models.
Tokens, cache reports and usage telemetry are kept separately from estimates;
no unmeasured provider bill is presented as actual expense.

**Group conversations.** Multiple agents share one room transcript without
forging new owner messages. Speaker selection, per-room token ceilings,
pause/recovery, learning and evidence inspection operate as Rust jobs.

**Video.** The video page shows plain-language engine choices (FFmpeg caption
cards, configured ComfyUI GPU, or paid Kling cinematic) and folds technical
FPS, seed, negative prompts and multi-shot controls by default. Paid
provider quotes and explicit owner acknowledgements remain visible.
ComfyUI multi-shot pipelines perform technical clip checks and can
optionally decode shot frames for mostly-black or frozen-frame defects. The
engine avoids rerendering previously verified shots. Charged remote
submissions preserve owner approval and unknown-outcome review.

**Source RSI.** Candidate patches run in isolated worktrees and are compared
against frozen independent checks before source-level adoption and
recompilation. This does **not** train model weights. Benchmark claims require
external completed evaluations.

**Durable streaming and SDK.** `GET /api/events?after=<sequence>` replays the
retained SQLite event log, including >500 records, and signals
`replay_gap` when retention has expired. The TypeScript SDK can reconnect a
read-only watch and observe a goal until a terminal state without resubmitting
side-effecting requests. Events are broadcast only after their SQLite insert
succeeds; a failed insert neither advances the durable sequence nor announces
a falsely replayable message. See [SDK documentation](sdk/typescript/README.md).

## Main API entry points

| Task | Endpoint |
| --- | --- |
| Health and stream | `GET /api/health`, `GET /api/events` |
| Chat | `POST /api/chat` (streaming) |
| Goals | `POST /api/goals/prepare` (no side effects), `POST /api/goals`, `GET /api/goals/strategies`, `GET /api/goals/:id`, steering and cancel |
| Durable work | `GET /api/jobs`, `GET /api/executions` |
| Sessions | `GET /api/sessions`, `GET /api/sessions/:id/messages` |
| Group chat | `GET/POST /api/rooms/:id/messages`, `GET /api/rooms/:id/state` |
| Providers/models | `GET /api/providers`, `GET /api/model-library/installed` |
| Tools and MCP | `GET /api/tools`, `GET /api/mcp` |

The Rust source and tests are authoritative for exact routes and payloads;
a historical UI screenshot or release note is not an API contract.

## Source hygiene and runtime retention

```bash
node scripts/check-repo-hygiene.mjs
cargo test --release --locked --all-targets
npm run typecheck --prefix web
npm run build --prefix web
```

**Never check in generated binaries, caches, downloads, logs or plaintext
snapshots.** `.gitignore` excludes non-source outputs, and CI rejects tracked
artifacts, oversized files and append-only documentation growth. Put new
functionality in the existing responsibility module rather than adding dated
feature reports to this README. Update old passages instead of appending
another release note.

The built-in `maintenance/artifacts.rs` sweeper runs at startup and every
6 hours. It reclaims only **old generated intermediates owned by sealed
jobs**, including obsolete finished-video shot renders, compiler build caches,
cancelled model-download fragments and abandoned temporary backup files.
Final video masters, installed models, SQLite state, frozen RSI evidence,
paused/uncertain work and unrelated files remain protected. Encrypted backups
have separate bounded retention.

## Validation and evidence boundaries

The GitHub Actions [CI workflow](.github/workflows/ci.yml) runs repository
hygiene, the front-end checks and native Rust tests. The inherited RSI
[proof workflow](.github/workflows/sub4-proof-audit.yml) separately validates
structural source evidence. A passing CI is not an independent claim of
SWE-bench solved rate, model quality, video aesthetics, or actual provider
cost. Do not publish unexecuted experiments as achieved benchmark scores.

Operational and research references:

- [ENGINE.md](ENGINE.md) — durable lifecycle, security and integration contracts
- [Architecture and code map](docs/RSI-CODE-MAP.md)
- [Native RSI structure](docs/SUB4-RSI-ARCHITECTURE.md)
- [SWE-bench Verified runbook](docs/SWEBENCH-VERIFIED-RUNBOOK-2026.md)
- [Video, event recovery and TypeScript SDK](docs/SUB4-2026-11-15-VIDEO-EVENT-SDK-RELIABILITY.md)
- [Full-system product integration](docs/SUB4-PRODUCT-UPGRADE-2026-11-14.md)
- [Desktop build and installation](docs/DESKTOP.md)

## License

Copyright 2026 hushmind3. [Apache License, Version 2.0](LICENSE).
