# DOMINION Sub2 comparative harness design — 2026-11-14

## Code inspected, architecture selected

This is architecture analysis of actual source, not a claim of copying SDK behavior.

| Origin | Repository paths inspected | Design worth integrating | Decision |
|---|---|---|---|
| US: OpenAI Codex | `codex-rs/core/src/tools/registry.rs`, `agent/control/mailbox.rs`, `exec.rs`, `context/user_instructions.rs` | Typed tool executor, life-cycle tracing, cancellation and persisted owner-facing instruction | Implement own Rust callback registry and preserve original owner input; keep existing native cancellation and evidence |
| US: OpenAI Agents | `src/agents/lifecycle.py`, `run_internal/run_loop.py`, `tool_execution.py` | Model/tool hooks, tool identity and per-run state | Integrate tool entry at native discovery and dispatch rather than add a second model loop; add independent hooks later |
| US: Microsoft Agent Framework | `python/packages/core/agent_framework/_middleware.py` | Explicit tool failure vs unrecoverable middleware failure | Tool callbacks return typed Rust `Result`; existing dispatch records error and feedback without claiming success |
| China: AgentScope | `src/agentscope/middleware/_base.py`, `_budget.py` | Lifecycle middleware and budget control separate from core | Registry stays independent of the model/provider; do not transplant third-party middleware policies |
| China: DeerFlow | `docs/ARCHITECTURE.md`, `backend/packages/harness/deerflow/subagents/AGENTS.md` | Harness/app boundary, sandbox scope, leasing and result verification | Use one Rust engine under SDK and HTTP; preserve native audited delegation, worktrees and verification |
| China: Qwen-Agent | `qwen_agent/llm/fncall_prompts/qwen_fncall_prompt.py`, `nous_fncall_prompt.py` | Legacy textual tool-call envelopes for local LLMs | Keep DOMINION's own `<tool_call>`/DSML parser; register schemas in the same native model tool catalog |
| China: OpenManus | `app/tool/tool_collection.py`, `app/agent/toolcall.py` | Small tool registry, model-tool-result cycle | Native Rust `Registry` maps names to callbacks; errors return to the existing model loop |

## Native execution path (connected, not a wrapper)

```
owner request verbatim
  -> Rust TurnRequest::from_value
  -> Rust model/tool planning
  -> native built-ins + SDK registered tools + MCP definitions
  -> provider call
  -> parsed native or local-model textual tool envelope
  -> Rust dispatch_tool_call
     -> approval for side-effectful SDK tools when owner configured
     -> effect receipt (custom callbacks conservatively effectful)
     -> Registry::execute: JSON object argument validation + async Rust callback
     -> tool_end or tool_error with persisted execution operation
  -> next model step, deterministic verifier, persisted assistant result
```

`sdk__` is an exclusive namespace; duplicate names are rejected. The registry is
per-`AppState`, shared via `Arc<RwLock<...>>` across concurrent workers, and
released before executing callbacks. No Python, Node.js, JS SDK or
Copilot/Claude/Codex execution runtime is invoked to run the callbacks.

## User command priority and provenance

The actual owner request remains a `user` role message, recorded with
`authorKind=owner`; it is not replaced by a planner's derived instructions.
`TurnRequest` now preserves the input string's surrounding whitespace and
newlines while validating that it is not wholly blank. Model responses and
tool-returned prose do not gain owner authority. Neither the Rust SDK nor the
registered-tool module adds a general content-classification refusal policy.
Physical side-effect approval is a separate, configurable execution concern,
not a model answer policy.

## Existing capabilities preserved / future research decisions

- Maintain DOMINION's existing per-goal worktree, deterministic evidence, RSI
  independent evaluation, MCP and group multi-model orchestration.
- Future: lifecycle hooks per stage, cancellation-safe sandbox process scopes,
  expensive-model selection by actual cache-aware usage and completions.
- SDK callback definitions are in-memory code, not a durable executable plugin
  manifest; restarted processes must re-register their Rust callbacks.
- Registry supports a documented JSON-schema subset (object/properties/
  required, nested arrays, primitive types, enum), not every Draft 2020-12
  combinator.
- Real-provider end-to-end coding quality and cost are *not* established by
  mock-provider unit tests.

## Source links

- https://github.com/openai/codex
- https://github.com/openai/openai-agents-python
- https://github.com/microsoft/agent-framework
- https://github.com/agentscope-ai/agentscope
- https://github.com/bytedance/deer-flow
- https://github.com/QwenLM/Qwen-Agent
- https://github.com/FoundationAgents/OpenManus


## DeepSeek Harness: deepseek-ai/deepseek-harness (China, official, MIT)

Inspected source code on `master` on 2026-11-14:

- `packages/core/agent-loop/src/agent.ts`: `ReactLoopAgent.send`,
  `followup`, `steer`, `inject`, `cancel`, `preStep`,
  step model calls and event publication.
- `packages/core/agent-loop/src/inbox.ts`: two durable inbox lanes,
  `next-turn` and `next-step`, reconstructed from `agent/inbox/spliced`
  events with unique message IDs. Claimed input is not a rewritten instruction.
- `packages/core/agent-loop/src/runtime-context.ts`: stable system prompt
  projection and explicit runtime-context supersession, avoiding stale context
  after a resumed turn.
- `packages/core/agent-loop/src/assistant-stream.ts`: explicit
  `committed` vs `abandoned` terminal events, preserving partial streamed
  text and usage across cancellation.
- `packages/compaction/compaction/src/tool-pairing.ts`: compaction only
  crosses balanced tool-call/result boundaries to avoid broken histories.
- `packages/context/agent-instructions/src/index.ts`: workspace instruction
  baseline and change-aware instructions projection.
- `packages/core/agent-loop/README.md`: the model/tool loop is a replaceable
  plugin over independently owned session, tools and prompt services, with
  cooperative cancellation, bounded parallel tool execution and per-session
  event-derived state.

### Resulting Rust design

**Adopt now**: owner-initiated live steering with an append-only SQLite record
and exactly-once message identity:
`DominionSdk::steer` or `POST /api/sessions/{id}/steer`
-> `engine::steering::enqueue` -> SQLite `chat_steer` queued record
-> every native model-step boundary `drain_at_step`
-> persistent, original user-role message
-> `messages.push` without transformation
-> user correction is visible to the next native model request. If the turn
already ended, the correction remains queued for the next turn.

**Next improvements**: per-session plug-in lifecycle hooks; explicit stream
commit/abandon event; compaction boundaries with tool-call pairing; user
settings and workspace files projected with revision hashes instead of
duplicating old instructions; callback tool timeout and in-flight shutdown
drain.

**Deliberately reject**: importing Cordis/Node.js into the Rust engine;
inheriting DeepSeek's host policy or instruction-precedence implementation;
treating provider outputs as authenticated owner instructions. The owner's
original instruction has priority over stale engine plan text while existing
file/system permissions remain enforced separately.

Source: https://github.com/deepseek-ai/deepseek-harness
