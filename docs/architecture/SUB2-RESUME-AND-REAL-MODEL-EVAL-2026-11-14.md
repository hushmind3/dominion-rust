# Sub2: atomic goal resume, complete usage attribution, and opt-in real-model evaluation

**Date:** 2026-11-14. **Branch:** `dominion-sub2`.
**Judge baseline:** 57/100 (shared 2nd, before this change). This is **not** a new score and does not imply a successfully completed real paid-model benchmark.

## Production source paths

```text
First run (already implemented):
 POST /api/goals -> api/goals.rs::create_goal
   -> Store::transaction(goal + job) -> background goal job

Recovery (new):
 POST /api/goals/:id/resume -> api/goals/recovery.rs::resume_goal
   -> prior terminal job + checkpoint, owner note and external-effects approval
   -> db.rs::session_has_uncertain_effects (parent + delegated descendants)
   -> db/goal_resume.rs::Store::resume_goal_atomic
        tx: check current goal status and prior jobId
        tx: reconcile owner-inspected *uncertain* tool effects
             preserve original status, effect state and outcomeUnknown
             mark unverified, NEVER promote to verified execution evidence
        tx: INSERT a new queued job (no UPSERT)
        tx: update the goal with new job ID and resume checkpoint
        COMMIT only if all changes succeed
   -> bus job record -> jobs::run -> goals.rs::run
   -> acceptance_gaps_for_workspace -> reviewer -> finish
   -> goals/receipts.rs::record -> goal_receipt
   -> GET /api/goals/:id/receipt
```

The earlier recovery API updated goal status and created its new job in **separate** writes. A failed job insert could leave a queued goal without an actual task. The transaction now prevents that case, including when a delegated tool effect is being reconciled. SQLite trigger-based regression tests deliberately abort the queued job INSERT and check that the goal, old job and unknown effects remain intact. A separate test confirms that a delegated child's still-active execution cannot be relabeled as reviewed just because the parent job is terminal.

When an owner explicitly acknowledges a terminal unknown effect, the receipt keeps `ownerReconciliation.originalStatus`, `originalOutcomeUnknown`, `originalEffectState`, the review note and `newResultVerified=false`. Its new status is `reviewed`, not `completed`, and `toolReceipt.verified=false`. The dispatcher sees an inspected effect and can continue after the owner has verified the outside world; the prior tool's uncertain result is not automatically counted as a successful write. Owner approval is **never** evidence that the external tool succeeded.

## End-to-end usage attribution

```text
GoalControl::complete (initial plan, review and replan)
 -> llm::complete_for_session(goal_session_id)
 -> gateway::record_usage(model.usage, goal_session_id, purpose="helper")
Executor calls in the same durable goal session
 -> gateway::record_usage(...)
 -> Store::session_usage(goal_session_id)
 -> goals/receipts.rs::record(usage)
 -> GET /api/goals/:id/receipt
```

Previously, the planning/reviewer calls used `llm::complete`, which recorded no goal session ID. Their tokens therefore went missing from the per-goal receipt. With the new session attribution, **the existing `maxExecutorTokens` API field is applied as a soft ceiling to all measurable goal-session calls**, including the associated helper calls. The name remains for backward-compatible clients; an in-flight call may overshoot the soft budget.

Provider usage can be provider-reported, estimated or absent. The receipt carries `usageIncludesGoalHelperCalls=true`, `providerBillingVerified=false` and an explanatory caveat. No actual paid KRW, electricity, amortized GPU expense or successful cost savings are inferred from token counts.

## Opt-in real-model smoke evaluator (no fake achievements)

