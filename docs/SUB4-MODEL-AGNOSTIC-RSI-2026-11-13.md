# Sub4 model-agnostic, evidence-conditioned RSI actors — 2026-11-13

## Why LoRA was not general RSI
A PEFT adapter only modifies *that local base model*. It never edits a GPT/Claude/Gemini API provider's weights. A local token-loss win is not evidence that the overall API-powered agent has improved.

## Executable next step
**rsi_model_ecology.rs** implements source-level *model-agnostic actor selection* inside the **running, rebuilt descendant's worker plan**.

- The campaign owner may enable \`modelEvolution=true\` with \`selectionPolicy=automatic\` and optionally \`modelProviderAllowlist=[provider IDs]\`. This is **independent** of \`allowLocalAdapterTraining\`.
- It discovers configured, enabled text/tool-capable model routes including multiple known IDs from a provider's \`models\` list. It preserves the owner's initial route as a baseline.
- It explores alternative real API/local model actors, then conditions selection on measured, source-backed downstream agent improvement from prior descendants. It distinguishes the chosen model API from modifications to its weights.
- The exact plan is validated by the coordinator and revalidated in the compiled worker. The next generation really uses the selected \`providerId\` + \`model\` when it calls the full existing ChatGPT-compatible, tool-capable DOMINION agent turn.
- The worker records the **actual provider/model/session token-usage events**. Only a generation with that evidence, an independently measured source evaluation, and revalidated accepted code may add a success.
- Regressions, failures and unexplored model alternatives feed next generation's posterior-smoothed selection. The model policy source is editable and recompiles, so a descendant can redesign the policy itself. Domain-specific evidence guards against conflating a memory problem and a code problem.
- A locally promoted PEFT adapter cannot override an independently selected external model on the basis of its private *base-vs-LoRA* loss alone. To earn general comparison credit, it needs the same whole-agent improvement measurements.

## Generalization and limits
This is a **real model-routing and outcome-feedback path**, not a completed cross-model paired agent benchmark; task mix, task difficulty, and source revision can confound model correlations. It does not claim weights of any remote proprietary model were changed or that learned route selection is causally superior. The evaluator and source adoption stay independent and frozen. No actual paid model calls, full CI or longitudinal performance experiment was executed by this commit.

Sources: HarnessDev (Hugging Face papers/2609.01437), Self-Evolving Harness on Multiple Tasks with the Agent as Its Own Optimizer (arXiv:2609.38372), Harness Bench (harness-bench.ai).
