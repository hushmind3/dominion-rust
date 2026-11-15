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
