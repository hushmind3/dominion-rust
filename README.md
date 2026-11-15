# DOMINION · Autonomous Execution Studio

**Native Rust SDK and independent Rust agent engine. HTTP API, TypeScript SDK and GUI are adapters to the same engine.**

The Rust core is a **clean rebuild** on `dominion-sub4`; the React/Vite frontend source is adopted from `main`. The Rust core does not inherit the previous runtime or its old task abstraction. The Rust server controls all model calls, tool execution, decisions, persistence and recovery. The UI and SDK call that same server rather than embedding a separate agent engine.

## What currently runs

| Surface | Entry point | Current capability |
|---|---|---|
| Native Rust SDK | `src/lib.rs`, `src/sdk.rs` | In-process run submission, tool/recovery events, source worktree import, delegated workers, group/video/RSI operations |
| Autonomous work | `POST /api/v1/runs` | Model-generated plan, native function-tool loop, file artifacts, bounded execution, secondary verifier and corrective turns |
| Human intervention | `/runs/{id}/approval`, `/directive`, `/resume`, `/cancel` | Persistent decisions, revised user instructions, resume after restart, cancellation between operations |
| Events | `GET /runs/{id}/events?since=N` | Replayable, sequence-numbered SSE timeline; snapshots remain authoritative |
| Group conversations | `/api/v1/groups` | Distinct participants, role-based and @mention speaker selection, shared history, model tiers, round and estimated-cost limits |
| Video | `/api/v1/videos` | Optional model storyboard, FFmpeg scene rendering, optional local speech, cached-scene partial rerender, merged MP4 download |
| Non-weight improvement | `/api/v1/improvements` | Model candidate on designated source file; detached Git worktrees; fixed baseline/candidate tests and numeric score; explicit Git adoption; inherited text reflections |
| Model integration | `src/provider.rs` | OpenAI-compatible chat-completions transport, configurable main/economy models, economy-to-main failover, token usage and estimated cost |
| Files and commands | `src/tools.rs` | Workspace read/write/search, exact-match atomic editing, Git status/diff, MCP; optional approved shell |
| Existing Git projects | `POST /api/v1/runs/from-repository` | Creates an isolated detached worktree; original checkout is not edited |
| Subagents | `/api/v1/runs/{id}/subagents` | Child native workers with lineage, isolated history/worktrees, sibling budget reservations and parent-visible results |
| Context optimization | `src/context.rs` | Non-destructive request projection preserving explicit user directives and tool call/result pairing; estimated saved characters tracked |
| Studio | `web/src/App.tsx` and `web/src/pages/RustWorkspace.tsx` | Adopted `main` React 19/Vite shell and Korean navigation, connected to real Rust v1 run/group/video/RSI/config routes |
| TypeScript helper SDK | `sdk/typescript` | Typed HTTP convenience client; has no independent agent policy or runtime |

## Frontend adoption from main (2026-11-15)

The **full React/Vite frontend source tree from the main branch** was imported into
`web/` (including navigation, branding, i18n, visual components and original
page source). The old sub4 `web/app.js` and HTML-only Studio were removed.
The Rust server now serves built `web/dist` static assets by default;
`DOMINION_WEB_DIR` can point to a relocated build.

`web/src/App.tsx` retains main's navigation, sidebar, search palette, theme and user
profile chrome. `web/src/pages/RustWorkspace.tsx` provides directly connected
workspaces for native runs, approvals, directives, tool reconciliation, code artifacts,
delegated subagents, group discussions, video render/scene editing, RSI evaluation/
adoption and current model configuration. `web/src/api.ts` handles JSON and
authenticated binary video responses. No second agent engine is embedded in React.

The **original main Home dashboard is mounted** and reads real Rust status, run
history and model configuration; its native-work panel creates actual Rust runs.
Other original main feature pages remain as source, but their old `/api/*` calls
expect the larger main backend and are **not mounted as functional pages** until each
capability is migrated to the current Rust API. In particular, main's desktop/Tauri
host, advanced provider management, channel system and SQLite state are **not**
present in the sub4 Rust backend. The original frontend source is not a claim of
feature parity. The Rust-linked workspaces are the currently supported UI path.
The main owner-profile editor remains available; its display name/avatar are
saved as a device-local browser UI preference, not as shared Rust session data.

