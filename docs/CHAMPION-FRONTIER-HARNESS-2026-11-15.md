# Champion: frontier harness, source-derived algorithms and verified RSI (2026-11-15)

## Purpose and implementation boundary

DOMINION owns the **Rust model/tool loop**, durable goals, review, permissions,
SQLite checkpoints, worktree isolation, independent RSI evaluator and decisions.
The TypeScript SDK is a **typed transport**, not a replacement planner or a
vendor SDK wrapper. Existing video, multi-model rooms, MCP and owner controls
remain on their established runtime paths.

## Actual source and architecture comparison (reviewed 2026-11-15)

| System/source | Relevant execution mechanism | Advantage | Limitation / Dominion adoption |
| --- | --- | --- | --- |
| [OpenAI Codex / codex-rs/core/src/tools/router.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/router.rs) | Finalized tool registry and model-visible specs, dispatch, tool-mode routing | Strong schema/runtime seam and bounded advertised authority | DOMINION retains its own selection and authorization; strengthen complete coding-operation coverage under schema caps |
| [Qwen Code / architecture](https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md) | UI-independent core, ACP bridge, persistent workspace daemon, TypeScript SDK | Same core callable through multiple clients | Avoid adding ACP/daemon to the inner Rust decision loop; preserve native /api/sdk endpoints |
| [OpenHands / agent.py](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/agent.py) | Agent step, pending actions, parallel batches, dynamic context condensation, observable events | Clear action/observation and parallelism boundaries | Keep DOMINION's durable effect receipts and isolated source evaluation; no Python runtime dependency |
| [ByteDance Trae Agent / tools](https://github.com/bytedance/trae-agent/blob/main/docs/tools.md) | Range-based editing and persistent shell with explicit tool boundary | Practical observe-edit-execute workflow for code | Do not replace Dominion's permission checks, source SHA or completion gates |
| [GEPA, ICLR 2026](https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html) | Reflective error feedback and Pareto selection across real trajectories | Counterexamples can improve subsequent methods without weight updates | Only **measured and independently replayed** lineage is eligible to influence a descendant |
| [GEPA-Curator, October 2026](https://arxiv.org/abs/2610.37651) | Failure-signal curation and retrieving prior successes | Avoid learning from failed infrastructure / noisy rollouts | Implement verified-sample filtering and bounded accepted-success retrieval; do not infer causal transfer |

The above are research/code references; no third-party agent policy or execution
engine is imported. The specific upstream revisions are not pinned in the
DOMINION build and upstream comparison is not a reproduction benchmark.

## Product execution call map (existing, preserved)

- Native SDK: `src/engine/agent_sdk.rs::DominionSdk` -> `chat::run_turn`;
  `src/api/sdk.rs` -> `/api/sdk/threads/*`.
- TypeScript: `sdk/typescript/src/client.ts` -> HTTP/SSE -> same Rust loop.
  Native Rust client: `sdk/rust/src/client.rs`.
- Durable coding goals: `src/api/goals.rs` -> `jobs` ->
  `src/engine/goals.rs` -> `goal_git` / `goal_navigation` ->
  `chat::run_quiet_outcome` -> `verification` -> next turn / review.
- Model tools: `src/engine/chat/runtime/tool_selection.rs` ->
  advertised schemas and permissions -> `tool_execution` and persisted
  tool receipts. Generated source is never sufficient to claim success.
- RSI: `rsi/execution/worker.rs` -> `execution/guards.rs::fit_worker_prompt`
  -> `learning/failure_feedback.rs::reflective_feedback` -> native
  model/tool edit -> separate worktree -> frozen independent pair scoring ->
  verified adoption -> recompiled descendant and method lineage.

## Implemented in this revision

1. **Execution-chain-aware tool schema packing.** When the model tool-schema
   byte cap is below the full catalog, preserve the smallest *advertised*
   schema for read, edit, command and diff as a complete coding chain,
   **if all four fit**. Fill residual budget with relevance-ranked tools
   and keep original declaration order for provider prefix caching.
   No tool is synthesized and no approval or capability is granted.
   If the complete chain cannot fit, use the previous ranked fallback;
   a hard provider budget remains in effect.

2. **Measured RSI evidence curation.** Select the latest 20 *measured*
   revalidated generations, not 20 most recent records before filtering.
   Unmeasured and forged episodes cannot evict old verified regressions.
   Deduplicate individual case+metric counterexamples; do not reuse
   solved/saturated quality targets as failure signals.

3. **Verified successful-pattern retrieval.** At most two relevant cases
   from adopted, eligible, independently revalidated generations are
   included in the new compiled descendant's prompt. The case has an
   actual directional measured improvement and a recorded accepted
   Git commit; unadopted, unmeasured or corrupt-commit inputs are skipped.
   Every example is marked `transferVerified:false`: a past gain is
   evidence of **that** task, not guaranteed future improvement.

4. **Bounded delivery and telemetry.** `fit_worker_prompt` sends up to
   two success examples in full context, one compact example when
   token budget requires it, and none in deferred mode. Context plan
   records `adoptedSuccessExamplesDelivered` and
   `adoptedSuccessExamplesAvailable` alongside counterexamples. This
   is genuinely consumed by `worker.rs`, and is preserved in the
   generation receipt `workerFeedbackPlan`.

## Static and executable verification

Focused regression tests are embedded in:
- `src/engine/chat/runtime/tool_selection.rs::coverage_tests`: complete
  read-edit-test-diff availability, budget containment, only offered
  schemas and noncoding policy compatibility;
- `src/engine/rsi/learning/failure_feedback.rs::tests`: measured-tail
  starvation, independent adoption, rejection and forged-evidence filtering;
- `src/engine/rsi/execution/guards.rs::curated_worker_context_tests`:
  accepted success and independently measured failure actually enter the
  compiled descendant's prompt and plan.

Run on a Rust-capable machine:

```bash
cargo test --release coverage_tests
cargo test --release failure_feedback
cargo test --release curated_worker_context_tests
cargo test --release
npm test --prefix sdk/typescript
cargo test --manifest-path sdk/rust/Cargo.toml --all-targets
```

For *real* coding completion, use `scripts/benchmark/swebench_dominion.py`
with the exact official SWE-bench Verified instances, real model/provider,
task-isolated workspaces and official Docker scoring as specified in
`docs/FRONTIER-SDK-RSI-2026-11-14.md`.
Record resolved %, actions, tokens, cost, latency, repetitions and
needs-review rate; pin the same task and model budgets for any competitor.

### Evidence boundaries

These commits are **source and test additions**, not an observed
SWE-bench improvement or demonstrated live-model outcome. This chat
environment has no Rust toolchain or repository clone network access;
`cargo test`, service startup, actual model calls, benchmarking,
workflow CI and end-to-end recovery were **not executed here**.
Do not infer functional performance, total savings or readiness from a
successful GitHub content update. There is no LoRA, finetuning or
model-weight training in the RSI path.
