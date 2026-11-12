# Champion — verified model/harness co-evolution bridge (2026-11-13)

The champion branch already has a mutable meta-controller, causal experiment search and independent paired source adoption. This integration adds an **owner-opted-in real model-weight lineage** from the sub4 prototype, without confusing remote API route selection with weight adaptation.

## Runtime and evidence boundary

1. The owner must explicitly pass `allowLocalAdapterTraining=true` to an ongoing RSI campaign and stage a SHA-256-bound training manifest/data under the approved data root. The compiled descendant can use `rsi_train_local_adapter` to check/train local PEFT weights. Its own editable Python training program may evolve in later source revisions.
2. The untrusted trained weight candidate contains a hash-bound artifact but **no quality or promotion claim**. The parent-only trusted Python evaluation compares it to the base model on separately approved private multidomain holdouts, rejects per-domain regressions, and persists the result.
3. Only a campaign-scoped independently promoted candidate is eligible for the next compiled descendant. A local inference sidecar loads the actual PEFT weights; it is neither registered as a production provider nor reused outside the detached child.
4. The generation's provider-specific chat model usage is bounded by a monotonic SQLite audit interval and its session/job identity. A launched server without actual exclusive calls does **not** count as model-weight inheritance.
5. Only an independently paired source-code evaluation may adopt the child's harness changes. The model-weight evaluation and source-code evaluation are distinct outcomes. Campaign rollback avoids repeating a promoted weight path after multiple measured failures.

Modules: `rsi_model_weights.rs`, `rsi_evolve.rs`, `chat/provider.rs`, `jobs.rs`, `tools.rs`, `db.rs`, `api/rsi.rs`, `scripts/champion_adapter_*.py`.

External research backdrop: [RSI-Env (2026-11)](https://arxiv.org/abs/2611.01736), [HyperAgents (2026)](https://arxiv.org/abs/2603.19461), and [Mendel Gödel Machine (2026)](https://arxiv.org/abs/2608.07645). This implementation reuses the existing sub4 weight-prototype logic while retaining champion-specific source/evaluation constraints.

**Verification:** Code integration only. CI and live model/weight tests were intentionally not run; no model improvements or sustained self-improvement are claimed.
