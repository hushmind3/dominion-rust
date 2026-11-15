# DOMINION · Autonomous Execution Studio

**Native Rust SDK and independent Rust agent engine. HTTP API, TypeScript SDK and GUI are adapters to the same engine.**

This is a **clean rebuild** on `dominion-sub4`. It does not inherit the previous runtime or its old task abstraction. The Rust server controls all model calls, tool execution, decisions, persistence and recovery. The UI and SDK call that same server rather than embedding a separate agent engine.

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

The original main branch feature pages remain as source, but their old `/api/*` calls
expect the larger main backend and are **not mounted as functional pages** until each
capability is migrated to the current Rust API. In particular, main's desktop/Tauri
host, advanced provider management, channel system and SQLite state are **not**
present in the sub4 Rust backend. The original frontend source is not a claim of
feature parity. The Rust-linked workspaces are the currently supported UI path.

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
- Shell is **disabled by default**, can be enabled by `DOMINION_ENABLE_SHELL=true`, and requests explicit per-operation approval. Shell execution is **not an OS sandbox**; approved programs have the process user's permissions. Run on isolated infrastructure for untrusted work.
- Estimated model cost uses user-configured KRW-per-million-token input/output rates and reported usage. **Prices default to zero when unspecified**; zero does not establish that paid model usage is free. Video CPU, storage and speech rendering are separately measurable local resource costs, not included in the API-token estimate.
- Requests for self-improvement execute supplied test/score commands **only after approval**, using detached Git worktrees. Candidates are adopted only if the fixed scoring procedure improves or a failing baseline becomes passing. No model-weight modification, training, LoRA or fine-tuning pathway exists.
- Mid-operation cancellation is cooperative; OS sandboxing and process-level resource isolation remain incomplete. Current MCP is Streamable HTTP only. A separate optional Veo backend exists; its real-motion output and cost are not equivalent to the default FFmpeg scene renderer.
- Cached input tokens are charged at user-configured cached-token rates only when the provider reports an explicit cache count; the engine does not yet control every provider-specific caching feature.
- Model context is projected under DOMINION_CONTEXT_MAX_CHARS without discarding the persisted transcript; saved characters are not identical to provider-billed tokens.

## Current data and migration

This branch was rebuilt from an empty source tree. Earlier runtime data is **not automatically imported**. The new disk layout uses explicit, stable JSON files for runs, groups, videos and experiments. Back up the `runtime` and `workspace` directories before deleting or moving a deployment.

## Engineering references

Read [Architecture](docs/ARCHITECTURE.md) for all entrypoints, lifecycles and persistence boundaries, [2026 Research](docs/RESEARCH_2026.md) for the design evidence, and [SDK documentation](sdk/typescript/README.md) for API use.

Validation: Existing test suites and CI workflows are present. Current frontier-refactor commits deliberately use [skip ci]; full CI and public benchmarks are deferred until source residue and compatibility gaps have been reviewed. Static source wiring is not a substitute for a compile/test run. In-repo tests include a simulated model completing an actual file-producing agent loop, independent Git candidate evaluation/adoption, and an FFmpeg artifact smoke test when FFmpeg is available. A green build does **not** constitute a frontier benchmark score.

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
