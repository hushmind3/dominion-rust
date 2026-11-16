# DOMINION · Autonomous Execution Studio

**Native Rust SDK and independent Rust agent engine. HTTP API, TypeScript SDK and GUI are adapters to the same engine.**

The Rust core was independently rebuilt on **`dominion-sub4`**, using main's React frontend as a starting baseline. The sub4 frontend **has since diverged**: its Video page, model installer, and autonomous production workspace now call dedicated Rust endpoints. The server exposes both `/api/v1/*` and the compatible `/api/*` product API. This branch is independently developed and tested; changes are **not merged into `main`**. Unsupported actions fail with explicit JSON rather than synthetic success. Entire-product feature parity is not claimed.

Read [exact Git tree evidence and the real API incompatibilities](docs/FRONTEND_ALIGNMENT.md) and [feature migration status](docs/UI_FEATURE_PARITY_2026.md).

## Existing native Rust backend

| Surface | Implementation | Capability |
| --- | --- | --- |
| Rust SDK | `src/lib.rs`, `src/sdk.rs` | Native work submission, recovery events, detached worktrees, delegated runs, groups, video and RSI |
| Native API | `/api/v1/runs` | Planning, tools, bounded run budgets, verifier and corrective turns |
| Approval and resumption | `/api/v1/runs/{id}` actions | Durable human approvals, reconcile/continue/cancel |
| RSI | `/api/v1/improvements` | Candidate edits, frozen independent judge, adoption and strategy evidence |
| Video and multi-agent | `/api/v1/videos`, `/api/v1/groups`, `/api/video/productions`, `/api/video/model-packs` | Video jobs, autonomous production, native/ComfyUI rendering, model installation and optional YouTube uploads |
| SDK helper | `sdk/typescript` | Native Rust HTTP client |
| Product frontend | `web/` | Derived from the main UI baseline, now with independently developed sub4 video creation, in-page model installation and production monitoring |

## Frontend source identity

- The initial integration imported main frontend from commit `879dc23dce309688691c200b001b180bead95a12`. That was a **historical baseline**, not the current tree identity.
- Sub4 modifies `web/src/pages/Video.tsx`, `web/src/components/VideoAutoStudio.tsx` and `VideoModelInstaller.tsx`. These changes exist **only on `dominion-sub4`** until separately reviewed and merged.
- Rust serves the production `web/dist` selected by `DOMINION_WEB_DIR` or the installed build; Vite serves the development page on `127.0.0.1:5173` with the Rust API proxy on `127.0.0.1:4317`. Seeing a changed page requires running this branch's frontend/server on those ports, not committing changes to `main`.

## Native Rust SDK (primary)

The supported native entrypoint is `Dominion::open(Settings)` in `src/sdk.rs`, exported by `src/lib.rs`.
The executable in `src/main.rs` now only serves the HTTP adapter backed by that same instance.
See [Rust SDK](docs/RUST_SDK.md) and [2026 frontier comparison](docs/FRONTIER_2026.md).
The model adapter cannot replace the Rust planning/tool/recovery loop.

## Start

Requires a modern Rust toolchain and Node.js 24.12+ per the exact main frontend package. The original main React/Vite frontend is the canonical UI source. FFmpeg with `libx264` and `drawtext` enables MP4 rendering; local `espeak` is optional for basic narration.

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

