# Comparable harness task runner

Use the same real model, budget, maximum steps, tool permissions and fixture
for each harness. The runner copies each fixture into a fresh workspace,
executes argv without a shell, and independently runs its acceptance tests.

Selftest:
    python3 benchmarks/compare.py --selftest

Set operational commands in matrix.example.json and save as matrix.json;
example placeholders are not runnable and do not imply a Codex/Dominion score.

    python3 benchmarks/compare.py --matrix benchmarks/matrix.json --repeats 3

A leaderboard is defensible only if both tools really enforce matching
model, budget, permissions and timeout. The runner reports token/cost values
only when an actual .dominion-benchmark-usage.json is written by an adapter.
Null means unmeasured, not free. An isolated verifier checks real artifacts.
SWE-bench Verified must use its official evaluation runner when applicable.

Mock-provider E2E, public MCP network tests, and fsync throughput are
engineering evidence, not frontier reasoning benchmark results.

Dominion adapter: benchmarks/dominion_adapter.py uses the genuine Rust
POST /api/v1/runs/from-repository endpoint. Start the Rust server first and
set its DOMINION_WORKSPACE_DIR to the runner's result workspaces directory
(default benchmarks/results/workspaces as an absolute path). Configure the
real provider model, token/limits and permissions yourself. The adapter
initializes each copied fixture as a Git checkout, submits an isolated
worktree run, exports only its changed files back to the verifier workspace,
and records measured usage. Unapproved shell/MCP actions are never silently
authorized by the adapter. The example matrix supplies a usable Dominion
adapter argv; Codex requires its own installed and authenticated CLI.

The runner detects modifications to protected acceptance test files (the
tests directory by default), restores the trusted fixture before verification
and rejects the attempt if those files were tampered with. Missing metering
is reported as null rather than assumed to be zero.

## Opt-in live 100-owner-turn semantic Room recall (Sub4)

The compiler-only Room context tests do not prove actual LLM semantic recall. To
run a **real 100 owner turns + final probe** against the configured provider,
with two alternating room participants and exact early-fact/later-correction
grading, explicitly opt in:

    cargo run --locked --bin dominion_room_recall_eval -- \
      --confirm-live-model --budget-krw 5000 \
      --context-budget-tokens 4000 \
      --output benchmark-results/sub4-room-recall.json

Configure the regular DOMINION_BASE_URL, DOMINION_MODEL and DOMINION_API_KEY (or a
loopback local model server). Configure actual input and output KRW-per-million
tariffs as well; if pricing is truly unknown, explicitly add --allow-unpriced.
An unpriced 0-KRW receipt does NOT establish that inference was free. The
Room's native post-inference budget gate cannot prevent a final provider
request from exceeding the declared KRW limit; the report retains observed
billed usage, unmetered/caching flags and any partial progress.

The evaluator itself makes no additional paid judge request. It uses 100
completed actual owner/assistant turns, injects a random permanent audit ID
early and a random target correction at turn 89, then strictly compares a
final JSON answer to the two original source facts. The report is written
even on an observed Room/model failure after startup. No result is implied
until this opt-in binary runs against a real configured model.
