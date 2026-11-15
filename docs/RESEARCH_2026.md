# 2026 reference inspection and design choices

Date of review: 2026-11-15. These are source-informed design references, **not claims that Dominion equals their measured performance**.

## Architecture

- [OpenAI — Unlocking the Codex harness, February 4, 2026](https://openai.com/index/unlocking-the-codex-harness/): the same core powers multiple surfaces through a persistent application server, with thread/turn/item lifecycle, streaming notifications, reconnection and approval requests. **Applied:** Rust owns work state, approval, snapshots and events; the Studio and SDK use the same HTTP API.
- [OpenAI — Codex as a platform, August 19, 2026](https://developers.openai.com/blog/codex-as-a-platform): separating harness, execution environment and product-facing interface. **Applied:** separate model transport, tool execution and product/API layers, rather than importing an external agent engine.
- [Anthropic — Steering Claude Code, June 18, 2026](https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more): persistent instruction sources, skill/context loading and subagent distinctions. **Applied:** explicit user directives and workspace isolation; **pending:** skills and hooks.
- [DeepSeek Harness 2026](https://www.deepseek.com/harness/en/) and [workflow implementation](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/workflow/workflow/README.md): composable plugin-owned tools/workflows and bounded delegated execution. **Applied:** provider and tool interfaces are separate from the Rust execution loop. **Pending:** general MCP/plugin transports and multi-agent fanout.
- [Qwen-Agent](https://github.com/QwenLM/Qwen-Agent): tool calling, MCP, code interpretation and model-native compatibility as flexible components. **Applied:** compatible chat completions with native function tool-calls, independent of a particular model brand.

## Reflection and measurement

- [GEPA, ICLR 2026](https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html): reflection on trajectories and measured feedback to revise natural-language prompts/strategies. **Applied:** persisted objective/evaluation evidence and descendant textual reflection in `src/improvement.rs`. **Not implemented:** Pareto portfolio selection or population search.
- [Codex multi-agent source guidance](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents_spec.rs): delegate concrete, non-overlapping subtasks and avoid duplicate effort. **Pending:** true simultaneous agents with disjoint write scopes; the current group mode is sequential reasoning with role routing.

## Evidence boundaries

The listed projects are comparative research inputs. Dominion's code is a clean independent implementation. References support design choices; feature readiness is determined by executable code paths and tests. The current MP4 renderer makes styled scene slides with optional local speech, not generative motion footage, and should not be marketed as a premium cinematic generator.

## 2026 reflection and memory source comparison (2026-11-15)

- GEPA, *Reflective Prompt Evolution Can Outperform Reinforcement Learning*, ICLR 2026: https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html — extracts reusable textual rules from measured traces and evaluates revisions rather than treating self-assessment as ground truth.
- Qwen Code multi-agent coordination: https://github.com/QwenLM/qwen-code-docs/blob/main/website/content/en/users/features/multi-agent-coordination.md — separate investigator workers, ordered task state and writer-owned worktree.
- Qwen Code memory: https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/memory.md — persistent written instructions and agent memory across sessions, with optional shared memory.
- Anthropic Claude platform memory tool: https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool — memory I/O is controlled by the host application instead of the LLM platform, with cross-session retrieval.

Dominion's implementation decision: keep independent Rust evaluation/adoption authority and reuse the existing durable Run and Improvement records as the source of truth. A separate memory record is not itself proof of success; only independently checked task receipts and adopted Git candidates feed the ranked case index. Unlike full GEPA, the current implementation does not evolve a Pareto population of prompt policies or conduct same-task comparative optimization. No external SDK has been adopted.
