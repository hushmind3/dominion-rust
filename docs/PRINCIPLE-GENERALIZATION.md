# DOMINION experience generalization (2026-11-06)

This layer implements the identity requirement that experience must transfer to
new problems, not only replay one task's tool call.

## What is stored

`experience_principle` is a global, model-agnostic record extracted only from a
verified successful adaptation:

```json
{
  "kind": "verify-before-mutate",
  "failureClasses": ["stale_state"],
  "sourceDomains": ["source_control", "database", "http_api"],
  "principle": "변경 전에 대상의 현재 상태와 버전을 읽고 기대 revision과 일치하는지 확인한 뒤 수정한다.",
  "suggestedActions": ["read", "list", "get", "status", "head", "diff"],
  "supportingModels": ["model-a", "model-b"],
  "supportingAgents": ["agent-a", "agent-b"],
  "supportingCases": ["run-a", "run-b"],
  "counterexamples": []
}
```

The principle deliberately does **not** contain a provider/model gate, a source
tool name in its applicability, or old argument values. Matching is structural:

1. the failed tool mutates an external resource;
2. the failure class matches (`stale_state`, `not_found`, ...);
3. the selected verified intervention was an observation (`read`, `get`,
   `status`, `head`, ...).

## Lifecycle

```
verified adaptation (mutation failed -> observation succeeded)
  -> learn_from_execution creates/updates a global principle
  -> next mutation failure in any domain/model calls context_block
  -> principle_usage is persisted with execution, agent and model
  -> finalize_usage adds a supporting case on success, or a counterexample on failure
  -> two verified executions promote it into a global strategy candidate
  -> the existing experiment/promotion pipeline can validate or reject that candidate
  -> confidence/support grows; the principle stays available to all agents
```

The model sees the principle as a suggestion, never as old arguments to replay.
The existing adaptation loop still validates new arguments against the current
task.

## Verification

- `verified_github_failure_creates_a_global_principle`
- `principle_transfers_to_another_domain_and_model` — GitHub `stale_state`
  evidence is retrieved for `sql_update`, `http_post`, and `workspace_write`;
  three different models contribute supporting cases.
- `observations_do_not_receive_mutation_principles`

Full suite: 346 unit + 69 E2E + 16 benchmark.

## Still missing

- A same-model, same-tools, same-budget before/after task benchmark. The
  transfer is proven deterministically, not yet as a live model success-rate
  experiment.
- Strategy-layer generalization is connected through `improve::propose`, but
  promotion still depends on the experiment runner being enabled.
- Algorithm-layer promotion (for example a preflight verification kernel
  activated from the principle) is not yet automatic.
- Causal attribution for every algorithm domain; several still use domain
  signals that are proxies rather than downstream success.
