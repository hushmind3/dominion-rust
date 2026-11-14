# Dominion native autonomous coding benchmark

This is a **real prediction-producing runner**, not a mock success-rate script.
It starts the **native Dominion Rust server** separately for each public SWE-bench
instance, creates a sealed single-commit Git repository and SQLite database, configures the
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
  --approve-tools workspace_write,workspace_replace,workspace_replace_range,workspace_replace_function,shell_exec,terminal_exec \
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
The mirror is consulted only during trusted preparation. A temporary checkout
supplies the exact base revision tree, then the runner constructs a **new Git
object database with exactly one root commit**, no remote or Git alternates.
The original Tree SHA is checked against the fresh commit Tree SHA; checkout
filter behavior or missing files cause the task to stop instead of silently
changing the baseline. The temporary mirror-linked checkout is removed before
the autonomous Rust agent launches. The internal goal worktree and prediction
patch use the sealed commit SHA; the original SHA is retained only as external
benchmark provenance. This prevents accessing future reference commits through
local Git history. It does not prohibit network requests or other external
references; evaluate in a locked-down container/VM for a comparable score.

Git worktrees alone are not VM or network isolation.
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

## Explicit owner tool permissions

Unattended goals would otherwise wait for interactive approvals. The operator
must provide `--approve-tools`, which is persisted as a per-task native
`POST /api/policy` record. Only local workspace edits and chosen shell tools
can be authorized; arbitrary file writes, GitHub mutations, MCP and browser
control are not implicitly allowed. You are authorizing execution of arbitrary
model-proposed shell code if `shell_exec` or `terminal_exec` is included.
**Execute the runner itself inside your own container/VM** before granting
shell execution for untrusted SWE-bench repository tests. A Git worktree is not
an operating-system sandbox.


## 2026-11-15: same-task independent Docker comparison

Run the native prediction producer with --dataset verified and the exact
official Verified test-set JSONL. Use identical model, provider type and URL,
max token budget, iteration and replanning limits, timeout, tool approvals
and source isolation for both versions. Then independently grade each output
using the official Docker evaluator, with DIFFERENT unique run IDs. The
upstream runner caches results by run ID and case; do not reuse an old run ID
for a changed patch.

The comparator reads manifest.json, records.jsonl and predictions.jsonl
from each Dominion run; and run.json plus each original report.json from
the upstream logs/evaluation/RUN-ID directory. It verifies exact native
patch bytes and SHA-256, identical instance lists and original Git commits,
dataset identity, grading split and report booleans. Missing reports for
nonempty patches are errors, not zero scores or fabricated successes.

Example command (replace paths with actual generated files):

    node scripts/swebench-official-compare.mjs \
      --left benchmark/runs/base --left-logs logs/evaluation/base-eval \
      --right benchmark/runs/candidate --right-logs logs/evaluation/candidate-eval

Output reports paired wins/losses, official resolved fraction, native model
tokens, tool calls and separate condition-match flags. The controlled
harness-comparison flag becomes false if the provider/model, budgets,
approved tools, or sealed Git repository mode differs. Missing usage remains
null. Native completed goals and nonempty patches do NOT become scored solves.

For official 2026 CLI paths and per-instance result structure:
https://www.swebench.com/SWE-bench/guides/evaluation/
https://www.swebench.com/SWE-bench/reference/cli/

Verified is a historically exposed public benchmark; supplement with
SWE-bench Pro and fresh independent issue suites.