```text
scripts/sub2-goal-eval.mjs
 -> explicit DOMINION_EVAL_ENABLE=1
 -> validate the benchmark-only engine identity and isolated data/workspace
 -> local Provider only by default; remote Provider requires separate paid opt-in
 -> frozen 3 goal cases: single output, 2 outputs, edit existing file
 -> each task uses a fresh, random, unique path under eval-runs/
 -> POST /api/goals with explicit provider, model, iterations and token ceiling
 -> poll job/goal terminal state (bounded deadline; cancel if timeout)
 -> independent Node file oracle (actual bytes, bounded file, no symlink/path escape)
 -> GET /api/goals/:id/receipt (matches CURRENT job ID)
 -> GET /api/sessions/:id/usage
 -> record completion only if:
       terminal completed AND current-job verified receipt AND
       at least one successful action proof AND independent file oracle passes
       AND at least one real model.usage call is attributable to the goal
 -> emit raw JSON per case to benchmark root/results
 -> aggregate pass@1 and recorded total tokens / verified success
 -> actualBilledKRW=null (not independently reconciled with bills)
```

**Invocation on an isolated local benchmark instance:**

```bash
ROOT="$(mktemp -d /tmp/dominion-eval-XXXXXX)"
mkdir -p "$ROOT/data" "$ROOT/workspace"
echo "benchmark - disposable engine" > "$ROOT/.dominion-benchmark-sentinel"

# In terminal 1, start a SECOND isolated Dominion process, not your normal
# everyday instance. Configure a local provider in its separate database.
DOMINION_BENCHMARK=1 DOMINION_ENGINE_INSTANCE="eval-manual-$(date +%s)" \
  DOMINION_PORT=44327 DOMINION_DATA="$ROOT/data" \
  DOMINION_WORKSPACE="$ROOT/workspace" cargo run --release

# In terminal 2: substitute real matching provider ID and model.
DOMINION_EVAL_ENABLE=1 DOMINION_EVAL_ROOT="$ROOT" \
  DOMINION_EVAL_BASE="http://127.0.0.1:44327" \
  DOMINION_EVAL_PROVIDER_ID="<isolated-server-local-provider-id>" \
  DOMINION_EVAL_MODEL="<installed-local-model>" \
  node scripts/sub2-goal-eval.mjs
```

A missing `model.usage` record does **not** count as demonstrated live-model completion, even if the output file happens to exist. File permission errors are not treated as proof that a file is absent.\n\nThe script **does not run by importing it in tests** and never starts an LLM or generates a fake result by itself. `node --test scripts/sub2-goal-eval.test.mjs` exercises the independent verifier, forged/stale-receipt rejection, and complete cost aggregation without a provider or payment. CI executes these tests and the existing Rust full test suite.

If a cloud provider is deliberately selected, `DOMINION_EVAL_ALLOW_PAID=1` is additionally required. The token ceiling is a **soft** cap; it is not a billing cap in KRW. Paid runs should use an isolated provider key/account with its own externally enforced spending limit.

### Limitations to preserve in scoring

- The deterministic checks and SQLite failure tests demonstrate implementation behavior, not actual open-ended real-model job completion.
- The smoke cases are tiny, transparent artifact tasks, **not** independent SWE-Bench/Terminal-Bench or production deployment evidence.
- No video aesthetics/continuity, frame inspection, sound quality or actual video bill has been measured.
- No model-driven RSI candidate was claimed adopted or inherited from these ordinary goal receipts. `canRewardFrozenRsiEvaluator=false` remains in the receipt.
- Token measurements can include estimates. Missing usage prevents a verified *cost-efficiency* number in the script; output still records the task status and failures.

### Methodology references (design motivation only)

- [Harness-Bench, Yao et al., updated October 2026](https://arxiv.org/abs/2605.27922): compare entire model+harness configurations on sandboxed tasks with final artifacts, traces, and usage.
- [What Does a Harness Buy? Tokens, Mostly (2026-10)](https://arxiv.org/abs/2610.04433): harness effects on pass rates can be noisy while token/cost differences remain material, motivating matched measured evaluations.
- [Long-Horizon-Terminal-Bench 2.0 (2026-10)](https://arxiv.org/abs/2610.14146): long workflows need verifier checkpoints rather than final model statements.
- [Harbor open benchmark framework](https://github.com/harbor-framework/harbor): example of isolated task/environment/verifier contracts.

None of these public benchmark results are attributed to the Dominion product without a genuine comparable run and trace.
