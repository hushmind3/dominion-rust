# Sub4 Runtime Review — 2026-11-17

## Sources actually compared

- Sub4 at 9301baab1c66; main at b6f5939fb01b; sub2 at 42ef96a04ba9; sub3 at 94ccbb71825. Same repository branches (hushmind3/dominion-rust).
- OpenHands Software Agent SDK code and architecture: https://github.com/OpenHands/software-agent-sdk — core ownership of execution/session/events; Canvas/UI owns presentation.
- Qwen Code 2026 architecture: https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md — UI/CLI/SDK share core tool execution and permission.
- Anthropic, June 18 2026, Skills/hooks/subagents: https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more — deterministic tool hooks should not consume model tokens.
- In-repo main native CLI OAuth at src/api/cli_oauth.rs; sub3 kernel bridge at src/engine/chat/kernel_bridge.rs.

## Before: three redundant chat loops and a blocked recovery action

Sub4 used a 250ms polling loop to cancel model-only inference and separate 350ms polling loops for session completion and streaming browser events. Each loop called Store::get, cloning the entire Run including model messages, receipts and events. In a quiet, non-terminal execution these paths could together perform roughly 9.7 full Run clone attempts each second, before unrelated UI polling. The legacy POST /api/jobs/{id}/resume always returned 501 despite having a native guarded Engine::launch.

## After: a single durable Run change-feed contract

The Store already broadcasts lightweight RunUpdate records immediately after an fsync-backed write. Store::changed filters updates by Run ID, ignores unrelated runs, and treats receiver lag as a signal to re-read the authoritative snapshot. Model-only chat is now an Engine-owned worker using the same cancellation watch channel and provider/budget accounting as autonomous jobs; a separate frontend model executor was removed. Cancellations take priority over provider completions. Session finalizers and SSE subscribe rather than timer-polling. A disconnected browser does not own the task or finalizer. Resume delegates to Engine::launch and preserves all protection against unresolved mutations, invalid restart conditions and repeated effects.

The account bridge now also supports official GitHub CLI browser/device login for local deployments that lack an OAuth app; credentials stay in the encrypted vault. A service-published Google OAuth app still needs to be registered by its operator.

## Verification and honest measurement

- Native regression: a different Run's update cannot spuriously wake a watcher; the target's fsync-backed update does.
- Native job resume regression: same execution ID, no new Run, unknown remote mutation cannot be resumed without reconciliation.
- Browser regression: GitHub CLI one-time code can be displayed without showing an OAuth credential form; OAuth server apps continue to work.
- CI: see branch's latest actions runs (Rust, web, TypeScript SDK, browser, external MCP, durability soak).
- The 9.7/s is a theoretical old-code loop rate (1/0.25 + 2/0.35), **not a measured CPU/DB benchmark**. After change the chat paths do not periodically query Run history while idle. Completion/token quality was not benchmarked against paid live models, so no frontier-completion claim is made.

## Native model-only convergence and selected model attribution

The preexisting frontend ran an independent model-only worker, with its own provider invocation, token/cost logic and cancellation polling, outside the native worker registry. That duplicated responsibilities and left non-tool chat less observable to the SDK than tool-based chat.

Now Engine::launch selects an agent or model-only kernel path under the same active worker, cancellation and durable status transitions. Tool-free runs provide no tools, perform one model request, charge the same Reply accounting and publish a verified receipt only if the request actually completes before cancellation. The SDK continues to disallow resuming an unbound, interrupted tool-free chat after restart, preventing double-billing. Session sync now records the actual selected provider/model rather than labelling every completion as the configured default model.

Tests use a fake local OpenAI-compatible server and check one request, no advertised tool definitions, KRW token billing, persisted Completed/Cancelled states and accurate session metadata. They do not claim live frontier model quality equivalence.
