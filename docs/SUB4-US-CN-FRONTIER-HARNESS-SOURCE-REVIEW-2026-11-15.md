# DOMINION Sub4 — 2026-11-15 source-level harness comparison and implemented adoption

This review inspected source in the repositories below (not only READMEs), against the independent native Rust engine in `src/engine/`. Repository sources evolve, so links are to upstream source paths; the checked state is the 2026-11-15 investigation. No third-party agent execution engine is embedded.

## Source comparison and integration decisions

| Ecosystem | Actual inspected source | Operational strength | Important limitation for DOMINION | Native Rust adoption |
| --- | --- | --- | --- | --- |
| US — OpenAI Codex | [`codex-rs/core/src/codex_thread.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/codex_thread.rs) | Separate session/thread state, typed SteerSubmission/RecoverTurnRequest, cancellation, permission/environment snapshots and runtime event flow | Cannot delegate DOMINION's reasoning/owner authority to Codex's own thread or SDK | Existing `src/engine/goal_steering.rs` + `src/engine/goals.rs` use durable owner revision, CAS terminal seal and preemption at safe Rust execution boundaries |
| US — Anthropic Claude Agent SDK | [`src/claude_agent_sdk/client.py`](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/client.py) | Stateful bidirectional interaction, streamed events, interrupts, transport abstraction, resumed interactive sessions | Its default client controls an external Claude Code subprocess; importing it would replace native execution ownership | Native `sdk/rust`, optional `sdk/typescript`, and Rust `/api/chat`/goal lifecycle; React builds separately and uses its browser HTTP/SSE transport |
| US — GEPA | [`gepa/strategies/candidate_selector.py`](https://github.com/gepa-ai/gepa/blob/main/src/gepa/strategies/candidate_selector.py), [candidate-selection guide](https://gepa-ai.github.io/gepa/guides/candidate-selection/) | Per-validation-case Pareto coverage, best/epsilon-greedy/top-K candidate selection and trace-grounded reflection | Per-key frontier is not necessarily the mathematical non-dominated Pareto set; model-written/unpaired scores are not safe RSI proof | `src/engine/rsi/meta/experiment_evolution.rs` now rewards **distinct verified holdout cases** in Explore/Factorial/Repair choice, only after native independent paired eligible revalidation; frozen adoption gate unchanged |
| China — DeepSeek Harness | [`docs/architecture.md`](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md), [`packages/core/agent-loop/src/index.ts`](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/index.ts) | Cordis plugin service composition, durable event log and next-step/turn inbox; agent factory awaits teardown/settlement | Built around a Node/Cordis runtime; direct adoption would violate Rust-native engine independence | `src/engine/chat/runtime/turn.rs` and `src/engine/goals.rs` implement explicit owner next-round yielding, durable checkpoints, verified tool operation receipts and no automatic duplicate external-effect replay |
| China — ByteDance DeerFlow | [`backend/packages/harness/deerflow/sandbox/sandbox_provider.py`](https://github.com/bytedance/deer-flow/blob/main/backend/packages/harness/deerflow/sandbox/sandbox_provider.py) | Explicit acquire/get/release lifecycle; thread/user scoped sandbox lookup, async lifecycle wrappers and skill projection sync | Sandbox support depends on provider and Python environment; shell/host surfaces can bypass skill views | Existing native Rust `goal_git.rs` isolated Git worktree and explicit owner diff-digest review/apply; avoid host-dependent Python sandbox as mandatory runner |
| China — AgentScope | [`src/agentscope/agent/_agent.py`](https://github.com/agentscope-ai/agentscope/blob/main/src/agentscope/agent/_agent.py) | Structured agent state and distinct model/tool/reply/user interrupt events; async tool/result streaming | AgentScope's own Python runtime would compete with DOMINION's independent execution loop | Existing Rust chat/gateway events, typed `program.rs` IR and independently selected tool policies; compare event granularity rather than transplant interpreter |

## A real source-level regression found and corrected

The native typed execution program `src/engine/chat/runtime/program.rs` was treating a failed `shell_exec(exit 7)` as an unrecoverable operation *before* the immediately following typed conditional could consume `result.exitCode`. This broke two preexisting real HTTP+tool E2E fixtures:

- `a_typed_execution_program_takes_the_failure_branch`
- `a_program_consumes_sequence_verify_loop_and_model_decision_nodes`

The recovery patch makes a **decidable immediately adjacent branch that explicitly references the failed observation** a legitimate failure handler. The actual failed receipt remains durable, its operation is not replayed on continuation, and the true/false branch runs through the ordinary Rust tool gateway. A missing or invalid predicate stays unresolved and is **not** recorded as handled. Explicit `onFailure:revise` remains a higher-priority instruction to repair.

Nested `sequence`, `branch`, `parallel` and `loop` nodes now propagate child failures and preserve completed/sibling receipts. Parent sequence/loop completion is not fabricated when a child fails. Source: `src/engine/chat/runtime/program.rs`; tests: its unit module, and `tests/e2e.rs::a_nested_program_failure_propagates_to_durable_repair`.

## RSI innovation, executable not a paper-only analogy

Existing native RSI architecture in `src/engine/rsi/` already has model-driven source edits, independent detached worktree evaluator, accepted/rejected paired receipts, recompiled descendants, versioned experiment strategy genome, and bounded selection. It is **source and strategy evolution, not model weight updates**.

New method: for each arm's original independently measured, eligible, revalidated and accepted result, extract the set of frozen validation cases showing a genuine gain without a regression. Reward solving **uniquely covered cases**, less credit for shared wins, capped per arm and normalized by measured attempt count. This alters the **compiled successor's executable choice** (Explore/Factorial/Repair) and not the benchmark evaluator or adoption authority. Missing sources, model stories, unchanged guards and cases with regressions grant zero extra coverage.

Tests: `tests/unit/rsi_meta_experiment_evolution_tests.rs` validates per-case strategy selection, lineage, ineligible-source rejection and regression exclusion.

## Source-provenance and extension contract

- **Rust-native engine** runs model/tool/plan/review; no Codex, Claude, DeepSeek or AgentScope agent runtime is linked.
- **Rust SDK** (`sdk/rust`) is a first-party external Rust client + Rust stdio MCP tool host. It is not a claim of embedded in-process agent engine.
- **TypeScript SDK** (`sdk/typescript`) is an optional published-style *external JS client*. React (`web/`) communicates to Rust through `web/src/api.ts` and `chat-stream.ts`, no SDK dependency in the shipped UI. Node/Vite is the frontend build tool.
- User configuration retains control of approved tool execution and external model selection. Model-generated plans and tool outputs are never rewritten into authenticated owner messages.
- Failure receipts, checkpoints, parsed SSE and owner revisions must survive interruption without spuriously replaying writes.

## Evaluation status and reproducibility

The repository has runnable `src/bin/dominion-swebench.rs`, `scripts/export_swebench_verified.py` and `docs/SWEBENCH-VERIFIED-RUNBOOK-2026.md`. These launch **DOMINION Rust** against fresh cloned tasks, produce patches and token/tool/time/KRW manifests, and can optionally run the official Docker evaluator. The 500-instance verified benchmark score requires executing all 500 under fixed model/timeout/token limits and checking official resolved/unresolved IDs. At the time of this review **a real-model full-500 submission has not been run**; a passing harness workflow or source compilation is not a SWE-bench score.

The immediately preceding Rust CI (head `329493e`) compiled 914 unit tests successfully but failed the two program recovery E2E cases above (97/99 E2E pass). The latest source change should be judged from its own commit-specific CI runs; do not treat superseded cancelled workflow runs as passing or failing independent evidence.

## Forward bottleneck (not claimed complete)

A fair frontier comparison requires a matched full real-model coding benchmark and per-task manifests including success, accepted patch/test, tool failures and recovery count, tokens, wall-clock, cached tokens and KRW pricing. GEPA-like per-case diversity is a **hypothesis for more effective RSI exploration** until recompiled successors beat an independently measured and matched base agent. Frontier equivalence is not established by SDK transport, number of files, source review, or synthetic scripted-model tests.

## 2026-11-15 follow-through: native execution fixes, not imported agent wrappers

The comparison above exposed two specific **production-control** risks in native multi-agent orchestration:

1. **Subagent quota race:** `src/engine/tools.rs::delegate` previously counted prompt jobs and created child sessions/jobs in distinct writes. That lets concurrent turns overrun both session lifetime and active-worker caps. `src/db.rs::Store::reserve_delegation` now uses an IMMEDIATE SQLite transaction to validate the parent's persisted cancellation and insert the session plus job under a single quota reservation. The normal existing Rust job worker, read-only delegation tool policy, cancellation and tool-result receipts still perform execution. Concurrency regression tests use 12 threads and two independent SQLite connections.
2. **Lost delegated billing scope:** `Store::session_usage(root)` only summed direct root model calls. A task could remain below its displayed goal token budget while many paid subagents ran. The `src/db/usage_tree.rs` recursive CTE now measures root+descendant session usage (with cycle defense) and is used at root executor, planner/reviewer, child-job and child-reservation admission, plus the final report. These are observed usage limits, not exact pre-billing reservation; test fixtures isolate unrelated sessions and verify the child gate returns `needs_review` before any provider request.

The implementation follows **source provenance over branding**: typed Rust state and original SQLite receipts provide the decisions, the user owns approval/limits, and an external TypeScript SDK never becomes the agent's control plane. For RSI, the existing DGM/GEPA-style `rsi/lineage/archive.rs`, `rsi/meta/experiment_evolution.rs`, `rsi/learning/negative_transfer.rs` and `rsi_prequential.rs` already gate evolving strategies on independently revalidated paired source measurements. The new control-plane source can itself be modified and assessed by that pipeline; mere changes to RSI metadata or an externally produced score cannot substitute for adopted descendants.

Verification status for this follow-through: changes committed to `dominion-sub4`; Rust compile/unit/E2E CI and live-model/SWE-bench results must be read from the **specific latest HEAD**, not from earlier successful/cancelled workflows. No new SWE-bench pass rate, cost reduction or RSI gain is claimed based on this static source audit.
