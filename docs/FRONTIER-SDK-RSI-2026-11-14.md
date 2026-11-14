# DOMINION: independent Rust harness, first-party SDK and evidence-gated RSI
Date: 2026-11-14. Working branch: `dominion-champion`.

This is a **source-linked implementation map**, not a claim of frontier task scores, successful CI, or completed paid model inference.

## Source comparison (direct file-level review, 2026)

| Origin | Specific implementation examined | What DOMINION adopts | Important limitation/choice |
| --- | --- | --- | --- |
| US — [Codex](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/orchestrator.rs) | Rust `tools/orchestrator.rs`: tool attempt, sandbox scope and approval; [`multi_agents.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents.rs): child inherits provider/workspace/approval state | Explicit per-step tool execution boundary, typed call IDs, session lease and subordinate execution scope. New owner instruction now prevents *unstarted* calls from an obsolete model plan | Codex's internal model transport and policy are not imported or required |
| US — [OpenHands SDK](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/state.py) | `conversation/state.py` event log with active-branch state projection and thread lock | Durable SQLite event/effect receipts, exact tool result pairing, user command provenance, and resumable session API | Copying OpenHands' Python SDK/runtime would duplicate the Rust execution engine; DOMINION retains a single native authority |
| China — [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md) | Cordis composition: plugin services, event callbacks, reversible registrations, selectable profiles | Independently replaceable policies, tool routing and user-owned execution config as a Rust-first design goal | All-plugin Cordis + Node service lifecycle is not a replacement for DOMINION's existing Rust scheduler, SQLite workers and native tools |
| China — [DeerFlow](https://github.com/bytedance/deer-flow/blob/main/backend/packages/harness/deerflow/agents/lead_agent/agent.py) | `lead_agent/agent.py`: ordered middleware, deferred tool declarations, budget controls, bounded subagent delegation | Scoped tool schemas, bounded parallel independent reads, explicit separation of model step vs tool step, stage-specific evidence | Python LangGraph state manager and its application/gateway dependency are not embedded |
| China — [Qwen-Agent](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/agent.py) | `qwen_agent/agent.py`: tool registry, function map, multiagent and MCP abstractions | Typed model-independent tool namespace and optional MCP adapters | Generic Python action loop alone does not supply DOMINION's worktree verifier, budget ledger, or crash-safe effect accounting |
| Research — [GEPA](https://github.com/gepa-ai/gepa/blob/main/src/gepa/api.py), ICLR 2026 | Reflective mutation uses diagnostics; Pareto-frontier selection preserves complementary per-example wins | `rsi/learning/failure_feedback.rs` extracts measured domain/case/metric regressions and unchanged unsolved cases from previously replayed frozen evaluations. Delivered to the **next actual compiled worker** through `rsi/execution/guards.rs::fit_worker_prompt` | A diagnostic is a hypothesis, not root-cause proof. Original frozen evaluator alone authorizes adoption |
| RSI reproduction — [RSI repository](https://github.com/GaryTang32/RSI) | Noisy evaluation can choose unchanged candidates; held-out trials and no-change baseline guard selection | Verified paired outcomes, zero-gain feedback, repeated-failed-patch suppression, explicit inconclusive outcomes | Its small demonstrated task sets are not DOMINION performance measurements |

## Actual native SDK boundary

- `src/engine/agent_sdk.rs::DominionSdk`: in-process Rust state, persisted threads, `run`/`run_streamed`, `steer`, `cancel`. Calls the existing `chat::run_turn` executor directly.
- `src/api/sdk.rs` and `src/api/mod.rs`: public HTTP/SSE `/api/sdk/threads`, `/:id`, `/:id/history`, `/:id/run`, `/:id/steer`, `/:id/cancel`. Uses the SAME Rust engine and state as the React web UI.
- `sdk/typescript` (`@hushmind3/dominion-sdk`, currently source package): typed `DominionClient` with goals, sessions, jobs, approvals, event-watch, native threads, live owner steering and SSE. Non-idempotent POST runs are **never** retried automatically after disconnect.
- `sdk/rust` (`dominion-sdk`, unpublished crate): independent async Rust HTTP client, native SDK thread routes, SSE, `McpTool` and `ToolRegistry` for host-provided tools via the existing MCP stdio protocol. Does **not** spin up a substitute agent engine.
- `web/` remains a client, not a dependency of the model/planner/tool execution core.
- Model selection remains owner/provider configurable. The existing algorithm registry and RSI compiled-descendant controller are native Rust. Third-party policy engines are not imported into the product.

## RSI: executable counterexample gradient, not a prompt-only claim

`rsi/execution/worker.rs` loads `rsi_system::revalidated_history`; the recompiled worker calls `fit_worker_prompt`, which now calls `failure_feedback::reflective_feedback`.

1. Read at most 20 recent generations **with** `pairedMeasured`, `pairedRevalidated`, a source commit and actual per-case metrics.
2. For the selected domain, identify measured regression (worse direction) or stagnation (no improvement) at the **case ID + metric** level. A saturated 1.0 → 1.0 quality guard is not a new optimization target.
3. Deduplicate repeated observations; prefer severe regressions, then unsolved cases. Bound to four counterexamples per generation; use a one-case summary if token limits require it.
4. Deliver observed values, original candidate and source paths, and an explicit *unproven* repair hypothesis to the real next coding-model prompt; record how many were delivered in `rsiWorkerContextPlan` and `rsi_generation.workerFeedbackPlan`.
5. Generated patch still uses original worker tools → independent frozen worktree A/B → trusted evaluator → candidate adoption or rejection → the next compiled descendant. A helpful sentence in the prompt is **not** success evidence.
6. The existing `rsi_archive.rs` already implements per-case Pareto parent selection, `rsi_model_lineage.rs` controls approved model competition, and `rsi/learning/trials.rs` gates algorithm promotion by actual subsequent trials. The new feedback augments, not replaces, them.

**Non-goals:** API-model weight training, LoRA, finetuning, benchmark-oracle editing, reporting a gain without a verified paired replay.

## Real coding benchmark pipeline

`scripts/benchmark/swebench_dominion.py` runs actual Dominion autonomous goals against real Git commits:

```bash
# Export the OFFICIAL set using datasets; no fabricated instances.
python - <<'PY'
import json
from datasets import load_dataset
rows = load_dataset("princeton-nlp/SWE-bench_Verified", split="test")
with open("verified.jsonl", "w", encoding="utf8") as f:
    for row in rows:
        f.write(json.dumps(dict(row), ensure_ascii=False) + "\n")
PY

# Need a prebuilt native Dominion binary and a PRECONFIGURED, QUIESCENT
# per-task template of DOMINION_DATA with real provider credentials.
python scripts/benchmark/swebench_dominion.py \
  --tasks-jsonl verified.jsonl \
  --binary target/release/dominion-rust \
  --state-template /path/to/pristine-configured-runtime \
  --provider-id my-provider --model my-model --model-name dominion-my-model \
  --output benchmark/swe-verified-real --max-instances 10 --run-official
```

The runner clones/fetches the instance's actual repository, checks out the exact `base_commit`, uses a dedicated workspace/port/state per task, invokes the goal planner/executor, waits for a terminal result, and exports exact isolated Git patches through **`GET /api/goals/:id/patch`**. This endpoint uses a temporary Git index for tracked and newly created files, never changes the real checkout index, refuses incomplete/oversized/uncertain patch export, and emits patch bytes, SHA-256 and source commit. A JSONL prediction file is submitted to the **official Docker evaluator** `python -m swebench.harness.run_evaluation --dataset_name princeton-nlp/SWE-bench_Verified`. Separate run IDs are derived from patches to avoid stale evaluator caching.

The trace records observed session tokens, tool operations, duration, iterations, replans and owner-supplied **reference** KRW tariff quotes (when known). A failed agent still produces an empty patch prediction rather than fabricated success. **Resolved percentage stays `null`** unless the official grader completes and writes a valid result. Compare competitors only on identical instance sets, repository commits, tool/network privileges, model budgets and independent judge versions.

For a reference baseline, run the same script with a pinned earlier DOMINION binary, or score public prediction files with the **same official evaluator**. No benchmark result from a different agent setup is asserted to be directly comparable.

## Test commands and evidence boundary

```bash
npm install --prefix sdk/typescript --no-audit --no-fund
npm test --prefix sdk/typescript
cargo test --manifest-path sdk/rust/Cargo.toml --all-targets
python3 -m unittest discover -s scripts/benchmark -p 'test_*.py'
node --test scripts/champion-system-contract.test.mjs
cargo test --release
```

The repository CI now calls these SDK and benchmark export tests; a successful **GitHub file commit** is not a passing CI or an actual SWE-bench result. A release claim requires successful Rust/TypeScript build, real model runs and the external benchmark report. The SDK packages have not been published to package registries.

## 2026-11-15 measured rejected-candidate path and benchmark correction

`rsi_system/replay.rs` distinguishes replayed independent measurement (`pairedReceiptReplayed`) from adopted/source-validated status (`pairedRevalidated`). `rsi/learning/failure_feedback.rs` now uses the former for failures, including rejected candidate regressions; only candidates with the latter and an eligible accepted commit contribute prior success examples.

The benchmark runner now resolves output to an absolute path, requires the official grader's run-ID-specific `logs/evaluation/<run_id>/results.json`, and reports tool attempts, failed/uncertain operations and observed task recovery rates. [Draft validation PR #11](https://github.com/hushmind3/dominion-rust/pull/11). Neither an unexecuted benchmark nor static tests imply a SWE-bench resolved score.

