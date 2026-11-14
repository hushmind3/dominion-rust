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


## 2026-11-15 source-level update — US/China SDKs and recursive experience

| Code/research (2026 checked) | Source algorithm | Adaptation in Dominion |
| --- | --- | --- |
| OpenAI Codex (US), codex-rs/core/src/session/turn.rs and state/turn.rs | Rust turn state, tool router and asynchronous tool loop; compaction, interrupt and multi-agent delivery | Native Rust ownership stays independent; no Codex runtime dependency |
| Qwen-Agent (China), qwen_agent/agent.py | Extensible Agent._run + independently configured tool registry and MCP | Dominion keeps provider-neutral Rust chat/tool selection and existing MCP dispatch |
| AgentScope (China), ReAct SDK and toolkit | Interrupt/resume, batched tools, context compaction, permissions and events | Keep durable Rust receipts with a separate model-facing schema window and tool authorization |
| SelfSearch (2026), arxiv 2609.37968 | Prior self-modification episodes guide successor redesign while fixed model weights remain unchanged | Implemented source-matched failed/accepted historical episodes in compiled meta-controller |
| GEPA-Curator (2026), arxiv 2610.37651 | Filters unreliable failure trajectories; retrieves relevant successes | Current learner requires original commit, role, accepted SHA receipts and frozen paired verification; other noise filters remain a research hypothesis |

### Actual integrated call chain

src/engine/rsi_meta_controller.rs → meta_controller/trajectory.rs::source_matched_alternatives →
meta_controller/selection.rs::synthesize → meta_controller/feedback.rs::instruction →
existing descendant worktree tool worker → original frozen paired evaluator →
adoption and compiled successor. The model observes up to 3 source-matched
verified alternatives. These are NOT evidence of causal improvement by themselves.
The model-controlled source change is distinct from the frozen independent reviewer.

src/engine/rsi/system/inventory.rs now recognizes trajectory.rs as an editable
improvement method; meta_controller/provenance.rs recognizes verified future
changes to it as meta-source improvement; metaMechanismModules includes it.

sdk/rust and sdk/typescript now wrap existing /api/rsi/start, /api/rsi/campaigns,
and /api/rsi/campaigns/:id/apply endpoints with exactly the actual Rust limits
maxAttempts, tokenBudget, timeBudgetSeconds. SDKs never delegate decisions to
an external agent. A server-side change and accepted commit must still pass
the existing frozen evaluator; no benchmark success is inferred from a
network test.

### Verification boundaries

New unit/integration tests are in the Rust trajectory module,
sdk/rust/tests/sdk.rs, and sdk/typescript/tests/sdk.test.mjs.
They verify source-matched evidence and HTTP contracts. A source commit
is not a completed local cargo build, paid model trial, or SWE-bench score.
Full benchmark evaluation follows the existing scripts/swebench-dominion.mjs
runner and requires a configured model and an execution environment.

Sources consulted 2026-11-15:

- https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs
- https://github.com/openai/codex/blob/main/codex-rs/core/src/state/turn.rs
- https://github.com/openai/codex/blob/main/codex-rs/tools/README.md
- https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/agent.py
- https://github.com/agentscope-ai/agentscope
- https://arxiv.org/abs/2609.37968
- https://arxiv.org/abs/2610.37651
- https://arxiv.org/abs/2610.08902
