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

Full suite after partial adoption: 350 unit + 73 E2E + 16 benchmark.

## Opt-in file guard (partial adoption)

This branch adopts only the verified mechanics from the `rust` experiments:

- `workspace_read`/`file_read` record the canonical path and SHA-256;
- `workspace_write`/`file_write` serialize per target path and reject a blind
  overwrite or a write after the observed bytes changed;
- an inline observation returns the current small-file content and hash so the
  model can retry without a separate read;
- experiment settlement can check exact file bytes, read/write order and a
  1.25× cost ceiling when a preflight request is evaluated.

The guard is **opt-in**. `mutation.preflight` is not promoted automatically,
no preflight candidate is queued from principles at startup, and no exact
replacement executor runs in the normal chat path. A policy must be registered
and activated explicitly; otherwise file writes use the existing path.

Automatic preflight promotion, deterministic exact replacement and scheduler
expansion remain isolated in `rust` for further experiments. The guard-only
model experiment was parity (5/10 vs 5/10), so it is not evidence of a
success-rate improvement.

## Still missing

- A same-model, same-tools, same-budget holdout where preflight improves
  success without a deterministic shortcut.
- Strategy promotion remains connected through `improve::propose`, but
  promotion still depends on the experiment runner being enabled.
- Causal attribution for every algorithm domain; several still use domain
  signals that are proxies rather than downstream success.
