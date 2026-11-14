# DOMINION Sub2 · Native Agent Algorithm Research & Implementation (2026-11-14)

**Objective:** Examine available 2026 agent SDK *source code*, learn execution patterns, and implement independent algorithms in the **existing Rust DOMINION harness**. No third-party agent SDK dependency, copied execution engine, extra Goal menu, weight learning, or bypass of the existing approval/receipts/RSI evaluator.

## Repositories and examined source, not packaging/imports

| Source inspected | Actual mechanism | Implementation decision for DOMINION |
| --- | --- | --- |
| [Rig Rust `crates/rig-agent/src/agent/engine.rs`](https://github.com/0xPlaygrounds/rig/blob/main/crates/rig-agent/src/agent/engine.rs), [`corpus/base/src/loop_breaker.rs`](https://github.com/0xPlaygrounds/rig/blob/main/corpus/base/src/loop_breaker.rs) | Distinct model/tool/committed-history stages; repeated-call Gate counts normalized tool+JSON arguments and refuses third identical call; stops run on fifth | New `src/engine/chat/runtime/loop_guard.rs` uses **observed successful result digests** and environment version instead of name/arguments alone; only local snapshot reads are suppressed, not live status/network/write actions |
| [OpenCode `packages/core/src/session/compaction.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/core/src/session/compaction.ts) | Retains recent exchanges and objective/requirements; trims completed old exchanges atomically; accounts for cost and context-overflow retry with shrinking budgets | Native `provider.rs::fit_provider_context` clips old tool output and omits old *complete* protocol frames if still over budget, protecting current user request, two recent tool batches and all system instructions. Retry genuine pre-output overflow **once at 70% projection**; no duplicate spend through speculative repeated attempts |
| [GitHub Copilot SDK `rust/src/session.rs`](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs) and [agent loop architecture](https://github.com/github/copilot-sdk/blob/main/docs/features/agent-loop.md) | SDK dispatches events; CLI runtime owns agent loop; tool effects have idempotence/replay semantics, turns have distinct completion/abort states | Existing Rust DOMINION agent loop, permissions, SQLite `execution_operation` and verified acceptance remain authoritative; projections occur **before** continuity state snapshot. No Copilot runtime/dependency |
| [OpenAI Agents SDK Python `src/agents/run.py`](https://github.com/openai/openai-agents-python/blob/main/src/agents/run.py) and [running agent sessions](https://github.com/openai/openai-agents-python/blob/main/docs/running_agents.md) | Bounded turns, handoff, configurable concurrency, explicit tool error and stop behavior | Preserve existing max round/tool/time budgets, separate successful tool evidence from textual final answer, keep proxy-specific formats |
| [Claude context engineering guide](https://platform.claude.com/cookbook/tool-use-context-engineering-context-engineering-tools) | Tool-result clearing first, summary compaction second, preserve original memory/artifacts when cutting model-visible history | Implement deterministic result clearing/atomic frame removal with **no extra model summary charge** and leave full retained SQLite history to future larger-context routes |

## Actual runtime code paths

```text
Chat.tsx /api/chat, Room, Workflow, /goal slash (only chat)
      ↓
src/engine/chat/runtime/turn.rs::run_turn_body
      ↓ frozen system/context/history/available tools + run budget
src/engine/chat/provider/openai.rs::stream_completion
      ↓ per-candidate ProviderRef + guarded error retry
      ├─ provider.rs::fit_provider_context
      │   ├─ provider-specific tool observation clipping (newest first)
      │   ├─ only if still over budget, omit *complete* old exchanges
      │   └─ no edits to SQLite messages or execution_operation
      ├─ continuity::prepare/project of exactly the bounded view
      └─ OpenAI-compatible / Anthropic / Codex / local CLI transport
      ↓
model tool_calls → turn.rs native dispatch
      ├─ tool registry and agent/room capability attenuation
      ├─ prior error fingerprint, permissions, replay guard
      ├─ loop_guard::ReadLoopGuard::decision
      │   ├─ first two real identical local read results allowed
      │   ├─ 3rd matching read refused with truthful tool error result
      │   ├─ 5th exact repeat exits, preventing infinite inference
      │   ├─ any observed file bytes/mtime/size or engine mutation changes reopen reads
      │   └─ no guard for shell, writes, web, MCP, task_status or RSI study turns
      ├─ dispatch_tool_call → persisted operation receipt
      └─ loop_guard::observe with actual returned data only
      ↓
verification::check_acceptance / existing owner review / recorded usage
```

`runtime/adaptation.rs::observable_path_state` adds bounded SHA-256 for files <=1 MiB, including same-length changes, to error recovery and repetition fingerprints. Larger files are identified without an unbounded hashing read. No file bytes or secrets enter fingerprint audit records.

## New evidence and boundaries

- **Structural tests:** `provider/tests.rs` keeps paired tool-call IDs/result outputs and current instructions when projecting a constrained model view; uncontrolled providers keep full evidence; overflow classification excludes unrelated errors.
- **Pure-policy tests:** `runtime/loop_guard.rs` verifies 2 real successes, third refusal, fifth stop, reopening after changed content or filesystem fingerprint, and exemption of all external/nonread effects. `adaptation.rs` tests same-length edits and hash-size limits.
- **CI:** GitHub Actions builds web, then cargo tests; source commits link through branch history. CI does not simulate a model intelligence gain or a token bill.
- **Real model:** No live Qwen/paid model call in this batch. Pass@1, requested/actual cached input tokens, context-overflow recovery success, model calls per verified artifact, total billed KRW must be measured on matched tasks before claiming a performance improvement.
- **Limits:** Character/token estimates can still undershoot uncommon provider tokenizers. The fallback retry cannot rescue a truly irreducible current task/instruction, and intentionally stops after one bounded retry. The read-loop guard excludes external polling, but a very large local file is not content hashed; its mtime/size still gate repetition, so that is a measured tradeoff rather than a guaranteed coherent cache.
- **Verification boundary:** Tool refusals are errors, not successful `execution_operation` receipts. Existing approvals, cancellation, idempotence rules and JSON protocol pairing remain in force. No weights/LoRA/fine-tuning introduced.

## Next measurable acceptance

Run `scripts/sub2-goal-eval.mjs` in an explicitly isolated benchmark environment with the same Qwen/provider configuration before and after these commits. Record prompt cache/usage as **reported or estimated**, verify actual file byte oracles and job receipts, and compare (a) tasks completed, (b) repeated-read invocations, (c) context errors, (d) input tokens per verified task including failures. Real inference spend must be consented to separately.
