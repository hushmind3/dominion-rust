# Dominion native autonomous coding benchmark

This is a **real prediction-producing runner**, not a mock success-rate script.
It starts the **native Dominion Rust server** separately for each public SWE-bench
instance, creates an isolated Git checkout and SQLite database, configures the
selected real provider, asks Dominion to solve the original issue, extracts the
actual resulting Git patch and writes the official JSONL prediction format.

## Prepare

1. Build Dominion: \`cargo build --release\`.
2. Download the public instance JSONL (e.g. SWE-bench Verified), independently.
3. Prepare a local complete Git mirror for each repository under
   \`./mirrors/<org>__<repo>.git\` with \`git clone --mirror\`. The runner never
   downloads reference patches or official test solutions. The user controls
   repository/network access and task selection.
4. Supply a working model endpoint and sufficient API credit (if applicable).
   Keys are read from the named environment variable and stored in the
   per-task native vault. No API call is made during test-contract checks.

## Run a real model and the actual source-edit engine

\`\`\`sh
export DOMINION_BENCH_KEY='your-provider-key'
node scripts/swebench-dominion.mjs run \
  --instances ./benchmark/verified.jsonl \
  --repos-root ./mirrors \
  --binary ./target/release/dominion-rust \
  --provider-url https://api.example.com/v1 \
  --model your-real-coding-model \
  --api-key-env DOMINION_BENCH_KEY \
  --output ./benchmark/runs/dominion-verified \
  --max-instances 50 \
  --token-budget 150000 \
  --iterations 16 \
  --timeout-seconds 1200
\`\`\`

Outputs: \`manifest.json\`, \`predictions.jsonl\`, \`records.jsonl\`,
\`summary.json\`, and separate \`instances/<id>/server.log\` plus per-case
durable state. The manifest records the dataset SHA-256, engine identity,
model and budgets. JSONL predictions contain the **real Git diff** produced
by the agent, including untracked files. Errors produce an empty prediction
with a diagnostic record, not a fabricated patch or an invented pass rate.

The real API path is \`POST /api/providers\` → \`POST /api/goals\` → native
\`goals::run\` → source-edit tools → \`GET /api/goals/:id\` → Git diff.
\`GET /api/benchmark/identity\` binds the runner to the exact child process,
workspace and SQLite directory, preventing accidental submission to another
local Dominion server.

## Score with official Docker evaluator

\`\`\`sh
pip install swebench
node scripts/swebench-dominion.mjs evaluate \
  --predictions ./benchmark/runs/dominion-verified/predictions.jsonl \
  --run-id dominion-verified-2026 --dataset verified --workers 4
\`\`\`

Equivalent official CLI: \`swebench eval verified -p predictions.jsonl
--run-id <id> -j 4\`. Results/resolve rates must come from **that official
Docker evaluation**, not the engine's internal completion verdict. Run competing
harnesses on the identical task list, exact provider model, budgets and
evaluation images before comparing. Public tasks may have contamination and
problem/test quality issues; prefer reporting a second fresh/private task set.

The runner is an **evaluation coordinator**, not an alternative agent harness.
All planning, tools, memory, models and approval decisions remain in Rust.
Git worktrees separate source changes but are not VM or network isolation.
For contamination-resistant evaluation, isolate the **whole process** in a
container/VM with the evaluator's network rules. Provider invoices and actual
Korean-won totals remain unmeasured unless separately reconciled.

## Contract tests

\`node --test scripts/swebench-dominion.tests.mjs\` verifies public-only
prompt construction, task-path handling, exact goal fields and reporting
semantics. This test suite does not claim paid-model runs or official passes.

## Evidence-led recovery and missing usage

The runner reads the server's persisted `execution_operation` records to
count failed calls, unknown side effects, verified effects and recovery. A
recovery counts only when a later successful call in the same execution
addresses the same resource. Missing token or tool telemetry is `null`,
**never** zero. Internal goal completion is not used as external resolution.
Korean-won billing is likewise `null` until independently reconciled.
