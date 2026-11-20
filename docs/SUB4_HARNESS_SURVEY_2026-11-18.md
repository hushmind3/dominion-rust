# Sub4 — 2026 harness source/research matrix and adoption gates
> Branch: `dominion-sub4` · 2026-11-18 · Scope: own Rust kernel only; neither main nor competing branches are modified.

## Why broad source comparison matters

Two separate engineering problems must not be conflated:
1. **Make Dominion's own single native execution loop better**: reduce tool/model calls, maintain high completion quality, preserve permission and durable recovery.
2. **Host independent agent runtimes**: OpenClaw/ACPX, OpenCode, Gemini CLI, Codex and other adapters are externally delegated workers with distinct session/approval/effect ownership. Adding more runtimes is not evidence that Dominion's native algorithm has improved.

Research calibration:
- [An Empirical Study of Harness Design for Coding Agents](https://arxiv.org/abs/2609.20804) (Sep 2026): four models, 176 settings, SWE-bench Verified + Terminal-Bench 2.1. Context control principally prevents context-overflow failure, explicit planning can lower cost for stronger models, and structured tools favor models less proficient at bash. **Do not generalize a coding-only experiment into proof for all Dominion money/research tasks.**
- [Harness Engineering: Anatomy, Architecture, and Evolution of Coding Agents](https://arxiv.org/abs/2609.00006) (2026): 11 coding systems and 29 observed design patterns. Catalog is comparative architecture evidence, not a measured winner ranking.
- [Beyond the Model: Demystifying Harness Effects in Software Engineering Agents](https://arxiv.org/abs/2609.32459) (Oct 2026): tool registry/subagent/compaction component effects are task dependent. Keep comparison baselines reproducible.

## What transfers without importing another execution loop

| Reference (primary source) | Useful mechanism | Existing Sub4 ownership | Decision |
| --- | --- | --- | --- |
| [Pi](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent) | Minimal tool-centric agent core and extensible UI/session shell | `engine.rs` is the one authoritative loop; `sdk.rs` owns public entrypoints | Borrow simplicity, not Pi's session runtime |
| [mini-SWE-agent](https://github.com/SWE-agent/mini-swe-agent) | Simple, mostly linear model→command→observation cycle | Native `tools.rs` + `journal.rs` already execute ordinary calls | Make `read_program`/`run_program` optional, keep approved shell; benchmark simple baseline |
| [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/architecture.md) | Dynamic services, plugin mounting and reversible registration | `frontend_extensions.rs` manages MCP and Skills | Improve extension lifecycle later; do not copy Cordis/plugin runtime |
| [Goose](https://goose-docs.ai/docs/mcp/extension-manager-mcp/) | Enable/disable MCP capabilities at runtime | Live `mcp.rs` registry, operator-managed extensions | Progressively disclose tools, keep single remote invocation path |
| [OpenCode](https://github.com/anomalyco/opencode) | Server/session/SDK boundary | Rust SDK + TypeScript client + durable Run Store | Evaluate API semantics, not transplant server/session journal |
| [OpenHands SDK](https://github.com/OpenHands/software-agent-sdk) | Workspace/tool/event executor contracts | Store emits Run updates, SDK owns artifacts | Future local/remote executor adapter; no duplicate scheduler |
| [SWE-ReX](https://github.com/SWE-agent/SWE-ReX) | Stable executor API across local/Docker/cloud | Native workspace and approved shell | Future isolated execution transport, preserve receipt reconciliation |
| [OpenClaw ACPX](https://github.com/openclaw/openclaw/tree/main/extensions/acpx) | Independent external coding harness hosted by parent session | `spawn_subagent` is currently native-only | Future optional ACP runtime adapter; never merge foreign loop |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md) | BeforeToolSelection / BeforeTool deterministic hooks | `hooks.rs` pre-dispatch and post-receipt | Extend catalog hooks only with explicit operator controls |
| [Deep Agents](https://github.com/langchain-ai/deepagents) | Optional middleware, skill/backend composition | Lazy Skills, memory, tool schemas | Borrow modularity without depending on LangGraph |
| [Letta](https://github.com/letta-ai/letta-code) | Durable, scoped memory that can be reloaded as relevant context | `knowledge.rs` scoped notes + owner-controlled promotion | Improve measured recall, avoid unconditional memory injection |

## Changes implemented in this branch

- Introduced independent **pure function** `src/tool_discovery.rs` based on the principle of progressive disclosure, not by importing any foreign runtime package or session format.
- Over 8 operator-authorized external tools, announce `tool_search` and pre-disclose up to **3** directly relevant original schemas so an obvious action can start without a paid discovery turn.
- Queries perform bounded local ranking over the live eligible catalog; other eligible tools are discoverable on demand. Small catalogs remain eager.
- Successful search receipts update a **bounded 16-name, durable, backward-compatible per-Run discovery view**; a future model turn receives each originally registered tool schema.
- Actual tool calls still enter the original Run: schema admission → operator policy + hooks → approval if required → intent fsync → execution → durable receipt / unknown-effect reconciliation.
- No external-agent runner, direct bypass dispatcher, separate session/approval state machine, model-name-only capability guess, unrequested background experimentation, or forced planner was added.
- Simplified the engine's model instruction so `read_program` and `run_program` are **optional** native execution accelerators, not mandatory workflow routing.

## Controlled comparison gate before further tuning

Keep the original objective, code fixtures, model sampling and cost budget constant. Test these alternative tool surfaces:
1. **eager**: full native workspace tools + eligible external tools
2. **hybrid**: native workspace tools + 3 intent matches + `tool_search`
3. **pure-lazy**: native workspace tools + `tool_search`, no auto-prefetch
4. **shell-oriented**: ONLY in an isolated, user-approved coding harness profile, and only for models demonstrated capable of using shell. Shell approval, sandbox and crash reconciliation MUST remain intact.

Run on at least:
- local ~4B structured-tool-sensitive model
- local ~27B model
- frontier coding/reasoning model
- coding repair and multi-file search, evidence-based web/MCP research, and mixed external-account task categories

Record: verified task completion, model/tool rounds, tool-schema input bytes and actual usage tokens, real KRW cost, p50/p95 latency, schema-admission errors, first relevant tool precision, owner-approval friction, cancellation/restart outcome, unconfirmed remote-effects, and failure modes. **An untested win is not a benchmark result.**

## Convergence contract

Do not cherry-pick `src/engine/chat/runtime.rs` from main/sub2/sub3 into Sub4. Candidate pure algorithms can be tested as small isolated modules, then selected by measured completion/cost. Existing Run journal is the single authority for retries, approvals, cancellation, child budgets, owner directives, and external-effect reconciliation.

The current MCP client only supports validated Streamable HTTP transports, not arbitrary local stdio installers. External ACP and SWE-ReX execution require separate future adapters, cancellable sessions, receipt correlations, and explicit integration tests.

Release CI, Windows packaging, live paid model benchmarks, and ACP hosting remain unclaimed.
