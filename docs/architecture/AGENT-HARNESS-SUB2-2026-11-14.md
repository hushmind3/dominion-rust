# DOMINION Sub2 · Autonomous Agent Harness v2 (2026-11-14)

Branch: `dominion-sub2` · New benchmark priority: **autonomous agent + harness, 25/100**. The 52/100 score is the judge's **pre-upgrade static assessment**, not a measured result for this revision.

## End-to-end, user-accessible execution

```text
Home / GoalConsole.tsx
  └── POST /api/goals   (owner intent, executor agent, optional file acceptance, soft token cap)
      └── api/goals.rs::create_goal
          └── jobs.rs::enqueue("goal")  -> durable SQLite job
              └── jobs.rs::run_job -> goals.rs::run
                  ├── project.rs::inspect: read-only, bounded workspace & project manifests
                  ├── goal_control.rs::complete: initial ordered plan
                  ├── plan_quality.rs::score: deterministic heuristic; not proof
                  └── iterate (maxIterations 1..64, maxReplans 0..16)
                      ├── checkpoint(goalState.inFlight, evidence, iteration)
                      ├── run_quiet_outcome -> chat/runtime/turn.rs
                      │    ├── provider inference & context/caching
                      │    ├── real file/shell/browser/MCP/GitHub tools
                      │    └── tools.rs::delegate_task -> independent worker job/session
                      ├── save successful tool evidence BEFORE an early exit
                      ├── verification.rs::acceptance_gaps (deterministic)
                      ├── reviewer LLM ONLY if deterministic gates pass
                      ├── on failure -> replanner and persisted revised plan
                      └── finish -> artifacts.rs::collect + receipts.rs::record
                         └── episode::set_root_outcome + optional existing learning
```

The goal interface appears **above the ordinary one-turn ExecutionMonitor on Home**; it does not route the owner's autonomous task through a single `prompt` job. Existing provider and agent credentials are reused. Ordinary chat, rooms, and RSI do not have to adopt another runtime.

## Execution responsibility and evidence boundary

| Responsibility | Production source | Durable output or owner action |
|---|---|---|
| Project orientation | `engine/goals/project.rs::inspect`, `engine/goals/code_navigation.rs::collect` | Root inventory + bounded `AGENTS.md`, `README.md`, manifests, suggested project-specific test commands; incremental source index and hash-verified ranked source spans, with no extra model call |
| Plan & repair | `engine/goals.rs::run`, `goal_control.rs` | `planHistory`, `reviewHistory`, `planScore`, explicit acceptance checks; full execution route |
| Code and tools | `engine/chat/runtime/turn.rs`, `engine/tools.rs` | Real tool operations; delegated workers in isolated sessions; no fabricated success from a model answer |
| Artifact receipts | `engine/goals/artifacts.rs::collect` | Workspace-only actual files from successful write evidence, file bytes/SHA-256; does not attribute arbitrary shell edits |
| Immutable attempt observation | `engine/goals/receipts.rs::record` | `goal_receipt` tied to `goalId + jobId`: proven status, action count, tokens, iterations, artifacts, plan score |
| Bounded actual diff | `engine/goals/diff.rs::capture` | GET `/api/goals/:id/diff` invokes read-only Git diff for up to 12 **receipt-scoped**, <=1MiB tracked files, no unrelated file dump |
| Safe resume | `api/goals.rs::resume_goal` | POST `/api/goals/:id/resume` with manual acknowledgement + explanation for uncertain external effects; new job inherits checked owner snapshot |
| Goal planning override | `api/goals.rs::edit_plan` | POST `/api/goals/:id/plan`; only queued / stopped / failed / needs_review, not a running turn |
| Worker crash/failure | `engine/jobs/goal_status.rs::on_error`, `jobs.rs::recover` | Parent goal becomes failed/needs_review/cancelled instead of hanging in running state |
| Team operations | `api/goals.rs::goal_subtasks/goal_approvals`, existing `tools.rs::delegate_task` | Status of real subordinate jobs and scoped approval requests, including descendant sessions; one-time tool approve/deny in goal UI |
| Live UX | `web/src/components/GoalConsole.tsx` | Goal status, progress, plan and owner edits, verification gaps, scoped approvals, delegated jobs, last assistant report, file hashes, actual Git diff, cancel/reconcile/resume |

**Important invariant:** a received `delegate_task` reply is NOT completion of a delegated job. `verification.rs::successful_evidence` rejects `queued`, `running`, `pending`, `failed`, `cancelled`, `needs_review`, `interrupted`, and `error` result states. A successful transport can no longer satisfy `tool_succeeded` for unfinished delegated work.

### Crash, cancellation and explicit recovery

- The original attempt is never re-enqueued from an ambiguous `goalState.inFlight`; `jobs.rs::recover` flags it for review.
- On resumption `api/goals.rs::resume_goal` locates the original immutable job, checks that it is terminal, verifies owner acknowledgement and 10+ characters of reconciliation notes **when external outcome is unknown**, and persists a fresh `resumeState`.
- For an in-flight uncertain turn the next attempt skips its old iteration index. It inherits prior plan, accepted tool evidence, independent-review gaps and owner's note. It checks real workspace state and acceptance conditions in its following iteration rather than repeating the uncertain tool call automatically.
- Cancelled goals can also be resumed only by the owner action. The session cancellation flag clears **after** acknowledgement, but the old generation-bound worker remains unable to resume because it captured the earlier cancellation epoch.
- The API can extend the executor token budget and total allowed iterations when the owner explicitly asks. Max 64 turns and 2,000,000 executor-session tokens.
- A failed dispatcher updates the parent goal's terminal status, with an auditable `goal.worker_failed` record. The job cancellation endpoint also synchronizes `queued` or `running` goals into `cancelled`, including cancellation prior to the first session checkpoint.

