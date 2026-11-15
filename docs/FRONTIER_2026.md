# Dominion vs 2026 frontier agent harnesses — implementation map

Review date: 2026-11-15. This is a **source-path capability comparison**,
not a leaderboard score or a claim of equal performance. Each reference is
a production project or an official architecture/code source inspected
for an execution mechanism, not a list of marketing feature names.

| Reference | Inspected mechanism | Frontier target | Dominion current path | Remaining gap |
| --- | --- | --- | --- | --- |
| OpenAI Codex Rust | codex-rs/core/src/compact_token_budget.rs, apply_patch.rs, session and approval layers | Persistent threads, context-lifecycle events, structured source edits, configured permissions | src/context.rs, src/engine.rs, src/tools.rs | No provider-aware compaction quality metrics, real sandbox or mature thread lifecycle |
| Claude Code (Anthropic, 2026) | Skills, hooks, CLAUDE.md, context-isolated subagents | Deterministic hooks and selective instruction loading; isolated delegated work | src/engine.rs, src/sdk.rs (isolated child runs, user directives), src/workspace.rs | No complete skills/hooks/procedure catalog |
| DeepSeek Harness | packages/core/agent-loop and session event store; plugin capability seams | Swappable decision and execution driver, complete replay and session events | src/engine.rs + store.rs + mcp.rs, write-ahead tool intent | Replace fixed loop with strategy/policy traits; richer recovery and event log |
| Qwen Code Agent Teams | Concurrent read-only investigators, owner-controlled writer worktree, message/task board | Separated investigation/writing and conflict-free integration | src/engine.rs child runs and budget reservation; src/workspace.rs reproducible detached checkouts | No task board, read-only worker permission mode or controlled patch merge |
| OpenClaw | src/agents/agent-command.ts and agent-bundle-mcp-* runtime | Multi-channel integration, resilient MCP registration, session admission | src/mcp.rs HTTP tools with approval; src/store.rs durable run files | Broader channel adapters, stdio MCP, admission/connection lifecycle |
| NousResearch Hermes | agent runtime, persisted state and skills ecosystem | Lifelong memory, skills, autonomy across sessions | Run history, non-weight improvement reflections | Persistent retrieval memory/skills with measured transfer absent |

Source links:

- https://github.com/openai/codex/tree/main/codex-rs/core/src
- https://developers.openai.com/blog/codex-as-a-platform
- https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md
- https://qwenlm.github.io/qwen-code-docs/en/users/features/multi-agent-coordination/
- https://github.com/QwenLM/qwen-code/tree/main/packages/core/src
- https://github.com/openclaw/openclaw/tree/main/src/agents
- https://github.com/NousResearch/hermes-agent

## Required parity gates (NOT yet passed)

1. Large-repository real-model task completes: inspect, edit across files,
   run actual tests with approval, repair failed tests, independently verify,
   and produce a reviewable Git diff and artifact.
2. User's late instruction wins while a model request or verifier is in
   flight. No stale result may enter the accepted plan or final answer.
3. Context under stress: user constraints/tool receipts retained, token usage
   and actual provider cache billing measured, quality and correctness do not
   drop relative to the full-history baseline.
4. Delegated workers cooperate via bounded budgets, isolated worktrees,
   explicit results and a conflict-checked integration strategy.
5. Permission policy, OS-level sandbox and resource isolation are measurable.
6. Sessions, skills, memory, channels and MCP live within a coherent Rust
   lifecycle; UI, Rust SDK and TypeScript API agree on current state.
7. Group conversation, video backends, RSI worktree evaluation and exact
   evidence all work end to end without feature loss or stale mock paths.
8. After the above, run matched-condition coding evaluations (including
   SWE-bench Verified) with identical model, token budget, tools, timeouts
   and judge to compare success, tool calls, cost and recovery.

## This iteration: code rather than a checklist

- Native SDK in src/lib.rs and src/sdk.rs, executable bound through the SDK.
- Source Git checkout attachment from SDK/API/GUI via src/workspace.rs.
- Exact-match code editing, Git status/diff in src/tools.rs.
- Non-destructive context projection preserving directives/tool pairs in
  src/context.rs, called by the real model loop.
- Stale model-plan/response/verifier rejection when user intervention races.
- Subagent tool execution with lineage, sibling budget reservation, Git
  revision inheritance, and parent-inspected child artifacts.
- Provider-reported cache token cost split, actual KRW rate configuration,
  saved runtime counters surfaced to the UI.

No new goals.rs or goals.api file is introduced.

## Evidence levels

- **Source-connected:** features listed above have linked Rust/API/SDK/UI
  entrypoints, with selected new unit tests.
- **Not yet executed this revision:** full Rust compilation, full UI browser
  simulation and real-provider tasks (CI deferred by project instruction).
- **Not yet achieved:** frontier-level measured task performance, complete
  sandbox/skills/memory/channel ecosystem, mature long-run task repair,
  multi-worktree merge, premium turnkey video quality.

Avoid calling a feature production-ready based on the presence of one
route, a named module or a mock model test.
