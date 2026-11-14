# SWE-bench Verified: native DOMINION real-model benchmarking

This runner is a real Git-editing/real provider adapter, not a mock coding
agent. It launches a separate Rust DOMINION server with an isolated SQLite
store and a separate disposable source checkout for every instance, executes
the existing autonomous goal planner/executor/reviewer, and records the actual
Git diff as an official-format model prediction. It does not claim a solution
until the upstream Docker evaluator grades the patch.

## Input prerequisites

- Node 20+, Git, compiled DOMINION Rust binary, an actual model/provider.
- Official SWE-bench Verified test-split JSONL as DOMINION_SWE_TASKS, with
  instance_id, repo, base_commit, problem_statement fields.
- Local precloned bare repositories in DOMINION_SWE_MIRRORS, named as
  owner__repo.git. For django/django use django__django.git, etc.
- DOMINION_SWE_PROVIDER_FILE: absolute JSON path containing actual provider
  configuration (baseUrl, model, kind, optional apiKey). DO NOT commit it.
- DOMINION_SWE_OUTPUT: a fresh absolute output directory. Never use the
  live workspace. The runner refuses to overwrite existing output.
- Set DOMINION_SWE_ENABLE=1 and DOMINION_SWE_BIN absolute executable path.
- Optional DOMINION_SWE_LIMIT, DOMINION_SWE_ITERATIONS,
  DOMINION_SWE_TIMEOUT_MIN, DOMINION_SWE_GRADE=1.

Run: node scripts/sub2-swebench-verified.mjs

Output: predictions.jsonl (official instance_id/model_patch/model_name_or_path
schema), report.json (per-task goal status, job/receipt, measured session
tokens, elapsed time, Git patch hash), and disposable checkouts. Failed jobs
still contribute token cost and still emit whatever real patch they created;
empty patches remain empty instead of fabricating a success.

With DOMINION_SWE_GRADE=1: invoke the official
python -m swebench.harness.run_evaluation --dataset_name
princeton-nlp/SWE-bench_Verified --predictions_path <predictions.jsonl>
--max_workers 2 --run_id <unique-run-id>

The upstream evaluator requires Docker, Python swebench and the official
dataset; it produces independent hidden-test results. A zero exit status
means the evaluator completed, NOT that a problem was solved. For a new patch
use a new run_id because the official harness caches results by run_id/ID.

## Fairness, RSI and controls

- Agent sees only issue problem_statement and pinned source commit.
- Official patch/test_patch/FAIL_TO_PASS/PASS_TO_PASS are not passed to model.
- Source is checked out from an operator-provided local Git mirror at exact
  base_commit; failures are recorded with no fake prediction.
- Each instance uses a new database so learned history cannot cross tasks.
- Exact binary revision and provider/model should be pinned externally in a
  reproducible experiment manifest for controlled comparisons.
- Model-generated claims or native goal status do not substitute for official
  Docker resolved-count results.
- Benchmark result may be used as **held-out** independent RSI validation;
  it must not feed per-case hidden test outcomes into subsequent mutation
  prompts or parent-selection scoring.
- No production workspaces and no third-party agent harness are used.
