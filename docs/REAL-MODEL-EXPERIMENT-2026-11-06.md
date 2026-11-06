# Real free-API + local model experiment (2026-11-06)

Branch: `codex/native-memory-harness`

## Question

Does the selected memory rerank kernel change what the model actually sees and
answers, on a real model, at zero API cost?

## Setup

- Isolated engine instance on a free port with a temporary data root and
  workspace. The production instance on `4317` and its DB were not used.
- Providers:
  - **Free API**: Pollinations `https://text.pollinations.ai/openai`,
    model `openai-fast` (GPT-OSS 20B), anonymous free tier, no key.
  - **Local**: Ollama `llama3.2:3b`, `toolMode=compat`, no tools used.
- Corpus: 21 synthetic global memories inserted into the isolated DB:
  3 questions × (1 gold memory + 6 short distractor memories). Each gold
  memory contains a unique code that is not present in the question or the
  distractors.
- Each run used a fresh session and one model call. `memory.rerank` was
  restricted to a single kernel (`none` or `bm25`) before the run.
- Retrieval inclusion was read from the engine's
  `selection_snapshot.contextItems`, not inferred from model prose.

## Results

### Retrieval inclusion (deterministic, provider-independent effect)

| Provider | `none` | `bm25` |
| --- | --- | --- |
| Pollinations free API | 0/3 | 3/3 |
| Ollama llama3.2:3b | 0/3 | 3/3 |
| **Total** | **0/6** | **6/6** |

With `none`, the gold memory was below the top-6 cutoff in every run and was
never delivered to the model. With `bm25`, the gold memory was delivered in
every run.

### Answer accuracy (secondary, noisy)

Two runs were performed (before and after the concurrent memory/rerank fixes):

| Provider | `none` | `bm25` run 1 | `bm25` run 2 |
| --- | --- | --- | --- |
| Pollinations free API | 0/3 | 1/3 | 2/3 |
| Ollama llama3.2:3b | 0/3 | 2/3 | 1/3 |

Answer accuracy varies between runs; retrieval inclusion did not (0/6 vs 6/6
in both runs).

Notable behavior:

- The free API model spent most of its output on reasoning. Two baseline runs
  and one `bm25` run produced empty final content, so answer accuracy
  understates retrieval success.
- The local 3B model sometimes dumped the retrieved evidence instead of a
  one-line answer. The code was present in its answer text in those cases.
- Clean free-API successes on the second run: `bm25` answered both
  `token expiration audit code` and one other question with the exact code.
- The first run answered `token expiration audit code` exactly `EAGLE9021`.

### Latency and cost

- Pollinations free API: ~8–57 s per call (reasoning-heavy), `$0`, no key.
- Ollama llama3.2:3b: ~2–7 s per call, local, `$0`.

## Reproduction notes

- The driver lived outside the repository (temporary script) and used only
  HTTP APIs plus direct SQLite inserts into the isolated DB.
- Raw result JSON was written to the session temp directory, not committed.
- Sample size is 3 questions per configuration. This is evidence that the
  kernel changes real model context and answers, not a general quality claim.
- Paid APIs were not used, per instruction.

## What is still not measured

- A larger task set and multiple free/paid models.
- End-to-end task success (file edits, tool workflows) under each kernel.
- Statistical significance. The clean, deterministic retrieval result is the
  reliable signal here; answer accuracy is directional.
