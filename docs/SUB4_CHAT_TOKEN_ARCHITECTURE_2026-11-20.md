# Sub4 — 2026-11-20 chat token architecture audit

## Scope and evidence

This audit is about **ordinary one-to-one chat** and the Rust worker, not primarily the 9-speaker room benchmark. The first pass over-emphasized remote cache flags and mistook transport-level server continuation for token-cost reduction. This note makes the actual constraints explicit.

Sources checked on 2026-11-20:

- OpenAI 2026 prompt caching contract: https://developers.openai.com/api/docs/guides/prompt-caching
- OpenAI Responses conversation-state contract: https://developers.openai.com/api/docs/guides/conversation-state
- OpenAI prompt cache diagnostics: https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics
- Anthropic 2026 cache prefix: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
- llama.cpp server prompt/KV cache implementation: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
- Sub4 comparison of main chat context ownership: `main:src/engine/chat/context_engine.rs`; sub2 tool selection: `dominion-sub2:src/engine/chat/tool_projection.rs`; sub3 receipt search: `dominion-sub3:crates/dominion-agent-sdk/src/context.rs`. These were read as separate branches; no cross-branch edits were made.

## Architectural faults found

1. **A new execution Run for each user turn is not, alone, a reason to expect higher billable tokens.** The previous_response_id option can avoid re-uploading history but OpenAI still bills earlier tokens when they participate in the current context. Cost optimization needs context selection and *actual* cache reuse, not only stateful transport.
2. **Always-on tool-enabled chat paid for a large native + external tool catalog even during casual exchanges.** There is no guarantee the conversational user needs code-edit or remote-call schemas. Keep execution policy and approvals separate from schema exposure.
3. **The previous renderer made one giant, changing user message** by concatenating profile, historical messages, and current request. This prevented stable prefix matching and erased previous user/assistant roles.
4. **The first revision kept a fixed first-four prefix + last-eight suffix, but that is not sufficient for cache eligibility**: GPT-5.6+ require at least 1,024 *actual model tokens* in the reusable prefix. Merely tagging a short message with a breakpoint is not evidence of a cache hit. Extending the head without a total history cap can also INCREASE paid input.
5. **Manual compaction used `ChatCompaction.through` for storage but not for later projection**, so archived snippets and old messages could be sent twice.
6. **History eviction without retrieval damaged continuity.** Local exact historical recall can be triggered for explicit prior-discussion questions, without creating an extra model request.
7. **Token totals include cache hits.** Summing prompt and output receipts is not a measurement of uncached prefill. Missing provider cache telemetry must be classified unknown, not as an observed zero cache hit.
8. **Dynamic prefix content also breaks llama.cpp slot reuse**, independent of paid API cost. A stable initial schema and a stable history head matter for local model prefill latency.
9. **Provider-specific cache controls are not interchangeable.** Anthropic explicitly hashes the tools/system/messages prefix, OpenAI GPT-5.6+ accepts explicitly marked content breakpoints and a 30m minimum TTL, and local runtimes rely on exact prefix equality.

## Implemented Sub4 changes

- `src/frontend_api/chat_context.rs`: independent, role-preserving deterministic context compiler. Up to six immutable early chat messages and six most recent history messages, with hard per-message clipping and a stable explicit breakpoint after the fixed segment. Previously copied archived source messages are excluded using `compacted.through`.
- Same compiler: optional bounded multilingual local relevance selection for explicitly asked earlier-chat details; retrieval does not alter the previously cached head or the durable full transcript.
- `src/frontend_api/chat.rs` + `src/engine.rs`: exact social utterances use the existing single-call model-only worker instead of advertising any tool schemas. Ambiguous task follow-ups remain in the native tool harness.
- `src/tool_discovery.rs`: static small native chat tool head, with controlled on-demand discovery, avoiding tool-list reshuffling on every local provider call.
- `src/provider.rs`: first-party GPT-5.6+ explicit cache breakpoints, unchanged suffix behavior and keyed, per-conversation prompt routing. For unsupported provider endpoints, proprietary fields are withheld.
- `cloud/src/index.js`: tenant-bound paid caching and cache-read/write cost separation. Cloud Workers AI without usage receipts do not fabricate zero tokens.
- `src/frontend_api/sessions.rs`, `src/frontend_growth.rs`, `web/src/components/TokenUsage.tsx`: measured cached, uncached, writes and unknown usage are surfaced separately.
- `src/frontend_api/chat_context.rs` tests include a 161-step synthetic chat baseline replay comparison. The test measures **actual serialized history character selections**, not fabricated model token savings or live provider cache hits.

