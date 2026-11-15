# Dominion vs 2026 frontier agent harnesses — implementation map

Review date: 2026-11-15. This is a **source-path capability comparison**,
not a leaderboard score or a claim of equal performance. Each reference is
a production project or an official architecture/code source inspected
for an execution mechanism, not a list of marketing feature names.

| Reference | Inspected mechanism | Frontier target | Dominion current path | Remaining gap |
| --- | --- | --- | --- | --- |
| OpenAI Codex Rust | codex-rs/core/src/compact_token_budget.rs, apply_patch.rs, session and approval layers | Persistent threads, context-lifecycle events, structured source edits, configured permissions | src/context.rs, src/engine.rs, src/tools.rs | Sandbox paths exist but need adversarial OS tests; provider-aware compaction quality metrics, quotas and mature thread lifecycle are missing |
| Claude Code (Anthropic, 2026) | Skills, hooks, CLAUDE.md, context-isolated subagents | Deterministic hooks and selective instruction loading; isolated delegated work | src/engine.rs, src/sdk.rs (isolated child runs, user directives), src/workspace.rs | Lazy SKILL.md exists; comprehensive deterministic hooks and policy-controlled procedural lifecycle are missing |
| DeepSeek Harness | `packages/core/tools/src/batch.ts`, `pipeline.ts`, `json-schema.ts`; scoped tool and agent loop layers | Bounded read pool, step-scoped admission, ordered tool outcomes, pluggable driver | `src/tool_batch.rs`, `tool_schema.rs`, `journal.rs` and engine's real queued-tool path | Advanced hook chains, full JSON Schema vocabulary, cancellation signals and swappable decision driver remain incomplete |
| Qwen Code Agent Teams | Concurrent read-only investigators, owner-controlled writer worktree, message/task board | Separated investigation/writing and conflict-free integration | src/policy.rs enforced investigator roles; src/engine.rs escrow settlement; src/workspace.rs approved, conflict-checked patch adoption | No shared task board, strategy-level role scheduling or measured conflict repair |
| OpenClaw | src/agents/agent-command.ts and agent-bundle-mcp-* runtime | Multi-channel integration, resilient MCP registration, session admission | src/mcp.rs HTTP tools with approval; src/store.rs durable run files | Broader channel adapters, stdio MCP, admission/connection lifecycle |
| NousResearch Hermes | agent runtime, persisted state and skills ecosystem | Lifelong memory, skills, autonomy across sessions | src/knowledge.rs durable project-scoped lexical retrieval; src/skills.rs lazy SKILL.md; inherited RSI reflections | No semantic memory index, automated skill refinement or measured transfer |

Source links:

- https://github.com/openai/codex/tree/main/codex-rs/core/src
- https://developers.openai.com/blog/codex-as-a-platform
- https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md
- https://qwenlm.github.io/qwen-code-docs/en/users/features/multi-agent-coordination/
- https://github.com/QwenLM/qwen-code/tree/main/packages/core/src
- https://github.com/openclaw/openclaw/tree/main/src/agents
- https://github.com/NousResearch/hermes-agent

## Required parity gates (NOT yet passed)

1. Large-repository real-model task completes: inspect, edit across files,
   run actual tests with approval, repair failed tests, independently verify,
   and produce a reviewable Git diff and artifact.
2. User's late instruction wins while a model request or verifier is in
   flight. No stale result may enter the accepted plan or final answer.
3. Context under stress: user constraints/tool receipts retained, token usage
   and actual provider cache billing measured, quality and correctness do not
   drop relative to the full-history baseline.
4. Delegated workers cooperate via bounded budgets, isolated worktrees,
   explicit results and a conflict-checked integration strategy.
5. Permission policy, OS-level sandbox and resource isolation are measurable.
6. Sessions, skills, memory, channels and MCP live within a coherent Rust
   lifecycle; UI, Rust SDK and TypeScript API agree on current state.
7. Group conversation, video backends, RSI worktree evaluation and exact
   evidence all work end to end without feature loss or stale mock paths.
8. After the above, run matched-condition coding evaluations (including
   SWE-bench Verified) with identical model, token budget, tools, timeouts
   and judge to compare success, tool calls, cost and recovery.

## This iteration: code rather than a checklist

- Native SDK in src/lib.rs and src/sdk.rs, executable bound through the SDK.
- Source Git checkout attachment from SDK/API/GUI via src/workspace.rs.
- Exact-match code editing, Git status/diff in src/tools.rs.
- Non-destructive context projection preserving directives/tool pairs in
  src/context.rs, called by the real model loop.
- Stale model-plan/response/verifier rejection when user intervention races.
- Subagent tool execution with lineage, sibling budget reservation, Git
  revision inheritance, and parent-inspected child artifacts.
- Provider-reported cache token cost split, actual KRW rate configuration,
  saved runtime counters surfaced to the UI.

No new goals.rs or goals.api file is introduced.

## Evidence levels

- **Source-connected:** features listed above have linked Rust/API/SDK/UI
  entrypoints, with selected new unit tests.