For frontend development, run `cargo run` in the repository root, then
`cd web && npm run dev` in another terminal. Vite proxies `/api` to
`http://127.0.0.1:8787` by default. Before starting the packaged Rust HTTP
server, run `cd web && npm ci && npm run build` at least once. A missing
`web/dist` build intentionally returns a static 404 instead of presenting a
fake standalone fallback UI.

## Native Rust SDK (primary)

The supported native entrypoint is `Dominion::open(Settings)` in `src/sdk.rs`, exported by `src/lib.rs`.
The executable in `src/main.rs` now only serves the HTTP adapter backed by that same instance.
See [Rust SDK](docs/RUST_SDK.md) and [2026 frontier comparison](docs/FRONTIER_2026.md).
The model adapter cannot replace the Rust planning/tool/recovery loop.

## Start

Requires a modern Rust toolchain and Node.js 20.19+. The original `main` React/Vite frontend is now the canonical UI. FFmpeg with `libx264` and `drawtext` enables MP4 rendering; local `espeak` is optional for basic narration.

```bash
cp .env.example .env
# Export the desired environment variables from .env in your shell.
export DOMINION_API_KEY="your-provider-key"
export DOMINION_MODEL="your-provider-model"
cd web
npm ci
npm run build
cd ..
cargo run
```

