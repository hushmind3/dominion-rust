# DeepSeek Harness source review → DOMINION Rust adoption plan

Date: 2026-11-14. Reference: [DeepSeek-AI/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness) at \`master\` (Git tree \`39163dd30f703a79079e2d2446545cff3c453738\`). **Source review and intended design; not a claim that every listed feature is implemented.**

## Read: source-level mechanics, not just marketing

| Original source | Observed implementation | Why it matters in DOMINION |
|---|---|---|
| [docs/architecture.md](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md) | Cordis service/event/effect plugins, ordered profile patches, no privileged agent-loop core. Three event domains: durable session, live agent, and capability. A model-visible input must be committed to the session log. | Make the Rust agent loop replaceable **through a typed interface**, but keep the user-root instruction and durable journal engine-owned. |
| [packages/core/agent-loop/src/agent.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts) | Turns contain steps; each step claims queued inputs, fixes model route/context, records messages, streams one model request, settles tools, and decides whether another step is owed. | Refactor a large \`chat/runtime/turn.rs\` into explicit step boundaries/typed states rather than growing the monolith. |
| [packages/core/agent-loop/src/inbox.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/inbox.ts) | A durable projection of \`agent/inbox/spliced\` events maintains distinct \`next-turn\` and \`next-step\` queues; message IDs must remain unique. | **Top priority:** an authenticated owner can steer an *in-flight* coding goal at the next safe step. Current DOMINION has versioned \`goal_steering.rs\` but no generalized durable step inbox. |
| [packages/core/agent-loop/src/runtime-context.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/runtime-context.ts) | Runtime context uses a typed message-source and supersession projection; prompt variants are reconciled by the provider's capability. | Prevent volatile memory, runtime notices, and other agent text from taking over the owner's instruction. Keep stable/cacheable prefix. |
| [packages/session/session-checkpoint-policy/src/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/session/session-checkpoint-policy/src/index.ts) | \`sessions.flush\` gates the model request and top-level tool dispatch; cancellation checked *after* persistence and *before* dispatch. | Attach durable checkpoint gates to our Rust model and tool boundaries, with no duplicate side effects on restart. |
| [packages/session/session-checkpoint-policy/tests/crash-recovery.e2e.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/session/session-checkpoint-policy/tests/crash-recovery.e2e.ts) | Child receives SIGKILL at deterministic model/tool failpoints. Restart shows completed request prefix or \`ToolOutcomeUnknownError\`; uncertain effects not blindly repeated. | Borrow the **test methodology**, not code: crash/restore matrix for each Rust effect boundary. |
| [packages/extensions/cordis-host-runner/src/lifecycle.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/extensions/cordis-host-runner/src/lifecycle.ts) | Plugin activation creates a child fiber; failed initialization disposes it before error propagation; stopping unwinds registered effects. | Tool registration ownership and cleanup must be transactional. |
| [packages/extensions/cordis-host-runner/src/guard.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/extensions/cordis-host-runner/src/guard.ts) | Dynamic code sees a limited facade; schema and cross-realm payloads are validated before host registrations. | Dynamic extensions should not receive the whole \`AppState\`; avoid giving arbitrary code engine internals. |
| [packages/sdk/server/src/server.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/server/src/server.ts), [packages/sdk/protocol/src/transport.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/protocol/src/transport.ts), [packages/sdk/client/src/client.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/client/src/client.ts) | stdio newline JSON-RPC between separately owned SDK client and complete \`dsh\` runtime; typed notifications, session resumption, subprocess shutdown. | Existing DOMINION TypeScript SDK is HTTP/SSE client. Retain it; add a **Rust-native SDK crate** and optional typed stdio JSON-RPC transport for local integration. Do not embed the Node runtime. |
| [packages/interaction/user-approval/src/index.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/interaction/user-approval/src/index.ts) | \`ask\` and \`never\` approval mode are per-session durable controls and separate from the agent loop. | Preserve separation between **user task intent**, explicit configured tool rights, and runtime authorization. Do not import anyone else's refusal prompts as user-task policy. |

## Architecture decision: combine US + CN selectively

Cross-compare [OpenAI Agents run lifecycle](https://github.com/openai/openai-agents-python/blob/main/src/agents/lifecycle.py), [Claude SDK typed hooks](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/types.py), [Copilot SDK Rust hook contracts](https://github.com/github/copilot-sdk/blob/main/rust/src/hooks.rs), [Codex typed thread events](https://github.com/openai/codex/blob/main/sdk/typescript/src/thread.ts), [AgentScope middleware](https://github.com/agentscope-ai/agentscope/blob/main/src/agentscope/middleware/_base.py), [DeerFlow sandbox provider](https://github.com/bytedance/deer-flow/blob/main/backend/packages/harness/deerflow/sandbox/sandbox_provider.py), [Qwen-Agent function-call grammars](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/llm/fncall_prompts/qwen_fncall_prompt.py), and [OpenManus tool loop](https://github.com/FoundationAgents/OpenManus/blob/main/app/agent/toolcall.py).

**Decision:** DOMINION owns Rust loop, protocol, authority, tool execution, checkpoints, and evaluation. External libraries are references, not runtime dependencies. **Do not port Cordis wholesale**: replicating a large JavaScript plugin universe adds startup/disposal/compatibility costs and gives no automatic increase in coding completion. Build small typed Rust seams, composable without an external Node process. Preserve existing HTTP/SSE SDK and existing SQLite event/goal infrastructure.

## Proposed implementation order

### 0. Preserve authoritative intent (work already in progress)

Sources: \`src/engine/chat/owner_intent.rs\`, \`context.rs\`, \`runtime/turn.rs\`, \`room_context.rs\`. In the prompt context, identify real owner messages separately from peer/retrieval/runtime messages and restore the stored original owner request after truncation. Run Rust tests to verify no duplicate owner, no provider assistant/tool pair breakage, and room/regular-chat parity. It is not safe to claim validation solely from source edits.

### 1. Durable owner input queue: highest ROI

Proposed Rust seam:
\`\`\`rust
enum InputTarget { NextTurn, NextStep }
struct OwnerInput {
    id: String, source_message_id: String,
    session_id: String, revision: u64, target: InputTarget,
    content: String,
}
\`\`\`
- \`POST /api/sessions/:id/steer\` or a separately named typed inbox endpoint: persist one authenticated user message and its queue insertion in the same SQLite transaction, uniquely by message ID.
- At a \`StepBoundary\`, atomically claim queued \`NextStep\` corrections with execution and generation ID. Do not interrupt an already dispatched external side effect; apply the newest user revision before *next* model request.
- \`NextTurn\` starts a new root action; \`NextStep\` is a legitimate revision of an existing root and records a distinct user-origin event. Peer, tool, retrieval, and background reminders cannot insert into the owner queue.
- Continue reusing \`goal_steering.rs\` CAS revision semantics, \`events.rs\` replay, and \`state.rs\` cancellation epochs.
- Tests: conflicting simultaneous revisions; duplicate event; restart between accept and claim; new owner instruction during a long tool; peer faking an owner; cancelled run never revives; latest owner correction preserved after context compression.

### 2. Step boundary + hook seam

Proposed traits (design only):
\`\`\`rust
trait AgentDriver { /* run a turn and advance one step */ }
trait ToolRegistry { /* register and revoke one scoped tool */ }
trait ExecutionHook { /* before model / before tool / after tool / after step */ }
\`\`\`
- Hook inputs contain typed \`task_id\`, \`session_id\`, \`step_id\`, \`owner_revision\`, and cancellation generation.
- Register hook IDs and scope ownership; unloaded plugins revoke only their contributions. Compose static Rust plugins first; optional WASM/native extension has a distinct, explicit trust boundary.
- Model adapters can change, but cannot rewrite or impersonate authenticated owner input.

### 3. Checkpoint + sandbox lifecycle

- Establish \`checkpoint_before_model\` and \`checkpoint_before_effect\` invariants, tested with forced SIGKILL and durable outcomes. Existing Rust operation journal should remain the authority.
- Implement one \`SandboxProvider\` / \`SandboxLease\` contract over local workspace; add process-isolated provider only after proving file/write, credentials, child process cleanup, and retained artifact results.
- Sandboxes isolate execution but **must not import new behavioral refusal policies**. Tool rights and approval remain explicit owner/runtime settings.

### 4. Expose real Rust SDK & measurable task success

- New \`dominion-agent\` library exposes \`Session\`, \`Goal\`, \`AgentDriver\`, \`ToolRegistry\`, \`SandboxProvider\`, and a typed event stream, independent of Axum routing.
- TypeScript SDK remains the remote HTTP/SSE client. Optionally add stdio JSON-RPC adapter for CLI embedding and local app workflows.
- Eval: same coding tasks across Qwen/native, DeepSeek/native, compatibility-parser models; track correct tool execution, artifact pass, owner correction latency, token cost, interrupted-recovery duplicate effects, plugin unload residue, and final task completion.
- Compare with a frozen baseline before promoting any policy to the running engine; do not equate extra modules with a performance gain.

## Priority summary

1. Authenticated durable **NextStep owner steering**.
2. Step-level checkpoints + crash tests.
3. Scoped Rust tool/plugin registration with deterministic cleanup.
4. Sandbox leases and lifecycle.
5. Public Rust-native SDK API.
6. Additional transport clients.

The value in DeepSeek Harness is not its bundled model or JavaScript runtime; it is the separation of **durable facts, live decisions, reversible capabilities, and client interface**.
