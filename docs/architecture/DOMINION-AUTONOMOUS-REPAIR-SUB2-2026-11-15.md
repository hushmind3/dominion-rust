# DOMINION Native Autonomous Repair and Product Integration — 2026-11-15

Branch: dominion-sub2. Native execution engine: Rust. Optional network transport: TypeScript.

## Actual end-to-end execution path

Authenticated owner -> goal API / Rust SDK / web UI
 -> durable SQLite goal and job -> owner-directed objective revision and frozen acceptance checks
 -> Rust planner + indexed repository context -> TurnRequest
 -> Rust model/tool execution loop -> tool_end/tool_error events
 -> TurnStats with persisted successful operations AND bounded failed stderr/exit status
 -> Rust deterministic acceptance and effect uncertainty checks
 -> goals/diagnostics.rs: stable SHA-256 observed-error fingerprint and repair guidance
 -> native new plan (on repeated real error or existing stall threshold)
 -> next real model/tool turn -> independent acceptance -> immutable goal receipt.

Source modules:
- src/engine/mod.rs : bounded_tool_failure and TurnStats.tool_failure_details
- src/engine/goals/diagnostics.rs : actual compiler/test/environment/tool error classification; persisted repetition-aware replanning
- src/engine/goals.rs : real execution loop, diagnosticHistory storage, source re-navigation after owner objective revision, repair/review/acceptance transitions
- src/engine/goals/turn_input.rs : bounded incremental prompt/observed acceptance gaps
- src/engine/owner_objective.rs : owner revisions atomically replace the original goal and freeze new checks
- src/sdk.rs : native DominionSdk::run returns failure_details and verified success evidence
- web/src/components/GoalDirector.tsx : owner visible failure classes, repeated errors and replan count

No third-party SDK, provider-specific policy or LLM-generated success assertion owns the native verification decision.

## Executed verification

- Real Rust tool-flow test: two failed workspace reads from a scripted provider, observed error->forced replanning, real Rust workspace_write, independent file acceptance and durable receipt.
- Deterministic Rust classifier tests for replay fingerprints, distinct errors, bounded stderr, unknown side effects and objective gaps.
- Rust model/tool/session tests, Node SDK tests, React typecheck/build run through the repository's cargo test --release CI.
- The fixture uses a scripted model, NOT a paid frontier-model score. Actual SWE-bench solved count requires the separate real-model benchmark driver and official Docker grader; it remains unmeasured here.

## Product scope preserved and incrementally strengthened

Autonomous software edits, native tools, MCP and SDK, owner steering, persistent jobs, provider routing and group chat retain their original engine path. User objective revision recomputes source navigation in the same iteration as the new goal so no old file-focus prompt controls it.

Video: LTX film now composes custom owner scene prompts with a persistent character/wardrobe/setting/voice anchor where budget allows. It preserves the exact owner's scene words; video technical metadata, verified paid clip SHA-256 and explicit retake consent remain separate from unverified semantic continuity. No new paid planning calls added.

RSI: Existing native Rust paired independent evaluator, preserved holdout exclusion from next-parent fitness, measured diversity niches and operator A/B selector stay in place. No model weight training is added.

Full frontier parity, independent semantic video continuity, recorded paid KRW unit economics and multi-generation measured RSI gains still require empirical external model and evaluator runs, not file-count or test-name claims.

## Sources inspected in architecture comparisons

US: https://github.com/openai/codex and https://github.com/openai/openai-agents-python
China: https://github.com/deepseek-ai/deepseek-harness , https://github.com/agentscope-ai/agentscope , https://github.com/bytedance/deer-flow , https://github.com/QwenLM/Qwen-Agent , https://github.com/ANative-Lab/EvoAgentX
Evolution: https://github.com/algorithmicsuperintelligence/openevolve , https://github.com/SakanaAI/ShinkaEvolve