- **Executed for focused Rust tool orchestration:** Rust compilation and 64/64 automated tests,
  plus optional TypeScript SDK success (Actions 43686024696).
- **Not yet executed:** full UI browser simulation, real-provider tasks and public coding benchmarks.
- **Not yet achieved:** frontier-level measured task performance, complete
  sandbox/skills/memory/channel ecosystem, mature long-run task repair,
  multi-worktree merge, premium turnkey video quality.

Avoid calling a feature production-ready based on the presence of one
route, a named module or a mock model test.

## Main frontend preservation

The sub4 branch now contains the complete main React 19/Vite source tree and mounts the original workspace navigation shell and Home dashboard, including a native Rust-backed `NativeAgentPanel`. `web/src/pages/RustWorkspace.tsx` is the v1 API-connected surface for real Rust operations; the former ad-hoc static HTML/JS frontend was removed. Main-only pages remain as preserved source, not as claims that the main SQLite/Tauri backend has been migrated. See `docs/ARCHITECTURE.md` for the mounted routes and `README.md` for production build/development commands. Browser/e2e parity not yet verified.

## New verified-by-source execution surfaces (not benchmark parity)

- Durable delegated budget escrow and settlement, read-only workers, parent wake after active children complete: `src/policy.rs`, `src/domain.rs`, `src/engine.rs`, `src/store.rs`.
- Human-approved, conflict-checked worktree integration, including newly created files: `src/workspace.rs`, `src/tools.rs`.
- Transient provider retry and test fixture, 10k-file indexed listing, pagination, line-range source reads: `src/provider.rs`, `src/tools.rs`.
- Project-scoped non-weight memory and lazy SKILL.md loading through real model/tool/API/SDK paths: `src/knowledge.rs`, `src/skills.rs`.
- Linux bubblewrap and macOS Seatbelt fail-closed OS shell isolation for approved agent and RSI scripts: `src/sandbox.rs`.

Current runtime still needs a compatible OS sandbox backend to execute shell/RSI commands; untested sandbox paths do not count as production-ready.

## Source audit: post-hardening runtime

- `src/verification.rs` accepts only a strict PASS verdict; `src/engine.rs` sends real run tool receipts, Git status and actual Git diff into the verifier, rather than treating model-written completion claims as proof.
- `src/engine.rs` owns an event-driven queued parent wake scheduler with no mutually-recursive async launch path. `src/sdk.rs::wait` now waits through deliberate child-work suspension, while a generic paused error still returns promptly.
- Cancellation during external tool execution is persisted with `cancel_after_tool`, and only reaches a terminal state after a durable tool receipt or human reconciliation; completed child budget settlement excludes active or uncertain tool operations.
- `src/knowledge.rs` persists a copy before publishing memory in-process; a failed filesystem rename does not create a phantom memory.
- `sdk/typescript/src/index.ts` mirrors native read-only delegation, memory, skills and run usage fields; mock HTTP tests added without invoking CI.

None of these static integrations establishes measured frontier task success. Required Rust compilation, OS sandbox/real-provider end-to-end tasks, real app navigation, group/video/RSI quality gates and matched-condition coding benchmarks remain pending.

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
The scoped Rust orchestration implementation compiled and passed 64/64 tests
(Actions 43686024696). This does not establish a product-wide frontier benchmark result.
## External reference for this focused milestone

DeepSeek Harness 2026 session behavior distinguishes TOOL_NOT_STARTED
from TOOL_OUTCOME_UNKNOWN and refuses blind repeat of unknown effects:
https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/session/README.md

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


## RSI capability evidence and frontier gap (2026-11-15)

The RSI path now supports bounded multi-file generation with real test-failure
feedback, a clean third-party judge worktree at the recorded Git revision,
score-controlled adoption, stable accepted commit pinning, and descendants.
The explicit integration tests exercise an actual Git repo, local model HTTP,
frozen commands, retries, distinct worktrees and post-approval adoption.
See src/improvement.rs::full_cycle_tests and the targeted Rust workflow step.

This improves candidate-generation and evaluation fundamentals but remains
well short of general autonomous full-repository self-improvement, open-ended
module creation, strategy search, cross-domain A-to-B-to-A refinement and
matched real-model SWE-bench Verified evidence. Worktree tests isolate code
changes, but do not constitute a hostile OS sandbox or prevent all forms of
environmental benchmark contamination.


Crash-recovery delta: RSI explicit adoption now commits a durable adopting
state, serializes adoption operations, verifies parent/tree identity after
an interrupted cherry-pick and can finalize the existing Git change after
restart. The multi-file end-to-end fixture simulates this interruption.

### Next frontier capability: source discovery instead of manually named RSI files

The Rust RSI path now has bounded Git-indexed module discovery, goal-sensitive path ranking, context-efficient signature sampling and model-driven selection validated against the source index before candidate generation. UI defaults to discovery; TypeScript and REST allow omitting target_file. This closes the manual-target dependency while retaining current strict worktree evaluation and adoption. It is not yet unrestricted autonomous repository restructuring, cross-domain A-B-A adaptation, real-model performance validation or a complete replacement for plan-act tooling. CI has not been run for this step on user request.
