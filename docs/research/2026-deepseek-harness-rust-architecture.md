# DeepSeek Harness source review → Dominion native Rust architecture (2026-11-14)

Official upstream: https://github.com/deepseek-ai/deepseek-harness (MIT; developer preview).
This review examines **source files**, not only product claims. Implementation ideas
are independent design adaptations, **not imports of the DeepSeek runtime or its rules**.

## Concrete source examination

| DeepSeek implementation | Code / meaning | Relevance for Dominion |
| --- | --- | --- |
| `docs/architecture.md` | Cordis plugin tree: scoped services and events; registrations unwind on unload. The agent loop, model adapters, tool registry, sessions, and policies are all plugins. | Prefer a first-party Rust trait-based `PluginRegistry` with scoped registrations and RAII unload; **do not embed Cordis/Node**. |
| `packages/core/agent-loop/src/agent.ts` | Explicit `turn/start → step/start → model → tools → step/end → turn/end`; each step has its own admitted tool set, and failure completes unanswered tool calls. | Split turn/step identities, record boundary events in the Rust journal, release per-step tool scopes. |
| `packages/core/agent-loop/src/inbox.ts` | Durable `next-turn` and `next-step` queues with event-backed splice, replace and claim operations. | An owner-issued steer must be **durable and consumed at a step boundary**; model output and tool result text must never forge owner input. |
| `packages/core/session/src/repair.ts` | `TOOL_NOT_STARTED` vs `TOOL_OUTCOME_UNKNOWN`; synthetic paired tool results close interrupted or forked history and leave side effects open to reconciliation. | Persist pre-dispatch and dispatched boundaries separately; read-only safe replay differs from write verification; avoid blind write retry. |
| `packages/core/tools/src/pipeline.ts` | Scope-resolved tool definitions and pre-execute admission; arguments snapshotted and frozen; concurrency safe **only on explicit exact true**, unknown defaults to exclusive. | Scope-approved registry + `ToolInvocation` immutable snapshot; read-only parallelism may be certified, writes exclusive. |
| `packages/core/scope/src/index.ts` | Parent-child scoping, inherited registrations and ancestor-visible events; dispose quiesces owned registrations. | Per-agent/per-session scoped plugins avoid other agents accidentally inheriting tools. |
| `packages/sdk/client/src/client.ts` | SDK owns spawned `dsh` subprocess, JSON-RPC stdio, notification subscription, EOF→termination cleanup. | Add optional first-party **Rust service supervisor** to `dominion-sdk`; current Rust SDK only connects to an existing Dominion service. |
| `packages/sdk/server/src/server.ts` | Server owns sessions, prompt admission, initialization and readiness, completion notifications, subagent lineage and shutdown. | Introduce independent Rust service-level SDK bridge while preserving the existing `/api` surface; add exact readiness and lineage. |

Source links:
- https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/inbox.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/session/src/repair.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/tools/src/pipeline.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/scope/src/index.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/client/src/client.ts
- https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/server/src/server.ts

## Implemented locally now vs gaps

Current Dominion branch contains `src/engine/goals.rs`, `goal_plan.rs`,
`engine/chat/runtime/turn.rs`, `src/engine/mcp.rs`, and `sdk/rust`.

- Existing: native Rust goal/executor/review loop, checkpoints, operation receipts,
  MCP stdio client, Rust SDK client, Rust MCP plugin server, cancellation/approval.
- New in this branch: direct owner plan pending check **after model response,
  before side-effecting tool calls**, with a safe next-iteration steer and
  paired superseded tool responses for calls already committed to history.
- Gap: no unified append-only `turn/step/tool` session event journal with
  replay-based reconstruction. `execution_operation` receipts and goal checkpoints
  are evidence, but not an identical global event-sourced state machine.
- Gap: current Rust SDK does not spawn, supervise or shut down its own Rust
  runtime process. Existing `dominion-sdk-tool-server` is an **MCP plugin server**,
  not the full embedded agent engine.
- Gap: no fully scoped, hot-reloadable native Rust plugin registry with
  `prepare → invoke → observe → dispose` hooks and parent-child scope isolation.
- Gap: owner steer is a plan field and safe-boundary check, not yet a generalized,
  durable next-step inbox supporting append/replace/stop with instruction IDs.
- Gap: strict per-tool concurrency capability attestations and exact child
  subagent ownership/teardown are not uniform across all built-in tools.

## Recommended implementation order (first-party Rust; not vendor SDK)

1. **Owner input journal**: `OwnerInput { id, source: Owner, session_id, task_id,
   target: NextStep|NextTurn, mode: Continue|Replace|Stop, text, consumed_seq }`.
   Persist before acknowledging admission; claim once with compare-and-swap.
   Never accept changes sourced only from model/tool outputs.
2. **Unified event ledger**: `RunEvent { run_id, turn_id, step_id, event_seq, kind,
   call_id, side_effect_state }`, append before dispatch and after settlement.
   Use `NotStarted`, `InFlightUnknown`, `CompletedVerified` to prevent
   double execution; reconstruct pending results after interruption or fork.
3. **Scoped plugin execution**: `DominionPlugin` trait providing schemas and
   async callbacks, independent `AgentScope`, exact detach/dispose lifecycle.
   A plugin exposes capabilities to the model only after upstream user/engine
   authorization; no new silent refusal policy from a vendor runtime.
4. **Explicit tool scheduling**: a tool defaults to exclusive. A plugin/builtin
   may declare read-only or `is_concurrency_safe(args)`, and parallel dispatch
   still obeys approval, task revision and owner-steer checks.
5. **SDK runtime ownership**: `DominionRuntime::spawn(binary, config)` manages
   the **Dominion Rust binary** with bounded health/readiness and shutdown;
   keep `DominionClient::new(base_url)` for remote instances. Do not launch
   DeepSeek/Codex/Copilot under the hood.
6. **Native Rust integration proof**: run a real goal with a scripted model,
   have it propose 2 side-effecting tools, steer the owner between them,
   verify the second proposal did not execute and every call ID has a
   matching result; then crash/restart and prove no duplicate write.

## Architecture choice

Keep Rust as the execution authority. DeepSeek's all-plugin Cordis model is
a useful **composition pattern**, but reimplement the necessary traits,
scopes, event store, and lifecycle in Rust. Use the existing Dominion tools,
workspace boundaries, MCP and approval state; do not bind runtime behavior to
a foreign agent or foreign provider policy. “Follows the owner” means it
**does not silently drop or override authorized user instructions**; user
steering is higher priority than model proposals, and actual tool outcomes
still require verified receipts instead of textual completion claims.
