# Algorithm measurements (2026-11-06)

Branch: `codex/native-memory-harness`

These are deterministic, model-free measurements produced by the Rust test
suite. They prove that each kernel changes observable behavior; they do not
claim a real-model quality win. Reproduce with:

```sh
cargo test --release --bin dominion-rust measurement_ -- --nocapture
```

## Measured results

| Area | Metric | Baseline | Selected kernel |
| --- | --- | --- | --- |
| Memory rerank | top-1 retrieval accuracy on a length-skewed 11-doc corpus (3 queries) | `none` 0/3 | `bm25` 3/3 |
| Memory compression | active records / active characters for 4 old low-importance memories | 4 / 285 chars | 1 / 177 chars (38% smaller) |
| Task decomposition | average quality over 3 multi-clause Korean requests | `single-step` 0.33 | `clause-split` 0.86 |
| Multi-agent conflict | expected-winner accuracy over 5 hand-built disagreement cases | — | 5/5 |

Supporting unit/E2E tests:

- `bm25_rerank_scores_documents_by_term_evidence`
- `compression_summarizes_old_memories_and_preserves_originals`
- `clause_split_decomposition_is_persisted_with_independent_quality` (E2E)
- `room_conflict_resolution_prefers_evidence_backed_claim` (E2E)
- `parallel_reads_combination_executes_independent_reads_in_one_round` (E2E)
- `normalize_paths_arguments_run_through_the_common_dispatch` (E2E)

Full suite at this commit: **328 unit + 64 E2E + 16 benchmark tests**, web
typecheck and production build clean.

## Not measured here

Real mixed local/API model quality and before/after task performance are not
measured in this document. They require user-selected local and API models,
credentials, and longer live runs; the current instruction set explicitly
forbids long model experiments without approval. The deterministic
measurements above are the evidence available without those resources.
