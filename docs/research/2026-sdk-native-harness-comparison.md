# 2026 Agent SDK architecture comparison — Dominion native harness

Checked: 2026-11-14 (KST). Applies to `dominion-sub1`. This is a source-to-design comparison, not a declaration of production parity.

## Non-negotiable architecture

Dominion keeps its own Rust agent runtime, model/tool orchestration, intent interpretation, execution policy, state, worktree, evidence verification, cost ledger, and UI. SDK clients that wrap another vendor's runtime are **reference implementations**, not substitutes for Dominion's runtime. No Copilot, Codex or Claude executable dependency was introduced in this study.

An owner's explicit tool/goal request must not disappear during prompt-size optimization. Engine/tool availability, per-session authority, path scope, approval, capability registration, and resource limits remain separate from *model-facing tool selection*. The latter optimizes schema cost; it does not become a new refusal/authorization policy.

## Current sources and reusable mechanisms

| System | Verified architecture | Native Dominion implication |
| --- | --- | --- |
| GitHub Copilot SDK (GA 2026-06-02, Rust supported) | Uses the Copilot agent runtime for plan/execute/file operations; typed pre/post/failure hooks; per-session tool updates and search-based deferral | Learn event/receipt correlation, failure hooks, and on-demand schemas. Do **not** run Copilot as Dominion's hidden decision engine. |
| Codex SDK / App Server | Persistent thread IDs; resume/fork; per-turn interrupt and steering; sandbox modes; app-server events | Keep owner steering and checkpoint identity explicit; do not substitute Codex execution policy or CLI. |
| Claude Agent SDK | CLI-backed agent runtime with tools, session resume/fork, hooks, permissions and MCP; tool calls use typed results | Use permission/result event separation and continuation/recovery patterns as design references only. |
| OpenAI Agents SDK | Trace/span model for LLM/tool/handoff/guardrail activity | Correlate native execution IDs, tool receipts, and subagent IDs; measure failures rather than equating model claims with success. |
| ACP Rust SDK | Agent/client/proxy/conductor roles with stable v1 protocol and tool/permission/stream events | A future optional client protocol adapter can sit outside Dominion's Rust decision engine. |
| Microsoft Agent Framework | Graph orchestration, handoff, checkpoints, human-in-the-loop, provider-neutral workflows | Compare with native durable program nodes, checkpoint schema, and parallel execution. |
| Rig (Rust) | Plugin-based coding harness; recorded effects; resume after partial tool calls; contextual tool catalog | Compare fail-safe effect journal and crash recovery with Dominion effect receipts. |
| OpenCode SDK | Typed client for a separate server; sessions/events/structured output | API design reference, not an agent-runtime drop-in. |

## Changes implemented in sub1

1. `src/engine/chat/runtime/tool_selection.rs`: the small schema window for code-writing goals retains model tools **already permitted by the upstream tool policy** when owner input explicitly calls out tool names. For mixed tasks, it also retains an available web/browser tool suite and requested command execution. This prevents a native optimization from turning `fix code and verify in browser` into `code-only`. If the permission gate never advertised a tool, this function cannot invent or authorize it.
2. The same file's native regression test covers mixed code + web search + browser + shell + named MCP. A simpler code edit still receives a small menu.
3. `src/engine/goals/tests.rs`: the read-only evidence regression expects the already-shipped deterministic no-progress review skip (3 mock model requests, not 4). It also checks that the goal remains `needs_review` and that the durable review history records why the model reviewer was skipped.

The work here does **not** add a prompt filter, refusal classifier, or remote SDK policy. Existing execution and path/approval controls remain unchanged.

## Boundaries and validation

- The source-level proof is the native Rust tool-window unit test and goal-harness integration test. Test execution and CI are reported separately from code changes.
- This patch does not establish Codex/Claude Code parity, external browser reliability, or real-model task completion.
- For larger MCP catalogs, a dynamic capability-search/late schema-load interface may be valuable. It should preserve explicit owner requirements and audit selection decisions, while avoiding unnecessary full schemas.
- Validate later with a two-part user goal: edit a repository file **and** run a website/browser check; compare advertised tools, receipts, actual outcome, and charged tokens.

## Sources (visited 2026-11-14)

- GitHub Copilot SDK GA: https://github.blog/changelog/2026-06-02-copilot-sdk-is-now-generally-available/
- GitHub Copilot Rust SDK: https://github.com/github/copilot-sdk/blob/main/rust/README.md
- Copilot tool search source/changelog: https://github.com/github/copilot-sdk/blob/main/CHANGELOG.md
- Codex SDK: https://learn.chatgpt.com/docs/codex-sdk
- Codex Python events/steering: https://github.com/openai/codex/blob/main/sdk/python/docs/api-reference.md
- Claude Agent SDK: https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/client.py
- OpenAI Agents tracing: https://openai.github.io/openai-agents-python/tracing/
- ACP official Rust SDK: https://github.com/agentclientprotocol/rust-sdk
- Microsoft Agent Framework: https://github.com/microsoft/agent-framework
- Rig/Rust: https://github.com/0xPlaygrounds/rig
- OpenCode SDK changelog: https://opencode.ai/changelog?area=sdk
