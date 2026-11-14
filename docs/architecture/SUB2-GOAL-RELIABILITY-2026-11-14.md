# Sub2: durable goal admission, evidence-based progress, replay safety

2026-11-14 · `dominion-sub2` · judge baseline **73/100** (`d1638cb4`; CI successful)

This change targets **agent task completion and recoverability**, not menu counts or commands. The judge's 73/100 was awarded *before* these changes. New scoring, large-repository task-completion gains, measured token savings and live-provider reliability have not been independently evaluated.

## 1. Atomic goal/job admission (prevents queued ghost goals)

```text
POST /api/goals {goal, executor, successCriteria, maxIterations, ...}
  -> api/goals.rs::create_goal
     validate owner-provided acceptance criteria & iteration limits
     allocate goal UUID + job UUID
     Store::transaction([("goal", record with jobId), ("job", durable queued record)])
       -> SQLite single commit or all rolled back
     on commit: goal/job bus records + job.queued audit
     on failure: HTTP 500 + goal.enqueue_failed audit, no phantom queued goal
  -> scheduler/jobs executes the durable job linked by jobId
```

Before this change, the goal was written independently, then `jobs::enqueue` could fail. A durable `queued` goal with no job would be visible but unable to run. The new transaction leaves neither record on failure. Existing `jobs::enqueue` behavior for other job types is unchanged.

Regression tests in `src/api/goals/tests.rs` verify a real goal record points at precisely one queued job, and a SQLite trigger that aborts **job INSERT** rolls back the preceding goal INSERT.

## 2. Deterministic progress instead of unconditional replanning

```text
engine/goals.rs::run iteration
   -> snapshot previous successful action receipts + current acceptance gaps
   -> run_quiet_outcome (real model + tool dispatch)
   -> collect trusted tool receipts, read file/workspace state
   -> verification::acceptance_gaps_for_workspace
   -> goals/progress.rs::measure
       [new successful action with novel tool+arguments]
       [acceptance predicate that actually becomes satisfied]
       [real tool errors]
   -> progress: reset stalled=0
      no progress: increment stalled
   -> replan only after consecutive unproductive turns exceed replanAfter
   -> lastProgress + stalled + bounded progressHistory (max 64 turns)
   -> goals/receipts.rs: immutable goal receipt with productive/unproductive
      turn counts and actually resolved acceptance-check counts
```

Previously, `stalled += 1` happened after **every** unsuccessful reviewer turn, including work that created additional real files, passed a new check, or made another successful change. That forces repeated planner calls, spends extra tokens and may destabilize a productive strategy. The signal is persisted even when the *final* executor turn completes the goal or hits a token budget; earlier code recorded `lastProgress` only after the success/budget return. The final immutable `goal_receipt` now includes `progressWindowTurns`, `productiveTurnsWindow`, `unproductiveTurnsWindow`, `resolvedChecksWindow` alongside session usage and replans. These counters cover the most recent 64 recorded turns and are **not** a synthetic real-model benchmark.

The new condition does not trust the assistant saying 'done': a duplicate of the same successful tool signature is *not* a fresh action, and failed tool calls are never counted. A newly satisfied deterministic predicate is credible progress even if other tool failures occurred. No acceptance condition or owner authorization is weakened.

`goals/progress.rs` tests cover distinct real successful actions, repeated no-op signatures, failed tools and independently reduced verification gaps.

## 3. Unknown side effects are checked even on successful model turns

```text
run_quiet_outcome
 -> record real successful tool effects
 -> Store::session_has_uncertain_effects(sessionId)
    idx_session_parent -> recursive bounded descendant session IDs (depth <= 16)
    idx_execution_session -> execution rows
    idx_operation_run -> execution_operation rows
    status: running/interrupted OR outcomeUnknown=true
 -> unresolved OR DB read error => goal needs_review (stop, owner reconcilies)
 -> only fully reconciled sessions continue verification/next tool turn
```

Previously, unresolved external effects were inspected **only if the model turn itself returned an error**. A model could produce a successful message even while a tool operation was interrupted or its remote side effects were unknown. Such work now stops without replaying it. The new read is scoped to the executor session **and its delegated descendants (up to depth 16)**, using indexed session-parent, execution-session and operation-run joins. Unrelated rooms and agents do not block the goal. The SQLite index exists in the normal `Store::open` migration path.

The regression fixture writes actual `session`, `execution` and `execution_operation` records: another session's unknown operation cannot block the goal; a completed operation is allowed; unknown, interrupted and running operations are detected. A **grandchild** session's uncertain external effect blocks the parent until reconciled. Error-on-query uses the same fail-closed treatment.

## 4. Incremental goal context instead of repeated self-claims

```text
engine/goals.rs -> goals/turn_input.rs::executor_prompt
  - full owner goal, current plan and acceptance predicates
  - bounded project inspection (full only initially or at intervals)
  - bounded review/error notes (~2.5K characters)
  - current iteration and stalled-work signal
  - explicit instruction to use tools and verify
  - NO copy of the previous 6,000-character assistant transcript
```

The persisted session already contains the actual messages and tool outputs. Re-inserting the model's own previous answer on every turn both wastes input tokens and can amplify unverified progress claims. The complete execution transcript remains available to review, recovery and audit; it is no longer duplicated in every executor input. Tests ensure bounded context with long reviewer notes and preservation of actual acceptance requirements. **No numerical token saving is claimed without provider usage and cache-hit measurements.**

## Verification evidence and limits

- **Previous baseline:** [`d1638cb4` CI](https://github.com/hushmind3/dominion-rust/actions/runs/43559631946) succeeded.
- **Current changes:** GitHub Actions separately executes web typecheck/build and `cargo test --release` on each branch commit; see [live branch CI](https://github.com/hushmind3/dominion-rust/actions?query=branch%3Adominion-sub2). Source and added test cases must not be conflated with passing CI until that exact HEAD run completes.
- **Reproducible test coverage:** atomic insert/rollback with injected SQLite failure; cross-session and multi-generation delegated unknown effects; acceptance-based progress and repeated ineffective tool calls; bounded incremental prompts.
- **Not measured in this update:** paid model multi-hour completion, SWE-bench task passes, actual billed-token reductions, OS-specific deployment behavior and public product economics.

## Related external research, with limited applicability

- [Anthropic, Harness design for long-running application development (2026-03-24)](https://www.anthropic.com/research/harness-design-long-running-apps).
- [Harness-Bench, 2026-05](https://arxiv.org/abs/2605.27922) considers tool execution, verification, recovery and efficiency in the combined model/harness evaluation.
- [Same Model, Different Harness, 2026-08](https://arxiv.org/abs/2608.26218) discusses repeated/stalled work and redundant context under pressure.
- [RigorBench, 2026-06](https://arxiv.org/abs/2606.22678) emphasizes verification coverage, recovery efficiency and atomic state transitions.

These works motivate the design direction; their reported benchmark improvements are **not** attributed to Dominion without a comparable reproducible experiment.
