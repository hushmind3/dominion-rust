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

## In-process verified artifact promotion

External Rust applications can now call `DominionSdk::apply_goal_patch(goal_id, sha256)` and `DominionSdk::discard_goal_worktree(goal_id, "DISCARD_GOAL_WORKTREE")`. The methods delegate to the exact `engine::goal_workspace::apply_reviewed` and `discard` functions used by REST. A full 64-character SHA-256 expected digest is required, and the engine additionally verifies that the exact reviewed worktree patch is unchanged and eligible for application. Disposal requires explicit confirmation and does not delete the original repository. This closes the native SDK goal lifecycle: enqueue -> model/tool execution -> review receipt/diff -> apply or discard -> optional owner-reviewed resume. The negative tests reject malformed hashes, unverified candidate patches and unconfirmed disposal.

## Evidence-complete 2026 SWE-bench grading

2026 SWE-bench upstream writes `logs/evaluation/<runId>/<model>/<instance>/report.json`
and the exact evaluated `patch.diff`. The native runner now invokes the
official grader with the **exact** attempted instance IDs and runs it
with the current working directory set to the new benchmark output folder,
keeping the generated Docker logs out of the source checkout.

Per-instance official success is accepted only after validating:

- The prediction was produced from this native source checkout and its
  `patchSha256` and byte count still equal the runner's persisted receipt
- The official evaluator's `patch.diff` digest matches the same SHA-256
- The report's instance key provides a real boolean `resolved`

No grader report leaves `officialSolved = null` for a nonempty patch.
An empty patch is a known non-solve and never masquerades as a pass.
A modified/reused official patch is a hard validation error.

The new offline import mode also works when a third-party machine ran the
official Docker grader after the Dominion generation job:
```sh
node scripts/sub2-swebench-verified.mjs --import-grade \
  /absolute/benchmark-output/report.json \
  /absolute/benchmark-output/logs/evaluation/<runId>
```
This reads saved `predictions.jsonl`, verifies source digests, updates
`report.json` with `officialGrading` provenance, and records partial
grading honestly. No model/API request is made by the import command.

All cost accounting now traverses the real native
`session.parentSessionId` tree (to a bounded depth), includes helper-agent
token usage and execution-operation receipts, and excludes unrelated
sessions. The result records `attributedSessionCount`. Source: upstream
`SWE-bench/SWE-bench` `swebench/harness/run_evaluation.py` and
`www.swebench.com/SWE-bench/guides/evaluation/` (inspected Nov 2026).

This is a fully wired execution-and-evidence path, NOT evidence of a real
SWE-bench solve without an authorized actual model run plus upstream Docker
grading. Synthetic graded-report unit fixtures test parser integrity only.

## Paired official benchmark comparison

`scripts/sub2-swebench-compare.mjs` consumes **two** independently graded
`report.json` files and compares the identical SWE-bench Verified issue IDs
with identical source repository/base revisions. It requires per-task official
Docker grade provenance, refuses missing grades, verifies the full instance
denominator, reports discordant task wins, token usage, and tool calls, and
only labels the harness comparison `harnessComparisonControlled: true` when
the recorded provider/model and iteration/token/time budget are identical.
Estimated or missing usage is not presented as independently counted tokens.
The companion fixture suite `sub2-swebench-compare.test.mjs` runs in CI.

After real official grading of two benchmark outputs:
```sh
node scripts/sub2-swebench-compare.mjs \
  /absolute/trial-A/report.json /absolute/trial-B/report.json
```
The comparison reports observed paired outcomes; it is not evidence of
frontier performance until the owner runs the two real model trials.

## 2026-11-15 native tool boundary and delegated spend control

The live Rust tool gateway has been split without changing its public
`tools::execute`, `tools::definitions` or `tools::run_command`
integration. Responsibilities are physically owned by:
- `src/engine/tools/gateway.rs`: real authority check, RSI scope and dispatch
- `src/engine/tools/editor.rs`: indexed search and policy-guarded source writes
- `src/engine/tools/process.rs`: timeout, child process groups and bounded I/O
- `src/engine/tools/network.rs`: SSRF defense, DNS pinning and bounded fetch
- `src/engine/tools/tests.rs`: regression tests behind `#[cfg(test)]`
- `src/engine/tools.rs`: stable native tool catalogue + delegation façade

A delegation request now passes the native `tools::delegate` gateway,
checks the persisted parent goal's full descendant model usage
(`src/db/usage_tree.rs`), and calls
`Store::reserve_delegation` (`src/db/delegation.rs`) for a SQLite
`IMMEDIATE` transaction that jointly inserts one child session/job.
This rejects overlapping concurrent admissions, cancellation, UUID
collisions and over-limit child creation **before** a worker can spawn.
The existing `jobs::start_now` worker is launched only after commit.
Boundaries are 4 active and 24 total child jobs per parent session,
with previously bounded delegation depth unchanged.

`goals::run` now checks `Store::session_tree_usage` before the
next executor turn and again after review. This counts all descendant
session model usage (including delegated agents and helper turns)
against the owner's existing `maxExecutorTokens` ceiling. The
immutable `goals/receipts.rs` receipt records the complete tree
usage with `usageComplete` and explicit error when accounting fails.
The cap is observational between model calls; it does not reserve
future provider tokens or prove exact billed KRW. A model call already
in flight can exceed the threshold. Nested child lineage with an
inherited `goalId` still resolves to the verified root goal.

Regression tests cover SQLite atomic limits, orphan-job rollback,
cancelled parent rejection, cycle-proof descendant sums, inherited
goal ancestry, source-edit security and bounded Unix process groups.
2026 reference: Tokio's official `tokio::process` cancellation/
`kill_on_drop` guidance, and Sourcegraph CodeScaleBench March 2026
on the value of large-codebase retrieval. No external benchmark gain
is claimed without a matched real-model run.

## Native catalogue/delegation separation and visible goal spend

The stable `src/engine/tools.rs` entrypoint now only assembles
independently maintained submodules: `catalog.rs` owns all model-facing
tool schemas and `delegation.rs` owns bounded child-worker creation,
session inheritance, and owner-only results. The gateway still performs
the final permission check before invoking real tools. The original
~113 KB all-in-one tools file is now ~8 KB, with the removed code
retained in typed, runnable Rust modules, not stubs.

The existing `GET /api/sessions/{id}/usage` returns complete native
descendant totals when `id` belongs to a goal's verified root session;
ordinary independent chat sessions preserve their former direct-session
accounting. `DominionSdk::goal_usage(goal_id)` exposes the same
whole-goal ledger directly in-process and rejects a queued goal without
a persisted execution session. A regression test verifies root + child
totals, exclusion of unrelated sessions, and inherited-goal ancestry.
