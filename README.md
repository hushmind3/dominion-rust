# DOMINION · Autonomous Execution Studio

**Independent Rust execution harness + versioned HTTP API + TypeScript SDK + browser studio.**

This is a **clean rebuild** on `dominion-sub4`. It does not inherit the previous runtime or its old task abstraction. The Rust server controls all model calls, tool execution, decisions, persistence and recovery. The UI and SDK call that same server rather than embedding a separate agent engine.

## What currently runs

| Surface | Entry point | Current capability |
|---|---|---|
| Autonomous work | `POST /api/v1/runs` | Model-generated plan, native function-tool loop, file artifacts, bounded execution, secondary verifier and corrective turns |
| Human intervention | `/runs/{id}/approval`, `/directive`, `/resume`, `/cancel` | Persistent decisions, revised user instructions, resume after restart, cancellation between operations |
| Events | `GET /runs/{id}/events?since=N` | Replayable, sequence-numbered SSE timeline; snapshots remain authoritative |
| Group conversations | `/api/v1/groups` | Distinct participants, role-based and @mention speaker selection, shared history, model tiers, round and estimated-cost limits |
| Video | `/api/v1/videos` | Optional model storyboard, FFmpeg scene rendering, optional local speech, cached-scene partial rerender, merged MP4 download |
| Non-weight improvement | `/api/v1/improvements` | Model candidate on designated source file; detached Git worktrees; fixed baseline/candidate tests and numeric score; explicit Git adoption; inherited text reflections |
| Model integration | `src/provider.rs` | OpenAI-compatible chat-completions transport, configurable main/economy models, economy-to-main failover, token usage and estimated cost |
| Files and commands | `src/tools.rs` | Bounded workspace read/write/list/search; optional shell requiring per-call approval |
| Studio | `http://127.0.0.1:8787` | Korean workspaces for execution, group conversation, video, improvement and model connection |
| SDK | `sdk/typescript` | Typed methods for the same HTTP endpoints, SSE, waiting and artifact access |

## Start

Requires a modern Rust toolchain. FFmpeg with `libx264` and `drawtext` enables MP4 rendering; local `espeak` is optional for basic narration. TypeScript SDK build uses Node 20+.

```bash
cp .env.example .env
# Export the desired environment variables from .env in your shell.
export DOMINION_API_KEY="your-provider-key"
export DOMINION_MODEL="your-provider-model"
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
- Mid-operation cancellation is cooperative. A running command is bounded by a timeout; OS container isolation, concurrent subprocess cancellation, remote tool servers, full provider prompt caching and high-end cinematic rendering are **not yet implemented**.

## Current data and migration

This branch was rebuilt from an empty source tree. Earlier runtime data is **not automatically imported**. The new disk layout uses explicit, stable JSON files for runs, groups, videos and experiments. Back up the `runtime` and `workspace` directories before deleting or moving a deployment.

## Engineering references

Read [Architecture](docs/ARCHITECTURE.md) for all entrypoints, lifecycles and persistence boundaries, [2026 Research](docs/RESEARCH_2026.md) for the design evidence, and [SDK documentation](sdk/typescript/README.md) for API use.

Validation: GitHub Actions compiles/tests Rust and builds/tests the SDK on pushes to this branch. In-repo tests include a simulated model completing an actual file-producing agent loop, independent Git candidate evaluation/adoption, and an FFmpeg artifact smoke test when FFmpeg is available. A green build does **not** constitute a frontier benchmark score.

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
