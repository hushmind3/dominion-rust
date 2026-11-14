# Sub2 — Frontier native SDK + autonomous job authority (2026-11-15)

## Executable change (not a second agent runtime)

The canonical decision engine remains Rust. `DominionSdk` can now launch a
**durable autonomous goal** without an HTTP server, watch its persisted job,
inspect an independently observed worktree diff and obtain the **current
attempt's** immutable goal receipt. Cancellation uses the same SQLite CAS in
the REST endpoint and in-process SDK. TypeScript is an optional network
transport, not a delegated planner, policy engine or harness.

```rust
use dominion_rust::sdk::{DominionSdk, EngineConfig, GoalOptions};
use std::{path::PathBuf, time::Duration};

// Must not share the data directory with a running Dominion server.
async fn example() -> Result<(), String> {
    let sdk = DominionSdk::open(EngineConfig::new(
        PathBuf::from("./my-project"), PathBuf::from("./.dominion-data")))?;
    sdk.start_job_workers();
    let mut options = GoalOptions::new("Repair parser, add regression tests, run them");
    options.max_iterations = 16;
    options.isolate_worktree = true;
    let goal = sdk.enqueue_goal(options)?;
    let id = goal["id"].as_str().ok_or("missing goal id")?;
    let job_id = goal["jobId"].as_str().ok_or("missing job id")?;
    let job = sdk.wait_job(job_id, Duration::from_secs(1800)).await?;
    let diff = sdk.goal_diff(id).await?;
    let receipt = sdk.goal_receipt(id); // None if no independently sealed receipt
    println!("status={} changed={} verified={}", job["status"],
        diff, receipt.is_some());
    Ok(())
}
```

`enqueue_goal` alone only persists queued work. The native runtime must be
configured with a model and started with `start_job_workers()` (or the
existing desktop bootstrap). `wait_job` does not claim model success;
it returns the terminal persistent state and times out when no worker runs.
An error, pause for review, or owner cancellation is not an acceptance receipt.

### Actual source -> call -> persistence path

| Contract | Actual connected path |
| --- | --- |
| Native entry | `src/sdk.rs::enqueue_goal(GoalOptions)` |
| Shared admission | `src/engine/goals/enqueue.rs::enqueue_goal` |
| HTTP parity | `src/api/goals.rs::create_goal` calls same engine function |
| Durable atomicity | `Store::transaction(goal + job)`, then bus events and audit |
| Execution | `jobs::start` -> `goals::run` -> planner + Rust chat/tool loop -> frozen checks |
| Tool effects | Native tool evidence, postcondition verification, worktree diff |
| Receipt | `src/sdk.rs::goal_receipt` selects `goal-receipt:<goal-id>:<current-job-id>` |
| Cancellation | `src/engine/jobs/cancellation.rs::cancel_job` -> `patch_if(job.status)`; both REST and SDK call it |
| Owner continuity | `DominionSdk::redirect_goal` and `steer` use original Rust durable control paths |

Cancellation now seals the job state first, then signals the session.
A stale request against a settled job cannot inadvertently stop an unrelated
later session. Goal and job admission is transactional; an invalid request
creates neither record. No model completion self-report is treated as a proof.

## 2026 US / China source-level comparative audit

The following are architectural comparisons of **source**, not endorsements
or borrowed provider policies.

| Project and actual source | Mechanism | Choice for Dominion |
| --- | --- | --- |
| US: OpenAI Codex `codex-rs/core/src/tools/registry.rs` and `router.rs` | Typed tool runtimes, tool exposure separate from execution; native review, cancellation, hook lifecycle and deferred tool discovery | Preserve native Rust registry/approval/effect ledger; avoid wrapping Codex execution policy |
| US: Anthropic Claude Agent SDK `packages` and SDK change history | Hook events and explicit permission lifecycle around agent tools | Transport-compatible event semantics; do not embed CLI agent or inherit its planner |
| China: DeepSeek Harness `packages/core/agent-loop/src/tool-calls.ts` | Concurrent, bounded rolling tool pool; exclusive calls act as barriers; commit results in model order, safely drain abort | Evaluate against Dominion native dispatch; any future parallelization must preserve invocation IDs, order, effect uncertainty, approvals and cancellation |
| China: ByteDance DeerFlow `backend/packages/harness`, `backend/app`, `docs/ARCHITECTURE.md` | One-way app → harness boundary, middleware, persistent subagent batches, tamper-evident tool receipts | Keep HTTP/UI as adapters; job CAS and receipt belong to Rust engine, not UI |
| China: Volcengine OpenViking `README.md`, memory integration source | L0/L1/L2 progressive context and memory trajectory | Consider progressive retrieval only when loss-of-owner-intent and token measurements beat existing Rust context |
| RSI: SakanaAI ShinkaEvolve repository | Archive, parallel isolated candidates, migration between islands | Retain diverse candidates but accept only independently evaluated descendants |
| RSI: OpenEvolve `openevolve/database.py` | MAP-Elites feature cells and isolated evolutionary islands; best program retained | Benchmark diversity-aware candidate parent sampling; never let archive novelty bypass frozen holdout gates |

