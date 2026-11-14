# DOMINION frontier-scale integrated system engineering (Sub2, 2026-11-14)

## What the product actually is

An owner-directed general-purpose **Rust agent operating system**, not an SDK
demo. The 2026 comparison target is a full user task cycle, including native
tool execution, goal/worktree autonomy, multi-agent tasks, durable state,
self-improvement without weight training, inexpensive cinematic video, and
operable web UX. Independent evaluations must score end-to-end observed
results, not sheer number of source files, tool definitions or UI pages.

Audit baseline: \`dominion-sub2\` has 228 \`src/engine\` tree entries
(~4.1 MB including tests), 38 Rust API modules, 50 web/src entries and 50
docs entries. These sizes describe coupling risk, not product intelligence.

## Single runtime/data/owner contract: actual entry-to-result graph

\`\`\`text
OWNER (web / Rust SDK / API / rooms / channels)
  | exact original message + attachments + explicit task revision
  +-- direct chat: POST /api/chat -> src/api/chat.rs
  |    -> src/sdk.rs -> src/engine/chat/runtime/turn.rs
  |    -> provider + tool catalog -> dispatch -> execution_operation evidence
  |    -> native verifier -> persisted result/usage/event
  +-- autonomous goal: POST /api/goals -> src/api/goals.rs
  |    -> durable job -> src/engine/goals.rs
  |    -> planner/steps -> chat runtime + tools
  |    -> isolated worktree/diff + acceptance checks + goal receipt
  +-- workflow/rooms: jobs.rs -> workflows.rs / rooms.rs
  |    -> agent-specific turns/shared provenance -> evidence/knowledge/usage
  +-- video: jobs.rs -> video.rs -> cinematic.rs / film.rs / ltx.rs
  |    -> frozen paid inputs -> per-shot receipt + MP4 technical quality
  +-- RSI: rsi_evolve.rs -> campaign / worker / evaluation
  |    -> model-generated source edit -> separate worktree
  |    -> frozen paired judge -> accepted code/strategy -> descendant
  |    -> feedback into strategy, retrieval, method selection (no LoRA)
  +-- owner correction: POST /api/sessions/:id/steer -> SQLite queue
  |    -> next real model step as a verbatim user-role message
  +-- owner cancellation: API / SDK -> cancellation epoch
       -> job status CAS prevents late result from overriding cancellation

CROSS-CUTTING:
  AppState + SQLite Store + Vault + event bus
  -> provider select/fallback/cache-aware context
  -> effect preflight / tool approval / tool evidence / deterministic verifier
  -> progress + budget + authenticated session lineage
  -> web screens: Chat, Operations, Rooms, Video, RSI, Models, Memory, etc.
\`\`\`

### System invariants (owner control ≠ copied provider policy)

1. **Only owner-origin messages own task direction.** User's literal input is
   stored; model-generated plan text, retrieval, tool output, web content, and
   peer-agent text can inform the answer but cannot overwrite owner intent.
   The latest authenticated owner correction supersedes stale generated plans.
   The same rule applies across native SDK, REST adapter, rooms and goals.
2. **A result is not a receipt.** A model's "done" cannot certify a source
   change; verified file changes, test status, tool effects, media metadata
   and paid-media receipts are independently observed.
3. **One cancellation transition, all domains.** Every job family (prompt,
   workflow, room, goal, video, RSI, learning, upload) obeys the same atomic
   job finalization CAS. Owner cancellation wins against a late worker result.
4. **Crash recovery preserves side-effect uncertainty.** A remote paid action
   or API mutation must be reconciled rather than blindly retried.
5. **Usage and success are recorded together.** A low-token turn without
   verified completion is not a cost optimization. Cache hits, failed runs,
   compaction overhead and provider fees belong to one completion-weighted
   measurement.
6. **Native Rust is the execution authority.** Optional TS/JS SDK is transport;
   Rust tools execute inside the same Rust dispatch/evidence/verification path.
   No external SDK is linked as a policy, planner, refusal or tool engine.
7. **No model weight learning.** All recursive improvement operates on
   source code, configurations, principles, memories and executable algorithms;
   the separately frozen evaluator does not become a candidate edit target.

## Whole-product gap map and engineering sequence

| Product | Existing physical entrypoints | Remaining frontier-grade gap | Required proof |
|---|---|---|---|
| Autonomy | \`goals.rs\`, \`runtime/turn.rs\`, \`verification.rs\` | Closed-loop real model multi-file coding with uninterrupted owner steering; plan rebase after late correction | A/B repository tasks: real file diffs, compile pass, tool receipts, owner correction applied without stale work |
| Algorithms | \`algorithms.rs\`, \`algorithm_eval.rs\`, \`adaptation_loop.rs\` | Measured strategy choice across previously unseen code and non-code tasks; avoid proliferation of unproven algorithms | Held-out functional outcomes and control/baseline comparisons |
| Durable scheduling | \`jobs.rs\`, \`db.rs\`, \`steering.rs\`, \`wake.rs\` | Unified terminal state under cancellation, retry and concurrent workers | Crash fault injection and cross-domain terminal CAS tests |
| Context | \`chat/context.rs\`, \`chat/provider.rs\`, \`compact.rs\` | Exact owner-intent fidelity through compression, group speaker transitions, and long-run retrieval | Token/cost accounting and recent owner correction survives compression |
| Tools/MCP | \`tools.rs\`, \`custom_tools.rs\`, \`mcp.rs\`, \`browser.rs\` | Native custom tool execution with independent effect verification, timeouts and sandbox contracts | Real SDK tool -> tool operation -> verified output or explicit unknown |
| Collaboration | \`rooms.rs\`, \`room_knowledge.rs\`, \`tools.rs\` | True parallel multi-agent worktree handoff with deterministic conflict resolution and cost-aware speaker selection | 2+ agents, non-overwriting diffs, join receipt, measured output cost |
| RSI | \`rsi_evolve.rs\`, \`rsi/worker.rs\`, \`rsi/evaluation.rs\` | Repeated independent real gains and cross-domain inherited method improvements | Multiple held-out source revisions with frozen paired evaluator, actual descendant reuse |
| Video | \`video.rs\`, \`video/film.rs\`, \`video/cinematic.rs\`, \`video/quality.rs\` | Semantic scene/voice/character coherence verified at full delivered-clip level, not just playable MP4 | Actual MP4 samples, shot-level hash receipts, human/blind continuity rating and paid invoice |
| Token cost | \`router.rs\`, \`chat/provider.rs\`, \`rooms/usage_ledger.rs\` | Charge-weighted per-completed-goal routing including caching/retry/multi-agent overhead | Actual billed input/cache/output tokens and walltime per verified completed task |
| Product UX | \`web/src/pages/*\`, \`web/src/components/ExecutionMonitor.tsx\` | User sees why a task paused, which action is awaited, verified edits, token spend and deliverables on one screen | Real E2E UI journeys rather than route-only smoke tests |
| Commercial | \`bootstrap.rs\`, \`vault.rs\`, \`db.rs\`, \`events.rs\` | Single-user desktop vs multi-tenant cost/key isolation, upgrade/migration and resource caps | Restore/migration/drill and no cross-tenant data access |

Priority ordering: **system contracts first** (owner intent + cancellation + evidence) ->
end-to-end agent coding proof -> video end-to-end proof -> RSI held-out
adoption -> cost-weighted optimization -> UI usability and multi-agent
integration. The ordering is dependency-driven, not a declaration that only
these domains matter.

## 2026 US/China source comparison — systems, not SDK names

- OpenAI Codex (US): \`codex-rs/core/src/tools/registry.rs\`, \`exec.rs\`,
  agent mailbox, owner instructions — typed Rust tool runtime, command process
  termination, auditable tool lifecycle. Adopt native execution semantics, not
  its developer/consumer policy bundle.
- OpenAI Agents (US): \`src/agents/run_internal/run_loop.py\`,
  \`tool_execution.py\`, \`lifecycle.py\` — explicit run lifecycle, streaming,
  tool identity, multi-agent handoff. Adopt instrumentation and deterministic
  failure categories, not their Python runtime.
- Microsoft Agent Framework (US): \`python/packages/core/agent_framework/_middleware.py\`
  — distinguish recoverable tool errors from control-flow failures. Model this
  in the native execution ledger.
- DeepSeek Harness (China): \`packages/core/agent-loop/src/{agent,inbox,runtime-context,assistant-stream}.ts\`
  plus \`compaction/tool-pairing.ts\` — event-derived agent state, explicit
  owner next-step steering, stable prompt projection, complete tool call/result
  pairs. Implement independently in Rust/SQLite.
- AgentScope (China): middleware lifecycle/budget boundaries. Implement
  callback hooks with causal scope rather than a global mutable singleton.
- DeerFlow (China): sandbox-scoped subagents, staged outputs, context isolation.
  Apply to Rust worktrees and isolated delegated tasks.
- Qwen-Agent (China): local-model textual function-call compatibility and
  tool schemas. Reuse DOMINION's native protocol parser with regression tests.
- OpenManus (China): small function/tool collection + feedback loop. Adopt
  discoverability while retaining evidence semantics.

## Peer branch transfer candidates (compare against main, 2026-11-14)

- \`dominion-sub1\`: separate \`sdk/rust/\` crate, measured goal verification
  scripts, native source hash readback. Assess for public SDK packaging and
  independently verified coding benchmarks.
- \`dominion-sub3\`: \`goal_checks.rs\`, rich \`agent SDK\` docs, measured
  evolutionary method tracking. Assess provenance-aware goal verifier.
- \`dominion-sub4\`: \`chat/owner_intent.rs\`, \`chat/qwen_compat.rs\`,
  frontier video cost and closed-loop RSI docs. Assess operator-intent
  consistency and local-model tool protocol.
- \`dominion-champion\`: \`agent_sdk.rs\`, \`agent_parallel.rs\`,
  \`goal_receipts.rs\`, \`tool_loop.rs\`; evaluate integrated diff/handoff
  implementations and merge proven units, not unreviewed entire branches.

## Implementation changes in Sub2

- Native \`src/sdk.rs\` and \`src/engine/custom_tools.rs\`: direct Rust
  model/tool execution.
- \`src/engine/steering.rs\`: durable correction enqueued by owner and read
  before the next actual model request (no extra planner policy).
- \`src/engine/chat/context.rs\`: explicitly marks plans, summaries and
  external data as lower-priority than direct owner task/correction.
- \`src/engine/jobs.rs\`: atomic cross-domain \`JobContext::settle\`; a
  cancelled job cannot be overwritten by late "completed", "failed" or
  "queued", and an optimistic result is never published as final before a
  durable SQLite transition.
- \`src/sdk.rs\` real mock provider test: first model emits tool call, owner
  steer is queued before tool completion, second native model request is
  asserted to contain the literal correction.

CI status and real-provider tests must be separately checked; this map
does not claim a paid-model autonomous coding benchmark was completed.

## References

- https://github.com/openai/codex
- https://github.com/openai/openai-agents-python
- https://github.com/microsoft/agent-framework
- https://github.com/deepseek-ai/deepseek-harness
- https://github.com/agentscope-ai/agentscope
- https://github.com/bytedance/deer-flow
- https://github.com/QwenLM/Qwen-Agent
- https://github.com/FoundationAgents/OpenManus

### Owner control is product-visible, not only backend code

`web/src/pages/Chat.tsx` now allows the submit button to send a literal correction while the current session is streaming. While idle it sends a normal chat input; while busy it calls `/api/sessions/:id/steer` and awaits the queued receipt. The next Rust model step emits `owner_steer` so the UI can display that the correction was actually incorporated. The JS transport offers `sessions.steer` for other products while the real engine remains Rust. No second chat stream is started when a correction is submitted.
