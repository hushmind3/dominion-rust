# Sub4 reproducible live-model coding benchmark

This optional executable compares **actual coding task completion**, rather than mapping static source structure or unit tests to a capability score.

## Required setup

Put a clean, committed Git checkout **under** \`DOMINION_WORKSPACE_DIR\` (default \`./workspace\`). Its relative path goes in a JSON manifest. A source repository with uncommitted edits is never used as the execution workspace; each Run receives a detached Git worktree under \`workspace/<run-id>/\` at a fixed revision. Your original checkout is not modified.

Example manifest \`coding-tasks.json\`:

\`\`\`json
{
  "cases": [
    {
      "id": "fix-two-files",
      "repository": "bench-projects/demo",
      "instruction": "Inspect src/alpha.rs and src/beta.rs, fix the defect in each, run the tests and summarize only actual results.",
      "expected_files": ["src/alpha.rs", "src/beta.rs"],
      "verify": [["cargo", "test", "--offline"]],
      "budget_krw": 300,
      "max_steps": 25,
      "timeout_seconds": 900
    }
  ]
}
\`\`\`

Prepare the same repository SHA, manifest, model/provider endpoint, task budgets and tool permission policy for every branch. Run with a configured live model (external API key or local endpoint):

\`\`\`sh
DOMINION_WORKSPACE_DIR=./workspace \
DOMINION_MODEL=my-real-model \
DOMINION_BASE_URL=http://127.0.0.1:11434/v1 \
cargo run --release --bin dominion_coding_eval -- \
  --manifest coding-tasks.json \
  --output benchmark-results/sub4-coding.json \
  --approve-tool write_file --approve-tool edit_file
\`\`\`

Tool permissions are **explicit opt-in**, never silently relaxed. Tools such as \`shell\` may still be disabled by native policy; the resulting approval/blocked status is recorded. The evaluator does **not** automatically reconcile uncertain external writes. Allowed tools only operate through the existing native tool approval, workspace and receipt infrastructure.

## Verifiable results

For each case, the evaluator requires all 3: (1) a **durably completed** Run, (2) the named source files actually changed relative to the baseline Git HEAD, and (3) independently executed **manifest-supplied argv verifiers** to return success. Externally executed verification is run by the evaluator, not inferred from model claims.

JSON results include status, any pending approval/block, commit/diff context, changed paths, verifier stdout/stderr excerpts, per-case Run token use, cached tokens, recorded KRW cost, whether the local native tariff was configured, tool approval count, and elapsed time. Unpriced models are **not** called free. A missing model key, unsatisfied verification or unresolved approval fails the suite instead of producing an inflated success number.

The fixture-driven \`src/engine/benchmark_tests.rs\` remains a kernel regression, **not** live-model performance. To score the next judge round, run the live manifest on sub3/sub2/sub4 with identical source, tasks, model and permissions and compare these JSON reports.