Sources:
- https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/registry.rs
- https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/router.rs
- https://github.com/anthropics/claude-agent-sdk-typescript
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/tool-calls.ts
- https://github.com/bytedance/deer-flow/blob/main/docs/ARCHITECTURE.md
- https://github.com/volcengine/OpenViking
- https://github.com/SakanaAI/ShinkaEvolve
- https://github.com/algorithmicsuperintelligence/openevolve/blob/main/openevolve/database.py

## Native parallel tool scheduler improvement

The prior `tool.combination = parallel-reads` implementation extracted **all**
read-only calls from a model batch and ran them before any writes. For a
model batch such as `workspace_write → workspace_read → workspace_read`, this
could run the reads against stale source and commit results out of causal
order. It also did not debit the proposal budget for parallel reads.

The corrected production path in `src/engine/chat/runtime/turn.rs` now:

- Batches only a **leading contiguous read prefix**, stopping at an unknown
  or effectful tool barrier; remaining calls execute in their original order
  through the serial dispatcher.
- Runs no more than **4** reads concurrently using
  `futures_util::stream::iter(...).buffered(4)`, preserving result order.
- Debits the normal tool-proposal budget for each parallel read and checks
  cancellation before starting the pool.
- Disables parallel-read batching when the independent `read-first`
  ordering policy is active; the two policies cannot silently combine.
- Adds Rust unit tests for `read → read → write → read`, an initial exclusive
  call, all-read and empty batches.

This is a bounded conservative improvement, **not** a claim of the complete
DeepSeek dynamic, reclassified rolling scheduler: reads following a write run
sequentially until a full barrier-aware group scheduler is introduced.
Tool failures and external effects continue through the existing native
evidence engine. Benchmark latency and billed token impact are unmeasured.

## RSI integration contract and next experiments

An SDK goal can improve the app, but this **does not itself constitute RSI**.
The existing native RSI path is `rsi::campaign` ->
isolated `rsi_worktree` -> `rsi::evaluation` ->
`rsi::lineage::adopt` -> descendant execution and
`rsi::method_control` counterfactual. It preserves the independent
paired evaluator and requires accepted source hashes and matching base
commit. A goal's receipt is usable as an observed task outcome, not as an
automatic license for an RSI candidate to edit its own evaluator.
No model weight learning is part of this path.

Algorithmic research directions with explicit proof requirements:

1. DeepSeek-style tool fan-out: evaluate a scheduler against currently
   serialized behavior on independent read tools, side-effectful writes and
   cancellation; require original model-order transcript and matching receipts.
2. OpenEvolve-style diversity niches: select candidate parents using verified
   task-domain success, cost and structural novelty, while keeping the original
   frozen paired evaluator unchanged. Test descendant gains, not just archive
   coverage.
3. Cross-domain RSI transfer: A→B→A improvement only when an inherited strategy
   actually affects the next native executable run, and independently passes
   holdouts on the original domain.

## Verification and benchmark status

- Added Rust SDK tests for rejection without ghost records, atomic goal/job
  linkage, current receipt absence, job cancellation and bounded waiting.
- CI on push includes `cargo test --release`, the native SDK suite, and
  Node SDK + web build. At document-writing time, no completed CI result for
  this change has been established; do not count in-progress jobs as passes.
- Live frontier model coding runs, paid API accounting, SWE-bench Verified or
  SWE-bench Pro successes are **not** established by mock tests.
- SWE-bench Verified can still be used for reproducibility, but 2026 audits
  identify benchmark contamination and scoring limitations. Add Pro and fresh
  private regression tasks, frozen test environments, exact model/token/tool
  budgets, failed attempts and reproducible patch artifacts before comparing
  success rates.

## Native interrupted-goal recovery and real benchmark controls (follow-up)

The HTTP-only `src/api/goals/recovery.rs` recovery controller was relocated to
`src/engine/goals/resume.rs::resume_goal` and now serves both the REST adapter
and `DominionSdk::resume_goal(GoalResumeOptions)` in process. A recovery
transaction allocates a new job ID, preserves prior immutable job evidence,
reconciles unconfirmed external effects only after an owner note, and retains
the current-attempt receipt boundary. Duplicate or simultaneous resumes are
rejected through a guarded SQLite transaction. An unstarted cancelled goal
may legitimately have no worktree; missing metadata for an in-flight worktree
edit is not silently recovered.

`src/engine/chat/runtime/turn.rs` now forms one tool response for **each**
original call ID even when some read arguments fail JSON parsing. All results
are emitted in model invocation order, not completion order. Trial success
credit requires every proposed call to parse and execute successfully.

SWE-bench `scripts/sub2-swebench-verified.mjs` now enforces an explicit,
exact `DOMINION_SWE_MAX_TOKENS` per goal (default 80000), with validated
integer iterations/time/instance limits rather than silent clamping. The report
records the chosen resource limits and real native diagnostic episodes and
replans. `officialResolvedAfterReplanRate` requires official Docker grading
and persisted Rust replan observations; it is an observational conditional
rate, not proof that replanning caused resolution.

Tests: native SDK cancelled-unstarted resume; new immutable job lineage;
uncertain external effects require an explicit owner note; bounded wait;
parallel tool ID ordering/malformed call handling; SWE benchmark limit and
measured-replan aggregate fixtures. These tests provide deterministic
conformance evidence; real provider SWE solves and provider bills require
separate authorized benchmark runs.