Open [http://127.0.0.1:4317](http://127.0.0.1:4317).

For a local OpenAI-compatible server, set `DOMINION_BASE_URL=http://127.0.0.1:8000/v1`; its key can be empty. For remote access set both `DOMINION_BIND` and `DOMINION_ACCESS_TOKEN`. The server refuses non-loopback binding without that token. API credentials are read from server environment variables and not returned through the configuration endpoint.

```bash
cd sdk/typescript
npm install
npm test
```

## Example: independent TypeScript client

```ts
import { Dominion } from '@dominion/sdk';
const client = new Dominion({baseUrl:'http://127.0.0.1:4317'});
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

## Video-only model library and precision-aware downloads (sub4, November 2026)

The **Video → 자율 콘텐츠** screen now owns its **video-only model library**. It provides family selection (Wan 2.2 TI2V 5B, LTX-Video 2B, HunyuanVideo 1.5), then model-family-specific precision choices, file sizes and installation. Chat-language models never appear in this picker. These are independently executable ComfyUI graph families, not merely quantization aliases of Wan.

| Model selection | Estimated video weight | Runtime loader |
| --- | ---: | --- |
| GGUF Q3_K_M | ~2.55 GB | ComfyUI-GGUF `UnetLoaderGGUF` |
| GGUF Q4_K_S | ~3.12 GB | ComfyUI-GGUF |
| GGUF Q4_K_M (default) | ~3.43 GB | ComfyUI-GGUF |
| GGUF Q5_K_M | ~3.81 GB | ComfyUI-GGUF |
| GGUF Q6_K | ~4.21 GB | ComfyUI-GGUF |
| GGUF Q8_0 | ~5.4 GB | ComfyUI-GGUF |
| Official FP16 | ~9.31 GB | ComfyUI built-in `UNETLoader` |

### Supported model families and actual graph routes

- **Wan 2.2 TI2V 5B**: 7 GGUF/FP16 precision choices; bundled API graph `src/wan22_5b_api.json`; UMT5 and Wan VAE shared between precisions.
- **LTX-Video 2B**: one FP16 checkpoint plus T5-XXL text encoder; separate video graph `src/ltxv_2b_api.json` using the LTXV scheduler, latent, and conditioning nodes. Legacy lower-compute choice, not a claim of 2026 state-of-the-art fidelity.
- **HunyuanVideo 1.5 T2V**: FP16 720p diffusion model, Qwen VL and ByT5 dual encoders, and Hunyuan VAE; separate video graph `src/hunyuan15_720p_api.json` with custom sampler and Hunyuan 1.5 video latent. This class is substantially heavier than Wan 5B.

The `frontend_video_models.rs` family-aware asset manifests share one **restartable SHA-256 verified downloader**, while `frontend_comfy.rs` binds the selected checkpoint/diffusion/quantized loader, applies the requested prompt and duration, and checks the required ComfyUI node family. New families require an explicit model manifest and compatible renderer graph; the current catalogue is a vetted starting set, **not a complete model search engine**. The GUI now separately selects video model **family** and **precision**, and the browser checks horizontal overflow at 375px, 768px and 1440px. The remote GPU and local ComfyUI live-rendering paths still require machine-level validation.

The **shared** UMT5 text encoder (~6.27 GB) and Wan 2.2 VAE (~1.31 GB) are downloaded only when missing (thus a new Q4_K_M user needs ~11 GB total, while switching precision only adds the new weight). These are *download/storage estimates*, not guaranteed GPU inference footprints. GGUF variants originate from [QuantStack/Wan2.2-TI2V-5B-GGUF](https://huggingface.co/QuantStack/Wan2.2-TI2V-5B-GGUF); the FP16 weight, UMT5, and VAE originate from official Comfy-Org repacks linked in [ComfyUI Wan 2.2 documentation](https://docs.comfy.org/tutorials/video/wan/wan2_2).

The Rust model manager (`src/frontend_video_models.rs`) uses a **closed video-only allowlist**, selects only the requested quantized weight and shared assets, resolves remote Hugging Face metadata, downloads with resumption and verifies SHA-256 before moving files to the selected model library. An automatic Dominion-owned model directory is created on first installation; an existing ComfyUI model directory can be reused, and a manual absolute path is inside an **advanced** section only. The UI always shows the actual precision selected. On completion, it installs an API-format workflow (`src/wan22_5b_api.json`) and substitutes the official `UNETLoader` with `UnetLoaderGGUF` for quantized models.

**Runtime prerequisites and honest readiness:** FFmpeg can render title-card stories without a diffusion model. AI motion generation requires a running compatible ComfyUI instance. GGUF additionally requires the official third-party [city96/ComfyUI-GGUF](https://github.com/city96/ComfyUI-GGUF) loader; the Video status API checks the node and does **not** report GGUF video generation ready when absent. A Dominion-managed library is not automatically mounted into an independently installed ComfyUI instance; to run locally, use a ComfyUI-visible models directory or configure ComfyUI external model paths. Installing or bootstrapping the entire ComfyUI runtime automatically is **not yet implemented**, even though model downloads are in-product. This difference between "weights downloaded" and "runtime ready" must stay explicit.

API: `GET /api/video/model-packs` returns video-only `variants` with `modelBytes`, `downloadBytes`, `installed`, `requiresExtension`, and `selectedVariant`; `POST /api/video/model-packs/install` selects `variant` and optional `modelsDir`; `POST /api/video/model-packs/cancel` pauses. The existing production pipeline `/api/video/productions` generates a storyboard, renders with native FFmpeg or connected ComfyUI, verifies output, and optionally queues YouTube upload only when authorized. Long-form multi-shot AI footage with narration and partial scene regeneration is still a product gap, not a completed turnkey capability.

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


## Source-grounded tool lifecycle, policies and cancellation (2026-11-15)

Official source mechanisms inspected:
- OpenAI Codex typed tool runtime and cancellation: https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/registry.rs
- DeepSeek Harness staged tool runtime and caller-owned abort signal: https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/tools/src/contracts.ts
- DeepSeek tool execution and cancellation docs: https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/core/tools

Dominion remains an independently controlled Rust harness with an explicit tool state machine:
1. Engine::drive freezes model-advertised schemas and validates calls in src/tool_schema.rs.
2. Ordered native pre-hooks in src/hooks.rs can veto tools before dispatch/approval; even approved-but-unstarted calls are rechecked against current hooks. Denials become durable tool receipts, not dispatches.
3. src/tool_batch.rs overlaps only declared pure reads in a bounded pool, honors write barriers and returns results in original model order. src/journal.rs stores intent before dispatch and final result before clearing intent.
4. Native post-hooks run before durable receipt storage, with a configurable model-output cap. Hook implementations cannot approve operations or bypass the journal; the native SDK exposes register_tool_hook.
5. Cancellation is persisted before signalling active model requests and pure reads with Tokio watch receivers. Parallel read cancellation aborts workers, waits for full task quiescence and only then commits ordered receipts. Unknown non-idempotent writes, external MCP calls and delegation remain receipt-safe and cannot be blindly replayed.
6. Environment controls: DOMINION_DENIED_TOOLS (exact tool names), DOMINION_TOOL_RESULT_MAX_CHARS, DOMINION_MAX_PARALLEL_READS and DOMINION_PARALLEL_TOOL_CALLS. Rust API, optional TypeScript SDK and the main React UI share the same runtime state.

Tests added for delayed model request cancellation, pre-hook veto preventing filesystem writes, approved-call re-veto, post-result caps, model-ordered concurrent reads, and durable crash recovery. Real-provider cancellation cannot reverse charges already incurred upstream.

Unfinished: full external-tool-specific cooperative cancellation and quiescence acknowledgements, persistence of dynamically registered native extension hooks across process restarts, and same-model/same-task frontier benchmarks.

Focused verification: GitHub Actions run https://github.com/hushmind3/dominion-rust/actions/runs/43687107564 compiled Rust and passed 69/69 tests, plus the optional TypeScript SDK suite. This included real local HTTP-provider cancellation and hook-veto integration. A subsequent read-worker quiescence regression was added and must be verified separately; broader frontier task-success parity remains unmeasured.


### Tool policy projection and verification

The operator's DOMINION_DENIED_TOOLS is enforced twice: filtered from both
per-run model-facing schemas in policy::select_catalog and the native Rust
SDK catalog, and blocked again by the ordered pre-hook just before dispatch.
This avoids wasting model tokens on unavailable tools without weakening
approval/schema checks. Per-run filtering preserves the frozen catalog for
crash-safe replay; dynamic SDK hooks can only add further denials.

Focused code and regression checks: src/hooks.rs, src/tool_batch.rs,
src/tool_schema.rs, src/journal.rs, src/policy.rs, src/engine.rs,
src/sdk.rs. A previous full source check completed with 70/70 Rust tests
and TypeScript SDK success (GitHub Actions 43687318332). The additional
SDK catalog-mask regression is included in this revision and must pass
the new source check before it is claimed verified. Broader model-task
success rates remain unmeasured.


## 2026 Streamable HTTP MCP: cancellation, progress and audit

Specification (2026-07-28):
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/cancellation.mdx
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/patterns/progress.mdx

The Rust backend implements request-scoped SSE and application/json in src/mcp.rs.
For SSE, it incrementally parses progress and stops reading after the matching
JSON-RPC response frame, instead of waiting for the server to close an idle stream.
On a user cancel it drops the response stream. MCP 2026 Streamable HTTP does
NOT send a notifications/cancelled POST; that signal is stdio-specific.

Client disconnect is NOT proof the remote mutation was undone. src/mcp.rs
returns an explicit Uncertain outcome on cancelled requests, lost transport,
incomplete/malformed replies or remote HTTP server faults. src/engine.rs stores
mcp_remote_outcome_unknown while keeping the in_flight intent, requires explicit
human POST /api/v1/runs/{id}/reconcile, and never automatically repeats the call.
The remote JSON-RPC request ID is deterministic from the persisted run and
tool-call IDs (dominion-<run UUID>-<model tool-call ID>), and is recorded in
tool_started and uncertain-result events for external service log correlation.

The request carries _meta.progressToken bound to its JSON-RPC request ID.
Only matching progress notifications are forwarded; max 16 buffered and
32 persisted events per call prevent unbounded work and storage. The adopted
main React UI distinguishes an active MCP call with progress from a paused
call requiring result reconciliation; native REST and SDK share these events.

Checks: real local Streamable HTTP fixtures exercise a live SSE response that
stays open after its final frame, user cancellation, mismatched progress tokens,
incomplete HTTP/SSE responses, 5xx ambiguity, and the entire Rust approval →
remote side effect → interruption → human reconciliation path. These do not
pretend to guarantee rollback of remote services or verified exactly-once
execution. External service-specific idempotency and task-resumption APIs
are possible next extensions, not assumed by the generic client.

### MCP focused validation — 2026-11-15

Commit b339afc358dde99dcaba1a8dd56ca94710e1ad27 passed all three independent
checks in https://github.com/hushmind3/dominion-rust/actions/runs/43689315045:
- Rust compile plus 79/79 unit/integration tests, 0 failures.
- TypeScript SDK suite, successful.
- Adopted main React frontend: npm ci, strict TypeScript typecheck and
  Vite production build, successful.

The MCP tests exercised a real local Streamable HTTP server, approved
side-effectful call cancellation, durable request-ID correlation, matched
progress notifications, transport ambiguity, and human reconciliation
without a second remote dispatch. The React check also removed the obsolete
Home dashboard's legacy input.prompt reference and distinguishes ongoing
MCP progress from paused reconciliation.

No real external MCP account, real-provider task success benchmarks,
interactive browser E2E, or end-to-end user data migration was performed
in this focused validation. These are remaining product-wide acceptance gates.

### Reliable source edits and test receipts

The native Rust agent now stages both `write_file` and `edit_file` atomically rather than
truncating existing source files, with fsynced content and preserved executable bits.
Approved shell commands with a nonzero exit become persisted tool errors that expose
stdout/stderr for agent repair and independent verification. These protections are shared
by the Rust SDK, REST API, TypeScript adapter and GUI because they enter the same Rust
`tools::execute` path. Refer to `docs/ARCHITECTURE.md` for recovery limitations.

### 2026 MCP tool discovery

The Rust MCP client now loads paginated `tools/list` results and follows server
`ttlMs`/`cacheScope` freshness hints instead of imposing an unconditional 60-second
cache. Missing or zero TTL means no cached discovery. This reduces stale tool
selection and respects the server's intended tool-list update behavior.

### RSI journal durability

RSI experiment states, scores and adoption evidence are persisted through synced,
atomic snapshots. Interrupted evaluations become durable failed records during
restart instead of remaining in-memory-only recovery markers. No interrupted
candidate is adopted without a new independent evaluation.


### Source-connected RSI with a separate worktree judge

RSI can now propose changes to 1-8 designated source files, retry up to three
times using real frozen-test feedback, evaluate on a separate Git worktree,
and pin accepted candidate commits until explicit adoption. Descendant trials
inherit the exact file set and persisted measured lessons; the original source
checkout is untouched before adoption. Rust SDK, REST, TypeScript and the
non-developer Studio screen all share this native execution path.

General Rust tests do not silently count an unavailable OS sandbox as a passing
RSI end-to-end test. CI explicitly runs two complete fixed-command, loopback
mock-model Git flows. Real-model task quality and production confinement remain
separate operational gates.


RSI adoption is now journaled as an explicit adopting state. If a process
stops between Git cherry-pick and durable completion, a subsequent action
checks the exact original parent commit and tree hash before finalizing,
rather than duplicating the Git mutation. A mismatched checkout requires
human inspection and cannot silently overwrite the repository.

### Native repository-wide RSI discovery (2026-11-15)

Starting an RSI trial with no `target_file` now enables goal-directed source discovery: Rust reads the checked-out Git source index, builds a bounded ranked manifest with source signatures, and asks the configured model to select up to eight relevant implementation modules. Files must match the advertised catalog and existing size/target protections. The selected paths, reason and actual provider charge are stored with the experiment and passed to the existing independent candidate/judge/adoption path. Explicit file lists remain supported. Studio defaults to automatic source selection, with manual files in an optional advanced mode; Rust REST and TypeScript SDK accept an omitted target field. The model is an advisor for scope only, while Dominion Rust owns source validation, git worktrees and evaluation. Current discovery is bounded and cannot select arbitrary new files outside the tracked Git catalog.

Per user direction, this branch's GitHub Actions workflow has been switched to manual dispatch. **No tests, CI or benchmarks were invoked for this discovery change.**

### Cross-session strategy learning and RSI transfer (2026-11-15)

The independent Rust runtime now derives a project-scoped strategy index from actual durable sources rather than duplicating an unverified memory database: **adopted** Git-evaluated RSI trials and **completed** agent runs whose independent verifier returned PASS and whose tool receipts are recorded. Rejected RSI candidates remain separate cautionary evidence; pending, interrupted, unadopted or claimed-only results never become successful methods. The RSI evaluator can use up to three relevant earlier examples when revising source files; the ordinary agent loop can use the same evidence in its initial plan and tool-driven continuation. A verified agent run can therefore inform a later RSI trial in the same repository, and adopted RSI lessons can inform the agent run between those trials.

After frozen RSI evaluation, a compact economy-model reflection may generate a structured *hypothesis* (principle, conditions, likely failure mode) using the real candidate patch and measured baseline/result. It is recorded with the experiment and actual API cost; reflection failure never changes the judge decision. Evidence is not an instruction override and is ranked by goal relevance and verified outcome. Historical trial records without a reflection remain available as labeled empirical examples. Project boundaries prevent automatic retrieval of another repository's history.

Rust SDK: `Dominion::search_strategies`. REST: `GET /api/v1/strategies?repository=...&query=...`. TypeScript: `dominion.strategies.search(repository, query)`. The RSI Studio exposes a user-triggered search and shows strategy origin, measured outcome, failure caveats and cost. The engine records a `strategy_evidence_loaded` event when planning reuses prior cases. The feature does **not** modify model weights.

No tests, benchmark or CI have been executed for this iteration, as instructed. GitHub Actions remains manually dispatched on this branch.

The operator switch `DOMINION_STRATEGY_LEARNING=false` disables automatic memory-based strategy injection and extra reflection model calls without deleting recorded evidence. The public configuration response and Studio settings view expose the effective switch.

### Main frontend source-of-truth sync — 2026-11-15

Latest main `879dc23d` was applied to **the entire `web/` Git tree** with no custom RustWorkspace UI or modified page components. This supersedes the earlier partial UI adaptation described in historical sections. The native Rust backend still exposes `/api/v1` and therefore does not yet satisfy main's existing `/api` routes. Exact tree comparison and outstanding API gaps are recorded in [FRONTEND_ALIGNMENT](docs/FRONTEND_ALIGNMENT.md). No CI, build or functional tests were executed for this frontend-only alignment.

## Native backend follows the unchanged main frontend (2026-11-15)

This supersedes the previous *frontend-only sync* and any historical statement that the legacy `/api/*` tree is entirely absent.

- **No frontend source edits**: main web Git tree stays `9e2be9b7376c1eefb6195e3ea7f9463b952456c0`.
- `src/frontend_api.rs` provides actual Rust-backed legacy API handlers. `src/frontend_state.rs` stores user-created agents and chat sessions in durable `frontend-state.json`, with atomic writes and strict size limits. The same `Engine`, `Store`, `ModelClient`, `Knowledge` and `OwnerProfileStore` handle execution, metering, memory and user edits.
- Home is wired to true status, model-provider state, agents, sessions, native runs, job list and execution-monitor receipts. Main `NativeAgentPanel` calls `/api/sdk/runs` and sees native checkpoint/approval/reconcile data.
- Main chat session creation/rename/fork/clear/cancel/history and SSE `/api/chat` are backed by native runs; answers are persisted even if the SSE viewer disconnects. Main agent profiles are durable and their system prompts are included in actual agent task inputs.
- `/api/approvals`, `/api/tools`, `/api/executions`, `/api/memories` and `/api/settings/owner-profile` use native stores. Remote access uses the same bearer or X-Dominion-Token middleware as `/api/v1`.
- **Fail closed on unavailable features:** dynamic provider editing, arbitrary SDK verification commands, per-chat tool-disabled mode, unimplemented reasoning toggles, RSI campaigns, workflows, full room/video workflows and other missing endpoints return explicit HTTP errors. The old main UI is never swapped for a pretend replacement.
- The default native HTTP bind is **127.0.0.1:4317** to agree with the unchanged main Vite proxy. The original main `--home` and `--open` server flags are now recognized. Other hosts/ports remain configurable through `DOMINION_BIND`.
- `sdk/typescript` includes typed calls for the same main UI agent/session/provider/execution routes as well as the independent native Rust API.

Read [the exact main frontend contract mapping](docs/FRONTEND_RUST_CONTRACT.md) for implemented endpoints, unsupported operations and known limitations. This stage is source-implemented but **not build/test/CI verified** by the user's instruction.

### 2026-11-15 — Main workflow page actually executes Rust runs

The original main `web/src/pages/Workflows.tsx` is kept unchanged. `src/workflow.rs` persists real workflow definitions and checkpoints in `workflows.json` (atomic fsync+rename). `src/frontend_workflows.rs` provides `GET/POST /api/workflows`, `DELETE /api/workflows/{id}`, `POST /api/workflows/{id}/run`, and the existing `/api/jobs` interface now includes workflow progress, cancellation and recovery (`POST /api/jobs/{id}/resume`). Every step is a real independent native Rust Run with actual agent prompts, `{{input}}`/`{{previous}}` substitution, model/tool receipts, approval, token/cost tracking and durable outputs. Overall workflow model budget defaults to 10,000 KRW; unsupported model selections fail rather than silently substituting models. Completed workflows create a linked real chat transcript for the original main View Result button. The process-restart checkpoint retains the existing native step ID instead of duplicating a mutating tool call. This code is a source-level integration; compilation, browser runs and CI were not performed per user instruction.

### Main chat control parity — tool-free and per-run learning (2026-11-15)

The original main Chat UI now selects between (a) native agent/tool execution and (b) a real no-tools provider turn when its toolsEnabled or toolsExecutionEnabled switches are off. Model-only runs are stored with exact token charges, cancellation-safe terminal state, and session transcript integration; they never enter the tool dispatcher. Both paths persist per-run strategy_learning, which disables strategy retrieval and downstream evidence extraction for that run when autoLearning=false. Operator-level learning disable remains authoritative. Unsupported reasoning-effort overrides still reject rather than silently apply a different setting. The frontend source is unchanged. No compilation, tests or CI were executed during this change.

### Native RSI campaign integration (2026-11-15)

The unmodified main RSI screen now uses Rust `/api/rsi/start`, `/api/rsi/campaigns`, `/api/rsi/campaigns/{id}`, `/choose`, `/verify`, `/apply`, `/api/algorithms` and `/api/rsi/evaluation-profiles`. Actual `Improvements` produces Git-isolated source candidates, executes frozen baseline/candidate scoring, records actual model token usage, gates adoption on score evidence, and creates later evaluated generations from the adopted checkout. Durable `rsi_campaigns.json` stores attempts, candidate/selected Git commits and recovery states, without synthetic meta-evaluation claims. To enable actual source edits, set `DOMINION_ENABLE_SHELL=true`, `DOMINION_RSI_REPOSITORY` (workspace-relative clean Git checkout), `DOMINION_RSI_TEST_COMMAND` and `DOMINION_RSI_SCORE_COMMAND` (numeric output, higher means better). Missing judge/source configuration returns a real API error. Source adoption does not mean the running binary has been rebuilt. Unsupported cross-generation performance claims are intentionally omitted. This is source-level integration; build/tests/CI have not been invoked.

### Main Extensions UI ↔ native MCP and skills (2026-11-15)

The original main `/api/mcp` and `/api/skills` flows now persist server and skill registrations in owner-permissioned `extensions.json`. Enabled Streamable HTTP MCP servers are immediately published to the native `Mcp` runtime, with configuration-generation checks preventing stale discovered tools from being called after changes. Existing running MCP calls keep the original frozen server target and must still follow the native approval/reconciliation journal. User-created Unicode-named skill templates are exposed through stable `managed_<UUID>` aliases in `skills::list` and can be loaded on demand by the native agent without executing embedded instructions as code. MCP secrets are excluded from API responses, HTTPS/loopback URL rules remain enforced, and arbitrary stdio installer commands or unverified repository imports are explicitly rejected. This is source code integration; no CI/build/test run was invoked.

## Sub4 autonomous multi-scene ComfyUI production (2026-11-16)

The Video > 자율 콘텐츠 surface now accepts ComfyUI durations from 3 to 90 seconds. The Rust production orchestrator creates 1-12 sequential ComfyUI scene jobs, persists the child IDs and completed-shot count in the existing production journal, verifies source footage with ffprobe, then normalizes and concatenates the clips with FFmpeg into an MP4. Its production ID is the final video identity for the existing preview, reveal, library and YouTube upload APIs. Native FFmpeg jobs and historical ComfyUI single-shot IDs remain compatible. Changes are exclusive to the dominion-sub4 branch.

Failure boundary: an interrupted remote scene or upload requires review rather than an unobserved retry; finished clips remain in the ComfyUI journal. The script-model KRW cap does not include local GPU, electricity or storage costs. ComfyUI footage currently contains no scene-synced TTS, identity-reference conditioning or targeted shot regeneration. Full browser/video-render checks and real model inference should be independently verified on the target machine.

2026 references: https://github.com/Comfy-Org/ComfyUI/blob/master/comfy_extras/nodes_video.py and https://github.com/Comfy-Org/comfy-skills/blob/main/skills/comfy-generate-video.md .

## Sub4: durable scene repair and visual continuity (2026-11-16)

The autonomous video planner now emits an optional shared visual_bible that is included in each ComfyUI scene prompt. Invalid storyboard JSON or scene counts are corrected via up to two bounded model retries. All retries are charged and persisted as scriptAttempts/costKrw before further work; no video rendering starts when the configured script-model spend is exceeded.

After the first multi-shot MP4 is ready, the Video Studio lets users regenerate one selected ComfyUI shot. Successful revisions retain other scene videos, render a replacement clip, create a NEW immutable output file and switch artifactRevisionId in the durable journal only after FFmpeg/ffprobe validation. The previous finished MP4 remains accessible if the new render fails. The existing product video ID remains stable for viewing/downloading; YouTube upload is blocked while a revision is in progress.

Interrupted revisions retain the remote ComfyUI prompt receipt ID, scene index and prompt. The next explicit revision request attempts a GET-only remote-history reconciliation: an already completed clip can be reused, a confirmed terminal failure permits a new submission, and unknown outcomes remain blocked rather than paying for an accidental duplicate. Direct model weights and original scene files are not modified.

2026 frontier context: official Comfy LTX-2.3 FLF2V workflows expose first/last-frame guidance and controlled motion (https://comfy.org/hub/workflows/ltx-2-3-flf2v-bb33cc433026/); MultiShotMaster (CVPR 2026) addresses multi-shot consistency (https://openaccess.thecvf.com/content/CVPR2026/papers/Wang_MultiShotMaster_A_Controllable_Multi-Shot_Video_Generation_Framework_CVPR_2026_paper.pdf). Sub4's visual_bible is text-only and does not claim frame-conditioned character consistency or synchronized generated speech. Shot-level revisions target lower wasted GPU work; actual GPU costs and quality require matched measurement.

## Sub4 Rust context-efficiency refinement (2026-11-16)

The Rust engine uses bounded rule-based compaction of historical tool-call arguments before model calls, while retaining function names, call IDs and paired tool receipts. Large old function arguments are replaced with valid JSON markers in the outbound model projection; the durable execution transcript is unchanged. Older preserved user directives are separately bounded and projected-character savings are recorded in the native run telemetry. This targets unnecessary repeated prompt traffic, not a claimed token-billing discount until measured with a real provider. Informed by the 2026 harness ablation study: https://arxiv.org/abs/2609.20804 .