Open [http://127.0.0.1:8787](http://127.0.0.1:8787).

For a local OpenAI-compatible server, set `DOMINION_BASE_URL=http://127.0.0.1:8000/v1`; its key can be empty. For remote access set both `DOMINION_BIND` and `DOMINION_ACCESS_TOKEN`. The server refuses non-loopback binding without that token. API credentials are read from server environment variables and not returned through the configuration endpoint.

```bash
cd sdk/typescript
npm install
npm test
```

## Example: independent TypeScript client

```ts
import { Dominion } from '@dominion/sdk';
const client = new Dominion({baseUrl:'http://127.0.0.1:8787'});
const run = await client.runs.create({instruction:'Create a useful HTML page',budget_krw:3000});
for await (const event of client.events(run.id)) console.log(event.kind, event.message);
const result = await client.runs.get(run.id);
console.log(result.status, result.answer);
```

## Execution rules and budget

- Each run owns `workspace/<uuid>/` for its generated artifacts and `runtime/runs/<uuid>.json` for durable snapshots.
- A run advances through plan → model/tool rounds → independent verification → corrective rounds, up to its configured step budget.
- Shell is **disabled by default**. Enabling it with `DOMINION_ENABLE_SHELL=true` still requires approval for each operation. `DOMINION_SHELL_SANDBOX=required` (default) additionally requires Linux bubblewrap (`bwrap`) or macOS Seatbelt (`sandbox-exec`), constraining filesystem writes to the isolated workspace and disabling network. Unavailable OS backends **fail closed**. A deployment operator can explicitly set `DOMINION_SHELL_SANDBOX=off` to run approved shell commands unsandboxed; that choice is not safe for untrusted code. On Windows, use a Linux VM/WSL2 sandbox for supported isolation. The same shell sandbox gates approved RSI test and score scripts.
- Estimated model cost uses user-configured KRW-per-million-token input/output rates and reported usage. **Prices default to zero when unspecified**; zero does not establish that paid model usage is free. Video CPU, storage and speech rendering are separately measurable local resource costs, not included in the API-token estimate.
- Requests for self-improvement execute supplied test/score commands **only after approval**, using detached Git worktrees. Candidates are adopted only if the fixed scoring procedure improves or a failing baseline becomes passing. No model-weight modification, training, LoRA or fine-tuning pathway exists.
- Mid-operation cancellation is cooperative; bwrap/Seatbelt confinement is available for explicitly approved shell and RSI scripts, while CPU/memory quotas, tool process-tree supervision and all-platform sandbox parity remain incomplete. Current MCP is Streamable HTTP only. A separate optional Veo backend exists; its real-motion output and cost are not equivalent to the default FFmpeg scene renderer.
- Cached input tokens are charged at user-configured cached-token rates only when the provider reports an explicit cache count; the engine does not yet control every provider-specific caching feature.
- Model context is projected under DOMINION_CONTEXT_MAX_CHARS without discarding the persisted transcript; saved characters are not identical to provider-billed tokens.

## Current data and migration

The Rust runtime was rebuilt with a new storage format, while the main React frontend source was preserved. Earlier runtime data is **not automatically imported**. The new disk layout uses explicit, stable JSON files for runs, groups, videos and experiments. Back up the `runtime` and `workspace` directories before deleting or moving a deployment.

## Engineering references

Read [Architecture](docs/ARCHITECTURE.md) for all entrypoints, lifecycles and persistence boundaries, [2026 Research](docs/RESEARCH_2026.md) for the design evidence, and [SDK documentation](sdk/typescript/README.md) for API use.

Validation: The focused tool-orchestration revision **compiled and passed 64/64 Rust tests**, and the optional TypeScript SDK job passed in [GitHub Actions run 43686024696](https://github.com/hushmind3/dominion-rust/actions/runs/43686024696) (source commit `d55e15c`). This includes actual model-mock tool-loop integration, parallel file reads followed by an exclusive write, rejection of an unadvertised tool, crash/approval regression tests and existing group/video/RSI regression coverage. Full browser E2E, real-provider trials and public coding benchmarks remain pending. A green build is **not** a frontier-performance score.

## Durable tool execution (2026-11-15)

Rust persists `in_flight` intent before executing each tool, then atomically
records the result and removes the pending call. If the process stops between
those writes, the interrupted side effect must be inspected and reconciled.
The Studio and TypeScript SDK both call `/api/v1/runs/{id}/reconcile`
(`{"succeeded":true,"result":"observed file"}`) before manual resume.

Recoverable provider failures pause the task. Exceeding the configured
model-token budget persists the charge and terminal status.

This is single-process crash recovery, not an OS-isolated shell sandbox.
See `docs/ARCHITECTURE.md` for current scope and migration gaps.

## MCP external tools (2026-07-28)

The engine can discover external MCP Streamable HTTP tools and expose them to the model through its own native function-call API. Configure `DOMINION_MCP_SERVERS` with a JSON array of up to 8 servers containing `name`, `url` and optional `bearer_token`. Plain HTTP is allowed only for loopback; HTTPS is required elsewhere. External calls always pause the run for explicit user approval, and their intent is journaled before execution to avoid accidentally replaying side effects after a crash. No MCP secret is exposed to the model or Studio. The 2026 stateless HTTP transport supports JSON and per-request SSE responses, including annotated MCP headers; MRTR user elicitation, subscription streams and legacy transport fallback are not connected yet.

## Generative motion video via Veo 3.1

The video API supports `backend: "local"` (default) and `backend: "veo"`. Local FFmpeg renders captioned scenes without a cloud video provider. The Veo backend submits long-running generated-motion scenes through the current Gemini OpenAI-compatible Videos API. Export `DOMINION_VEO_API_KEY`, then select Veo in the Studio. Each scene must be 4, 6, or 8 seconds. The server persists remote operation IDs and reuses them on retries to avoid unnecessary duplicate requests. Clips are normalized and assembled into an MP4 through FFmpeg. Remote-provider cost depends on your billing agreement; configure `DOMINION_VEO_KRW_PER_SECOND` for an estimated KRW amount. At zero, the display omits any video-provider charge even if the remote provider bills you. The Veo integration has a mock HTTP test; a paid live generation test has not been run. The retired Sora Videos API is not used.

## Rust agent hardening revision (2026-11-15)

The 2026 frontier-driven work now enforces durable hierarchical child budget escrow and idempotent settlement (`src/policy.rs`, `src/engine.rs`), read-only delegated workers, explicit approval/conflict-check before adopting a child Git patch, transient API retry, workspace file pagination beyond the former 200-file limit, project-specific persistent memory (`src/knowledge.rs`) and lazy SKILL.md procedures (`src/skills.rs`). The bundled main React frontend uses the same native Rust routes for these operations; the optional TypeScript SDK now exposes their API surfaces.

The model interface accepts optional OpenAI-specific prompt-cache TTL/key options, and reports provider-supplied cache read/write token counts separately with configurable KRW rates. Approved shell and RSI test scripts use fail-closed Bubblewrap/Seatbelt confinement by default. User cancellation during an executing tool leaves an auditable outcome and only becomes terminal after the tool receipt or explicit reconciliation.

The targeted Rust/TypeScript build checks passed in Actions run 43686024696 (64 Rust unit/integration tests, 0 failures). A full browser E2E, real-provider task suite and SWE-bench Verified have **not** been run, so frontier-level task success is not claimed.

## Focused acceptance: durable tool lifecycle

The production route is API or native SDK -> Engine::drive -> journal::begin ->
Store::mutate (fsynced intent) -> tool executor -> journal::finish ->
Store::mutate (receipt and intent removal atomically) -> API/SSE/SDK.
Denial uses journal::deny without executing or recording an intent.

Restart behavior:
- Queued and never started: resume persisted queue.
- Started pure local inspection: safely retry the queued read.
- Started mutating write, shell, MCP, delegation: UNKNOWN outcome and
  no automatic replay. User must observe external state and reconcile.
- Approved but not yet started: recover prior approval and resume.
- Undecided human approval: preserve it; never assume consent.
- Cancellation during a tool: terminate only after durable result or reconciliation.

Model tool-call IDs are checked against the durable transcript for duplicates;
invalid responses produce a corrective model turn without dispatch.
Store uses uniquely named create-new temporary snapshots, fsyncs data,
renames atomically, then syncs the directory on Unix.

Focused automated coverage: src/journal.rs transition and duplicate tests;
src/store.rs simulated before/after crash and write failure tests;
src/sdk.rs approved-but-unstarted restart with real file output;
src/engine.rs denial via real scheduler and no tool-start record.

This is NOT a claim of exactly-once external effects, complete crash recovery
of billed model calls, or parity with an entire frontier product.
The durable tool execution and parallel read scheduler passed a focused Rust build/test
check on commit `d55e15c` in Actions run 43686024696. Unmeasured external-provider
performance, full frontend integration and global frontier parity remain separate gates.

## Model tool-call orchestration (2026-11-15 implementation)

The production **Rust** code follows explicit source-level lessons from:
- Codex `codex-rs/core/src/tools/parallel.rs` (read/write exclusivity and dispatch barrier):
  https://github.com/openai/codex/blob/b016cd18472588606c04af565528c4cd6746a41f/codex-rs/core/src/tools/parallel.rs
- DeepSeek Harness `packages/core/tools/src/batch.ts` (bounded pool, model-ordered results):
  https://github.com/deepseek-ai/deepseek-harness/blob/39163dd30f703a79079e2d2446545cff3c453738/packages/core/tools/src/batch.ts
- DeepSeek Harness `packages/core/tools/src/json-schema.ts` and `pipeline.ts` (argument admission, fail-closed schema):
  https://github.com/deepseek-ai/deepseek-harness/blob/39163dd30f703a79079e2d2446545cff3c453738/packages/core/tools/src/json-schema.ts

Actual Dominion call path:
1. `provider.rs` submits `parallel_tool_calls=true` by default (operator can disable).
2. `engine.rs` freezes the exact advertised catalog on the persisted run each model step.
3. `tool_schema.rs` validates every requested tool and JSON argument before any side effect.
   A missing/invalid/unsupported schema gives an ordered TOOL ERROR result, not silent execution.
4. `tool_batch.rs` groups only contiguous safe workspace reads in a rolling pool
   (limit 1..8, default 4), preserving the original model call order.
5. `journal.rs` persists batch intent before dispatch and all ordered receipts together.
   On crash, unfinished reads are safe to retry. Writes, shell, remote MCP,
   subagents and memory mutations remain exclusive and unknown effects are never auto-replayed.
6. `src/api.rs`, the native Rust SDK, TypeScript HTTP adapter and React UI
   still use exactly the same `Engine` and run state; no imported foreign agent runtime.

Configured by `DOMINION_PARALLEL_TOOL_CALLS` and `DOMINION_MAX_PARALLEL_READS`.
Backend tests include a real mock-model multi-tool turn reading two files
concurrently, executing one exclusive write and verifying model-ordered receipts;
bounded concurrency, schema rejection, rollback and crash restart also have
regressions. Benchmark parity, broad MCP schema coverage and real-provider cost
effects remain unproven until separately measured.
