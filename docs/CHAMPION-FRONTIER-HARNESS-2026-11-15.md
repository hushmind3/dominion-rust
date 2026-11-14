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

## SDK capability negotiation (2026-11-15)

A read-only `GET /api/sdk/info` advertises the server's native `apiVersion:1`,
transport protocols, runtime capabilities and critical retry semantics:
run POST is **not idempotent**, losing an SSE socket does **not** cancel
the server run, and read-only event replay GET is idempotent.
`src/api/sdk.rs::info` is connected through `src/api/mod.rs`;
`sdk/typescript/src/client.ts::sdkInfo()` and
`sdk/rust/src/client.rs::sdk_info()` consume the same contract.
A server can evolve optional features without a TypeScript SDK guessing
whether a method exists. This is a capability-discovery contract, **not**
a new agent executor or evidence that a live model ran.
The TypeScript and Rust SDK suites each include a mock HTTP integration
asserting the GET route and execution semantics.

## 2026-11-15 continuation: rejected-candidate RSI and benchmark trace correctness

**Crucial provenance distinction:** `rsi_system/replay.rs::revalidated_history` sets `pairedReceiptReplayed` after verifying the original independent paired candidate receipt **even for a rejected candidate**, while `pairedRevalidated` means the experiment has been **adopted**. Previously `failure_feedback.rs::measured` required the latter, preventing rejected experiments from teaching the next worker. The corrected decision is:

- failure/counterexample eligibility: `pairedMeasured && pairedReceiptReplayed` plus real per-case metrics and source commit;
- prior-success retrieval: the above **and** `accepted && pairedRevalidated && eligible` plus accepted commit provenance.

An independent rejected regression now appears in next-generation diagnostic input without promoting it to the selected source. Tests explicitly cover this status difference.

**Benchmark fixes:** `scripts/benchmark/swebench_dominion.py` resolves `--output` before running the official evaluator in that output directory. It supports two source-verified official layouts (`logs/evaluation/<run_id>/results.json` and `<model>.<run_id>.json`) with the exact run ID; verifies the entire submitted cohort, per-status ID lists and count consistency; rejects conflicting duplicate reports. It counts failed and uncertain tool operations rather than labeling only completed operations as all calls. The receipt records `failedToolCalls`, `successfulToolCalls`, `uncertainToolCalls`, `observedFailureCases`, `observedRecoveryCases`, `observedFailureRecoveryRate`, with unknown metrics kept null when telemetry cannot be read.

[Draft PR #11](https://github.com/hushmind3/dominion-rust/pull/11) isolates CI/review from automatic main-branch merge. No remote Docker evaluation, paid model inference or local Rust compilation was performed by this chat.

## Reproducible SWE-bench evidence manifest

The benchmark runner writes `run-manifest.json` with a SHA-256 of the
exact native binary, JSONL task bytes, ordered instance IDs and run limits.
The final summary also cites these hashes. It records *which provided rows*
were run; it cannot authenticate that a user-supplied JSONL originated from
the official dataset without a trusted external reference. Use the official
SWE-bench Verified export and retain its provenance.

`scripts/benchmark/test_swebench_dominion.py` includes an intentionally
**synthetic, offline, missing-repository** end-to-end CLI check. It asserts
that a relative output path creates correct outputs and a fingerprinted
manifest while producing **only `setup_failed` with no resolved score**.
This is test coverage of evidence honesty, not an AI benchmark result.

The run directory distinguishes model calls, successful / failed / uncertain
tool receipts, recovery after measured tool failure, and official Docker
resolved IDs. Internal goal completion is not a substitute for the official
grader. Real model results still require a configured provider and runtime.

## Continuation: complete provider boundary, paid final film and official judge scope

- `src/engine/chat/runtime/turn.rs` applies `budgeted_tool_definitions` **again after** RSI phase control, which can rebuild a tool menu from the original untrimmed catalog. `harness.tool_schema_final_budget` tracks the final schema bytes and cap; this does not bypass actual authorization.
- `scripts/benchmark/swebench_dominion.py` assigns SHA-based case directories and restricts GitHub `owner/name` labels before constructing clone URLs. The official evaluator receives `--instance_ids` matching the prediction cohort. Its run ID now commits to the binary SHA, dataset-row SHA, model and exact patch JSONL, preventing stale result reuse after a binary or cohort change. See the official [SWE-bench evaluation guide](https://www.swebench.com/SWE-bench/guides/evaluation/).
- `src/engine/video/quality.rs::inspect_final` checks the staged aggregate MP4 using FFprobe and a stronger final-film duration bound (5% or 1 second) plus the existing dimension/audio checks, before `film.rs` publishes `output.mp4`. Failed stitching preserves each original paid clip and all SHA receipts; no new provider request is made. Original output metadata remains `semanticQualityVerified:false` until an independent semantic assessment exists.
- `scripts/benchmark/test_video_concat_contract.py` runs a tiny, local, **zero-paid-cost** FFmpeg/FFprobe concat smoke in CI when the binaries are available. A direct local synthetic media smoke produced 6.128 seconds for two 3-second clips and audio/video streams. This verifies the external assembly/probe behavior, **not** the currently uncompiled Rust integration or cinematic quality.

The FFmpeg concat demuxer requires compatible stream layouts and adjusts timestamps from each input duration; its successful exit alone does not establish a correct-length film. [FFmpeg format documentation](https://www.ffmpeg.org/ffmpeg-formats.html).

