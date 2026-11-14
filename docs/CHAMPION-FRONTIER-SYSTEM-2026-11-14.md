# DOMINION Champion — user sovereignty + whole-engine integration (2026-11-14)

**Branch:** `dominion-champion`. This document describes **committed execution paths**, not a claim that a frontier benchmark or full CI has passed.

## Operating principle

DOMINION owns its Rust executor, planner/algorithms, durable state, provider selection, tools, evidence and RSI. An authenticated owner's **latest instruction** takes precedence over conflicting earlier objectives or untrusted derived plans. Retrieval, peers, synthetic tasks and upstream SDK defaults cannot impersonate the owner. Actual effects already started are real and cannot be wished away; instructions steer the *next unstarted execution boundary*. This means owner control, **not merely a prompt slogan**.

## Implemented and connected paths

| Area | Live source path | New decision/effect |
| --- | --- | --- |
| Owner command lifecycle | `web/src/pages/Chat.tsx` → `POST /api/sessions/:id/steer` or `DominionSdk::steer` → `engine/steering.rs` → SQLite `chat_steer` | Verbatim owner input persisted with a unique ID; delivered once at the next model step; crash recovery does not duplicate it |
| Obsolete tool prevention | `chat/runtime/turn.rs` → `steering::has_pending` → `tool_dispatch::skip_for_new_owner_instruction` | If owner corrects during model generation, unstarted proposed tools get explicit `executed:false` tool results with their original call IDs; started effects settle normally |
| Bounded read parallelism | `chat/runtime/turn.rs` → `StreamExt::buffered(4)` → `check_tool_result_pairs` | Up to four independent reads; model-call ordering preserved in result pairing; side-effect tools stay on the serial path |
| Owner task provenance | `owner_intent.rs`, `chat/context.rs`, `chat/runtime/turn.rs`, `task_episode` | Frozen original user source remains attributed; a later authentic owner correction supersedes conflicting historical root details |
| Measured model choice | `rsi_model_learning.rs::learn_verified` → `router.rs::select_provider` → `chat/provider.rs` | Replays original detached-source candidate metrics and real model-usage provenance before comparing independently matched RSI cohorts; deep **auto** routing can prefer observed quality. Owner/agent explicit route selections are unchanged |
| Real source evolution | Existing `rsi/execution/campaign.rs` → compiled descendant controller/worker → `rsi_worktree` → independent evaluation/adoption → `rsi_model_lineage.rs` | Keeps independent frozen evaluation and restricted model candidate pool; model selection learns from compiled-source results, not unsupported claims of changing API model weights |
| KRW model price and audit | `Models.tsx` → `POST /api/providers` → `gateway::record_usage[_tokens]` → SQLite `audit` → `session_usage`/`usage_summary` → `Chat.tsx` | Owner enters **both** input/output prices in KRW per 1M tokens. Missing prices are unknown. Usage shows explicitly marked reference cost with known-priced-call count, **not** provider billing |
| Video quality and retakes | Existing LTX multi-shot + per-clip SHA/ffprobe receipt → `video/diagnostics.rs` FFmpeg black/freeze scan → `film.rs::scenes` → `FilmRetake.tsx` | GPU-free short sample; measured black and frozen seconds guide owner-selected retakes. No automated paid resubmission or semantic quality claim |
| Group collaboration | Existing `rooms.rs`, `rooms/routing.rs`, `api/rooms.rs` | Preserved owner-selected speaker/model pools, adaptive discussion and feedback-driven multiagent economy |
| Goals, workflows, skills, automation | Existing `goals.rs`, `workflows.rs`, `jobs.rs`, `goal_artifacts.rs` | Preserved durable worker/resume, scoped skill snapshots, intermediate artifact SHA receipts, repeated-workflow idempotency |

## Empirical inference — strict limits

- `rsi_model_learning::learn_verified` replays the source-backed paired evaluation and checks the registered provider's original usage; unverifiable generations do **not** train the production router. The pure `learn` function remains usable for isolated fixtures/tests.
- A route is promoted only if the existing learner finds independent comparable baselines across at least two domains; an explicit user-selected provider/model does not go through auto routing.
- Model cost quotes are computed from owner-declared input/output tariffs. Cache-discounted cost, taxes, credits, invoices, missing token splits and unpriced calls are **not** inferred or zero-filled.
- Black/freeze findings are diagnostics; a dark artistic frame is not automatically an error. A completed paid clip is never silently purchased again.
- A changed owner goal does not retroactively alter completed work. Proposed but not started tools receive synthetic skipped outcomes; the next model decision sees the newest instruction.

## Regression contracts

- `engine/steering.rs`: distinct identical-text instructions, persistence, crash replay, no room/archived injection.
- `engine/rsi_model_learning.rs` and `router.rs`: comparable-source model preference, known-KRW price only when both tariffs are present, explicit route preservation by the caller.
- `engine/video/diagnostics.rs`: black/freeze windows, normal shots, invalid metrics and clamp.
- `engine/video/quality.rs` and `film.rs`: existing ffprobe and per-shot SHA receipts remain authoritative.
- `engine/gateway.rs`: known-price KRW quote and unknown-price rejection.
- `scripts/champion-system-contract.test.mjs`: static, source-level cross-component checks of new API/UI/engine integration.

## Verification status and remaining frontier work

**Confirmed:** GitHub accepted commits to `dominion-champion`; the inspected HEAD contains the runtime, module, API and UI references above. **Not executed in this environment:** Rust `cargo test`, complete web build, CI, forced-crash test, paid LTX calls, real-model coding campaigns or a frontier benchmark. No score, reduced costs, or increased capability is claimed without a trace.

Next high-value empirical tests:
1. Scripted Rust model + tool fixture: receive owner correction between first and second tool; assert original ID got a non-executed tool result and zero second side effect, including forced restart.
2. Two-source real coding tasks across multiple models, matched frozen workload and independent quality/time/price observations. Test router over a held-out set against the static baseline.
3. Film test with ffmpeg generated normal/dark/frozen synthetic clips and a multi-clip receipt; verify each warning matches observed frames and that the selected paid retake is exactly one approved submission.
4. Long-running goal/workflow: verify checkpoint resume, irreversible effects and actual file-grade acceptance; compare multiagent vs solo under the same cost envelope.

## Source comparison (patterns, not foreign runtime dependencies)

- China: [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (step/turn inbox, session-event checkpoints, ordered tool pairing), [DeerFlow](https://github.com/bytedance/deer-flow) (isolation, skill/subagent coordination), [AgentScope](https://github.com/agentscope-ai/agentscope), [Qwen-Agent](https://github.com/QwenLM/Qwen-Agent).
- US: [Codex](https://github.com/openai/codex) (Rust tool contracts, event/turn lifecycle), [OpenAI Agents](https://github.com/openai/openai-agents-python) (execution hooks), [Microsoft Agent Framework](https://github.com/microsoft/agent-framework).
- All new logic remains **native DOMINION Rust**, with ordinary HTTP/SSE and React for presentation. External harness policy engines and Node-based runtimes are not embedded.
