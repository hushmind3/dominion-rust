# Sub4 frontier harness source comparison — 2026-11-18

This document records source-level design evidence, not a claim that Dominion outperforms a frontier harness in independent live-model benchmarks.

## Repository scope

- Modified only `dominion-sub4`. Source inspected on the five current branch heads at start of work: `main` `816f14a9`; `dominion-sub2` `4b89e06a`; `dominion-sub3` `e9921c13`; `money` `a8e888df`; `dominion-sub4` `dc22c078`.
- Other branches use the frontend `src/engine/chat/room_context.rs`, `src/engine/rooms.rs` and `src/engine/gateway.rs`. Sub4 uses a separate native Rust `src/group.rs`, `src/frontend_rooms.rs` and `src/provider.rs`. Their tests and paths are *not* equivalent solely because the product screens look alike.
- `dominion-sub3/src/engine/chat/room_context.rs` separates owner state, prior speaker reply, retrieved evidence, peer history and input component metering. `dominion-sub2` additionally maintains bounded source-indexed peer briefs; `main` and `money` also maintain their own source-ranked context compilers. Our Sub4 previously ranked short owner/peer excerpts but did not keep a speaker's latest reply after the shared rolling window sealed.
- Previous Sub4 benchmark `9×3` failed because `maxTurns` erroneously capped lifetime `room.rounds`, despite intended per-owner allowance. Fix and 30-message regression are in `src/group.rs`, `src/frontend_rooms.rs`, `src/frontend_rooms/fanout_tests.rs`. `lifetimeMaxResponses` now independently limits total lifetime responses.

## 2026 official/open-source comparator code

| Harness | Source and checkable principle | Applied to Sub4 |
| --- | --- | --- |
| Anthropic Claude Code (Apr 30, 2026) | [Prompt caching is everything](https://claude.com/blog/lessons-from-building-claude-code-prompt-caching-is-everything): static → project → session → messages; append updates rather than rewriting system prefix | Shared group prefix; mutable learned memory moved to an end suffix |
| DeepSeek Harness (Jul 5, 2026) | [Reconstructable requests](https://github.com/deepseek-ai/deepseek-harness/blob/master/.agents/notes/implemented/architecture/2026-07-05-reconstructable-requests.md): immutable event projection and per-request reconstruction | Source-indexed durable Room + deterministic window checkpoint; full wire-byte audit still not implemented |
| Qwen Code (2026 active design) | [Chat compression cache sharing](https://github.com/QwenLM/qwen-code/blob/main/docs/design/chat-compression-cache-sharing.md): unchanged session-key/prefix; avoid cold summarizer requests | No extra summarizer model call; stable OpenAI cache key per chat and room-specific provider affinity |
| OpenHands SDK (2026) | [Condenser](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/context/condenser/README.md): append-only event log, view projection and explicit cache-busting compaction tradeoff | Full original transcript kept separate from selected model window |
| Moonshot Kimi Code (2026) | [Server sessions](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/reference/server-api.md): session isolation, resumability, explicit mutation boundaries | Persist chat identity before launching native Run; reused through distinct user turns |
| OpenCode Go (Sep 2026) | [Official Go docs](https://dev.opencode.ai/v2/docs/console/go/): stable `x-opencode-session` per conversation; missing header previously caused 400 in clients | Removed Sub4 branch-wide constant; use exact-host HTTPS and durable Run/Room/session identity |
| Google Gemini CLI (2026) | [Session management and settings](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/settings.md): session turn retention separate from compression threshold | Per-owner answer budget separate from lifetime response budget; context projection separately bounded |

## Source-informed 2026 prefix-pressure fix

- [TokenPilot (2026-06, arXiv:2606.17016)](https://arxiv.org/abs/2606.17016) separates contextual token limits from provider prefix cache hit economics; selective/batched invalidation avoids gratuitous cache resets.
- Sub4's initial checkpoint envelope previously reserved 38% for empty evidence and reset after 16 turns even with free budget. `project_room_context` now reserves that stable-evidence space only when a sealed checkpoint exists, permits up to 48 short appended messages while below the actual char budget, and then returns to bounded 20-message post-checkpoint segments. Exact owner/peer evidence and native receipts remain unchanged.
- This is a source-guided optimization, **not** a measured reduction in actual provider billable tokens. The 30-message prefix and forced-rollover owner/self memory tests are deterministic Rust tests.

## Actual paths changed

1. `src/group.rs`: per-owner response budget and separate lifetime maximum; owner corrections pinned as source-indexed evidence; shared append-only transcript; speaker-specific last answer as a bounded suffix; mutable auto-learning notes as another bounded suffix.
2. `src/frontend_rooms.rs`, `src/frontend_rooms/fanout_tests.rs`: direct 9 speakers × 3 owner requests = 27 agent replies + 3 user turns; duplicate clientMessageId idempotence, provider 400 same-turn recovery; reported cached vs uncached token accounting and per-speaker usage.
3. `src/provider.rs`, `src/engine.rs`, `src/domain.rs`, `src/frontend_api/chat.rs`: OpenCode Go first-party session routing by real Run or Room UUID, UI chat across Runs by stable hashed session ID, first-party OpenAI cache-key inheritance, measured upstream token receipts and missing-usage flags.
4. `src/api.rs`, `src/sdk.rs`: same per-room affinity on direct SDK/native API entrypoints; no process-wide cross-room collisions.
5. `src/store.rs`, `src/frontend_api/operations.rs`, `src/frontend_api/sdk_endpoints.rs`: expose native coding token receipts to jobs/SDK/executions without re-estimating counts.

## Evidence classification and remaining limits

- **Source/CI evidence:** Rust targeted CI checks source- and mock-model-based regressions; GitHub Actions results show actual pass/fail. The mocked model provides synthetic token usage, useful to verify measurement transport but **not** proof of real provider cache hits.
- **Independent live measurements still needed:** same model/provider and identical 9×3 conversation across branches; actual provider `cachedPromptTokens`, `uncachedPromptTokens`, context recall accuracy and wall time; 3 coding workloads with independent semantic artifact verification.
- **Not equivalent yet:** full request byte-exact replay (DeepSeek) and learned semantic summaries (OpenHands) are outside the current bounded deterministic room projection. Do not infer that history preservation equals perfect recall or that a stable prefix guarantees discounts.