## What the CI does and does NOT prove

CI verifies Rust compile, deterministic context/tool contract tests, SDK/UI/Worker tests and offline backend regressions. It **does not** establish real vendor prompt-cache hit rates, real costs or local llama.cpp prefill acceleration. CI's mocked usage counters must not be presented as measured savings.

For a proper A/B on unchanged model, provider, prompt and dataset, capture each real request's:
`model`, `input_tokens`, `cached_tokens`, `cache_write_tokens`,
`output_tokens`, `cost_krw`, `tool_schema_chars`,
`context_chars`, cache miss diagnostics (where supported) and actual elapsed prefill time.
Compare total cost, prefill latency and task completion quality; never present raw model input counts alone as an economic saving.

**Decision:** Prefer preserving an exact reusable prefix and capping the paid history projection. Do not force all chats into remote Responses continuation and claim that it eliminates prior-context billing.

## 2026-11-20 final architecture pass

The stable frontend transcript remains in `FrontendStore`; a separate pure `src/frontend_api/chat_context.rs` projects a bounded cacheable head, a six-message recent suffix and optional explicit older-evidence retrieval. It uses one durable Run per user turn to retain the existing cancellation/approval interface, but never serializes all archived transcript messages for each provider call. After user-initiated compaction, the exact `ChatCompaction.through` watermark prevents double replay; an explicit history question may still select bounded original raw evidence from durable history. `src/context.rs` now preserves the annotated head when tool receipts force hard compaction, without elevating older messages to new user commands.

High-confidence ordinary social and provided-text translation turns use the existing model-only worker without tool schemas; ambiguous/long-horizon instructions continue through the normal native agent tool/search/approval kernel. The default native chat tool set remains byte-stable for llama.cpp/Ollama prefix reuse. All real tool dispatches still pass the original policy and approval checks. There is no side executor or new UI menu.

First-party OpenAI GPT-5.6+ explicit cache breaks are tested with provider request JSON; third-party OpenAI-compatible endpoints are not given incompatible fields. Native Rust sends a hashed `x-dominion-chat-affinity` only to its own exact HTTPS Cloud Worker. The gateway combines its verified D1 account ID and model identity with that hash before generating OpenAI cache-routing keys. Changing user questions never become a new explicit breakpoint.

Rust and Cloud usage diagnostics are distinct: provider token receipts (including cached reads and cache writes), serialized context/tool characters, and pricing-known flags. A character reduction does not prove paid-token savings. In addition to existing per-session totals, successful Run metadata now retain bounded per-model-round input/cache/output and the corresponding source/tool-schema character footprints. Default remote native models without verified tariffs and separately registered remote API models explicitly signal unknown KRW pricing; zero is not reported as a known free charge. `cloud/src/index.js` settles cache reads, writes and ordinary input under configured micro-KRW tariffs and preserves the existing uncertain-reservation behavior.

The 161-step synthetic benchmark asserts fewer real outgoing history characters than the previous 6,500-character replay on identical lengthy simulated messages. CI is source-contract and mocked-provider evidence only; neither an OpenAI/Anthropic paid A/B cache-hit rate nor local model prompt-prefill speedup has been observed. Do not claim a percentage or production deployment until independently measured.