### Cost policy

- Deterministically failing acceptance checks skip that turn's redundant reviewer model call. `reviewHistory[].skippedForDeterministicGap` and `goal_receipt.reviewerCallsAvoided` give concrete counts of skipped calls, not invented monetary savings.
- `maxExecutorTokens` defaults to 80,000; it is a **soft bound checked between executor turns** using `store.session_usage`. Single-turn overruns are possible. Planner/replanner/reviewer helper calls are not included in that session-only budget, so it is **not a complete provider bill cap**.
- `project::inspect` uses local file operations, no extra model call. `code_navigation::collect` refreshes the existing source index in a blocking task, ranks goal-relevant files, and includes SHA-checked spans in a bounded first-turn/periodic context. The large source excerpt is not retransmitted on every executor turn, reducing repeated token input; real savings remain unmeasured. Suggested `cargo test`, `npm test`, `pytest` or `go test` commands are only advice; actual execution requires normal agent tools/approvals.
- The completion receipt stores the provider's recorded session usage and independent acceptance outcome. It is NOT evidence of a reduced benchmark cost without a paired baseline.

### RSI coupling

`rsi_system/agent_feedback.rs::observe` aggregates the most recent 64 actual `goal_receipt` attempts into counts of accepted task completions, unresolved outcomes, session token consumption, and redundant-reviewer calls avoided. `rsi_system.rs::architecture_frontier` attaches this **`ownerTaskFeedback` directly to the real `agent_harness` editable source candidates**, so the already compiled descendant planner/worker receives concrete production observations while deciding which target source/method to improve. This is **a feedback signal rather than a benchmark reward**: the payload has `qualityProven=false` and `independentPairedEvaluator=false`; no evidence is promoted into frozen paired RSI scores.

`rsi_system.rs::source_role` and `architecture_role_domain` now include **agent_harness** files (`goals.rs`, `goal_control.rs`, `goals/*`, `api/goals.rs`) as executable `context`-domain improvement targets. The descendant `source_inventory → blueprint → architecture_frontier` can propose improving the actual goal loop, recovery policy and project inspection.

`goal_receipt` belongs to normal user task telemetry. It is explicitly tagged `isIndependentBenchmark=false`, `canRewardFrozenRsiEvaluator=false`: no goal outcome can self-certify a frozen RSI paired evaluation, no model weights are edited, and adoption still requires original independent receipts, rebuilt child and source lineage.

### 2026 research basis

- OpenAI, **Harness engineering** (2026-02-11): https://openai.com/index/harness-engineering/
- OpenAI Developers, **Codex as a platform: build on the open agent harness** (2026-08-19): https://developers.openai.com/blog/codex-as-a-platform
- Anthropic, **Harness design for long-running application development** (2026-03-24): https://www.anthropic.com/research/harness-design-long-running-apps
- Anthropic, **Building a C compiler with a team of parallel Claudes** (2026-02-05): https://www.anthropic.com/engineering/building-c-compiler
- Anthropic, **Scaling Managed Agents with stable interfaces** (2026-04-08): https://www.anthropic.com/research/managed-agents

### Unproven / further integration

1. No Rust compilation, TypeScript build, test, CI, actual model session, benchmark, or paid video generation was run in this source-only change. Regression specifications were authored, not executed.
2. Artifact capture is conservative: direct writes can produce hashes, but shell/Git/API mutations without a tracked path require a broader trusted before/after worktree journal. Git diff shows tracked changes, not untracked file contents or a guaranteed per-goal baseline.
3. Normal goal executor can delegate to existing independent workers; its real child-job status and pending approvals are visible in the owner goal console. Pending/running/failed delegation statuses cannot satisfy independent success evidence. A dedicated planning-DAG/parallel merge conflict scheduler and explicit child-job quality integration remain deeper engineering goals.
4. User goal receipt recording is live-source integrated; statistically significant task-completion, API spend and agent–RSI A→B→A improvement require actual independent real-run evidence.
5. Current `goals.rs` retains a large iterative coordinator (with production tests now external) and `chat/runtime/turn.rs` still mixes substantial concerns. Further structural isolation should follow verified execution boundaries.

### 2026-11-14 static rejudge follow-up

After the judge's 57/100 review, the executable harness gained an **opt-in detached Git goal workspace** and a **real 1–4 read-only parallel investigation path**. Unlike a new directory only, the scoped root is carried by `goal_workspace.rs::workspace_for_session` into `chat/runtime/dispatch.rs`, `goals.rs`, `goals/{project,code_navigation,artifacts}.rs`, and `verification.rs`. The owner can view exact tracked diffs and untracked names and explicitly discard a terminal goal's worktree without touching the original checkout. The same worktree is inherited by child sessions.

`tools/batch.rs::delegate_many` creates actual asynchronous child jobs, and `delegatedReadOnly` is checked at the tool catalogue, the native tool executor and MCP dispatch. The new modules are mapped into the compiled RSI source frontier as actual harness/tools changes. Source creation and source review here are not a frozen-paired performance measurement; the new integration has not been compiled or tested.

See [Rejudge follow-up source map](AGENT-HARNESS-SUB2-REJUDGE-2026-11-14.md).
