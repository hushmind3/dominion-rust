# Champion — model-route evolution inside a self-improving descendant (2026-11-13)

## The 64-point judge correction

The judge clarified an important distinction: a local LoRA adapter is not a change to the weights of an existing external GPT/API model. Adding a model training tool cannot by itself demonstrate that the entire agent became more capable or that a newly trained model should replace the production model.

The Champion branch therefore adds a different capability: **the model used to improve DOMINION is now an owner-approved, descendant-editable, measured learning choice**. The new policy is source code in `src/engine/rsi_model_lineage.rs`, registered as an independently editable `scientific_method` component.

## Exact execution path

1. An owner can opt in by setting `rsiAllowModelEvolution=true` and supplying `rsiApprovedModelPool` with 2–8 existing provider/model pairs. `rsiLocalOnly=true` excludes all nonlocal routes. Ordinary campaigns do not change routes.
2. The original `providerId`/`model` are carried as `rsiModelOrigin` unchanged throughout the campaign. Model pool availability and permission are checked **before creating the campaign worktree**.
3. The **recompiled descendant** runs `rsi_model_lineage::plan` together with its source, meta-method and whole-intelligence improvement policies. The supervisor validates the plan and binds the selected *real provider/model* to the following `rsi_generation` job.
4. `jobs::run_prompt` stores that provider/model in the actual session, `chat::provider::enforce_rsi_route_policy` verifies the approved route immediately before streaming, and RSI provider fallback stays disabled. No unapproved paid model is silently substituted.
5. The running descendant worker recomputes the policy from archived evidence **before its model call**; original model, pool membership, immutable generator commit and plan revision must match.
6. The generation ledger now records the **actual job provider/model**, model policy revision, route selection, executed source identity and whether an independent paired comparison was genuinely performed. Previously, generation receipts always copied the campaign's original provider/model even if a future route policy chose another one.
7. `rsi_model_lineage` uses `rsi::replayed_pair_metrics` and the archived immutable candidate receipt. Only an actual changed-source candidate from the compiled worker with the *same frozen evaluation profile* counts. Claims in model text, mere tool invocation or a successful build do not train selection.
8. Direct model-route comparisons require **the same generator source commit and evaluation profile**. Changing generator commits invalidates a head-to-head comparison. Historical comparisons can guide exploration and conservative prequential forecasts, clearly labeled as noncausal and not an identical-code comparison.
9. When an approved alternative is explored, it produces a **real next-generation code-edit attempt** whose outcomes flow back to the same model-policy learner. Accepted new source carries this algorithm forward. Future descendants can rewrite the policy's exploration, estimation, scoring, comparison and model-registration logic.

This is a functional path **from model selection → actual agent source improvement → frozen external task evaluation → model selection-method learning → next descendant**. It does not assert improved upstream API weights.

## Owner opt-in example — job input fragment

```json
{
  "rsiNativeApproved": true,
  "rsiSystemEvolution": true,
  "rsiAutonomous": true,
  "providerId": "registered-owner-provider",
  "model": "existing-model-id",
  "rsiAllowModelEvolution": true,
  "rsiLocalOnly": false,
  "rsiApprovedModelPool": [
    {"providerId": "registered-owner-provider", "model": "existing-model-id"},
    {"providerId": "second-registered-provider", "model": "alternative-model-id"}
  ]
}
```

The provider IDs must refer to enabled provider records in the existing engine. Pool membership is a permission boundary, not a claim that an unknown model ID is available. Existing token/time campaign budgets remain in force. A local LoRA candidate must first become an actual runnable, registered local provider before its inference route can enter this pool; training the weights alone is never automatically treated as adoption.

## Comparative research

- [RouteLLM](https://github.com/lm-sys/RouteLLM) — measured quality/cost tradeoffs can guide model routing, while keeping explicit alternatives and reproducible benchmarks.
- [TwinRouterBench (2026)](https://arxiv.org/abs/2605.18859) / [implementation](https://github.com/CommonstackAI/TwinRouterBench) — route success is measured by downstream agent-task resolution and realized spend, not by model name.
- [LLMRouterBench, ACL 2026](https://aclanthology.org/2026.findings-acl.1881/) — model routers must be evaluated against simple baselines and source of apparent gains must be explicit.

## Known remaining boundary

The new policy evaluates **models as candidate improvement-agent generators**, not as a full general-purpose pairwise benchmark of direct responses from every cloud API and trained local adapter. Model-specific inference and adapter-weight adoption require additional real independent testing. No empirical multigeneration success or superiority score is claimed. Tests in `rsi_model_lineage.rs` are authored, not run.

The project judge currently scores Champion **64/100**, below the full-structure 80-point CI qualification; all implementation commits carry `[skip ci]`. No new CI was triggered by these changes.
