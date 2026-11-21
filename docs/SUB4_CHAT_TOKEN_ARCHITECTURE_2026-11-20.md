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

## 2026-11-20 live Cloudflare model sample (account-owned Workers AI, **not** Rust production)

A single small *real inference* A/B was executed through the connected Cloudflare account's official Workers AI API. There was no mock usage receipt. Its exact request shape was the prior one-string 12-message/6,500-character replay versus the new role-preserving six pinned + six recent bounded-message projection. Source messages and current question were deterministic, repeated engineering notes. Model: \`@cf/meta/llama-3.1-8b-instruct-fast\`, temperature 0, maximum 24 output tokens in each arm.

| Actual provider metric | Old composite transcript | New bounded projection |
|---|---:|---:|
| Message objects | 2 | 15 |
| Serialized provider message characters | 6,903 | 6,401 |
| Observed input tokens | 1,321 | 1,207 |
| Observed output tokens | 24 | 24 |
| Observed cached prompt tokens | 0 | 0 |
| Observed neurons (Workers AI unit) | 6.277448654 | 5.807932377 |
| HTTP request elapsed (single sample, milliseconds) | 639 | 788 |

**Observed input-token difference: -114 (8.6%)** for this one synthetic replay workload on this model. It is **not a universal speed, quality or KRW saving**. No cache hit was observed. The new message structure has more message boundaries and a different selection of historical evidence, so output quality is not causally comparable from a single sample. The two responses were both grounded engineering advice, not task-completion proof. Wall-clock time actually increased by 149 ms in this one sample and has large variance; this is no prefill-speed claim.

The connected live \`dominion-cloud\` Worker exposes only D1, Workers AI, allowed-origin and Google client-ID bindings; it currently lacks paid OpenAI/Anthropic supplier credentials and a priced paid-model registry. A live **paid OpenAI** prompt-cache hit-rate or KRW reconciliation A/B cannot be run from this deployment. The execution container has no accessible Ollama/llama.cpp process, weights or GPU; no live **local-model prefill** measurement can be inferred from cloud-only receipts. Both are kept as unmeasured, rather than fabricated numbers.

The native Rust and Cloud code now pass exact cache-boundary indices through a strict single-origin header \`x-dominion-cache-anchor\`; the gateway validates the index and never marks the changing latest request as a paid cache write. Its source tests use deterministic local protocol mocks. The user-visible web model usage remains receipt-based.

A one-off tagged CI push runs actual Rust HTTP + Vite + Chromium integration with a loopback HTTP provider, rather than mocking the React frontend or paid-provider usage. The live HTTP stream, persisted session, 3 ordinary conversations, tool-free catalog, usage counters and stable profile prefix are verified. This is separate from the Workers AI direct inference sample.

### Release boundary

Only \`dominion-sub4\` code and test files were committed. No live \`dominion-cloud\`, \`dominion-website\`, \`dominionai.kr\` or production database migrations were modified. A deployment of the experimental Sub4 backend over the user's main production service would breach the existing independent-branch constraint and cannot provide actual model credentials that are absent from the account. Release is therefore a separate controlled action after real-provider credentials and host access are available.

## 2026-11-20: Repeatable real inference receipts

### Real Workers AI A/B: three paired independent requests

Through the authenticated Cloudflare Workers AI endpoint, the actual model
@cf/meta/llama-3.1-8b-instruct-fast received the same historical engineering
data and task under the previous 12-turn composite prompt and the Sub4-style
six stable + six recent role-preserving projection. All three pairs returned
identical token counters for the selected deterministic input (temperature 0;
max 24 output tokens). **This is direct Workers AI inference, not a deployed
DOMINION Rust runtime.** Answer quality is not inferred from the token counts.

| Upstream receipt | Old composite | New bounded projection |
|---|---:|---:|
| Input tokens, each of 3 samples | 1,321 | 1,207 |
| Output tokens, each sample | 24 | 24 |
| Cache-read tokens, each sample | 0 | 0 |
| Workers AI neurons, each sample | 6.277448654 | 5.807932377 |
| HTTP elapsed, ms | 639, 715, 595 | 788, 664, 476 |

**Observed difference: 114 fewer input tokens per call, or 8.6%, for this
synthetic prompt workload.** Neurons decreased roughly 7.5%; median HTTP
latency was 639ms old and 664ms new, so no latency saving is claimed.
There were no cache hits. Because different history was selected, completion
quality and capability must be compared separately before a general claim.

### Actual llama.cpp / Qwen3.5 CPU prefill

[Successful isolated GitHub Actions run](https://github.com/hushmind3/dominion-rust/actions/runs/44711254578)
used a real 2026 Qwen3.5-0.8B-Q4_K_M GGUF (public Unsloth distribution,
about 508MB) inside the official ggml-org llama.cpp server Docker image.
The GitHub-hosted CPU had a 4-vCPU limit, a single model slot and one stable
8,878-character prompt head; three /completion requests differed only in
their current-question suffix. Cache reuse was enabled on each request.

| Server receipt | Cold | Warm 1 | Warm 2 |
|---|---:|---:|---:|
| HTTP elapsed, ms | 9,913.3 | 284.9 | 269.9 |
| Prompt evaluated tokens (prompt_n) | 1,520 | 2 | 3 |
| Prefill time (prompt_ms) | 9,688.328 | 77.546 | 58.624 |
| Server-reported tokens_cached | 1,527 | 1,525 | 1,526 |
| Server-reported tokens_evaluated | 1,520 | 1,518 | 1,519 |
| Generated tokens | 8 | 8 | 8 |

This directly verifies a huge **local KV-prefill reuse** on a fixed
prefix for that llama.cpp instance. The two token fields above preserve
the server's own naming/meaning; cached KV tokens are not a billing discount.
It is NOT a measurement of the user's local RTX 3070, an Ollama service,
or a 27B model, and the extremely short outputs are not an agent benchmark.
[Download the actual JSON result on GitHub](https://github.com/hushmind3/dominion-rust/actions/runs/44711254578/artifacts/14616927567).

### Verification boundaries

- Real Rust HTTP + React/Vite + Chromium, with a transparent loopback
  model **fixture**, passed the 9-test E2E suite on
  [run 44709808079](https://github.com/hushmind3/dominion-rust/actions/runs/44709808079).
- Real independent Workers AI model inference: three old/new pairs above,
  including real upstream model usage receipts.
- Real llama.cpp Qwen3.5 local CPU inference: cold and warmed KV receipts
  above, no fabricated tokenizer counters.
- Real first-party paid OpenAI GPT-5.6+ cache economics in KRW: NOT measured.
  The connected production Cloudflare Worker exposes only D1, Workers AI,
  allowed-origin and Google OAuth client ID; no paid supplier secret or model
  tariff is configured.
- Production Dominion website, Worker and user hardware are unchanged.
  No Sub4 code was silently released onto the production main service.


## 2026-11-21: Common-product contract hardening (Sub4 only)

Scope: `dominion-sub4` only; production services, other branches, navigation and UI layout unchanged.

- **Korean no-tools selection:** `no_tools_required` now recognizes complete multiword social exchanges such as `안녕하세요 반가워요` and self-contained timeless Korean explanation requests. Any explicit current-data lookup, attachment, file, repository, approval, modification, or previous-task reference still uses the normal tool-capable Run. No assistant/worker approval is bypassed.
- **At-most-once model-only dispatch:** after provider throttling, the Run Store atomically persists `model_only_request_intent` **before** handing the request to a provider. `Engine::launch` refuses to replay a model-only Run containing an earlier intent, including after a crash. This chooses an uncertain/needs-reconciliation outcome over silently charging twice; it is not a claim of exactly-once provider-side execution.
- **Correct effective budget:** `drive_model_only` now honors `account()`'s failure decision, which includes reserved/delegated budget rather than only the raw budget ceiling. A failed settled receipt cannot be overwritten with `Completed`.
- **Remote Worker result accuracy:** a Writer completion now reports `writer: true` and `readOnly: false`, while an Inspector reports `writer: false` and `readOnly: true`. The isolated patch-review and separate remote charge rules are unchanged.

Regression tests added/extended:

- `korean_social_variants_are_tool_free_without_losing_tasks`
- `korean_explanations_skip_tools_but_followups_remain_tool_capable`
- `model_only_reserved_budget_failure_cannot_become_completed`
- `model_only_uncertain_request_is_never_replayed_after_restart`
- `model_only_chat_uses_native_worker_once_and_records_usage` asserts exactly one persisted send-intent.
- Existing remote Inspector HTTP test asserts `writer: false`.

**Verification state:** Changes committed only on `dominion-sub4` (chat context, native runner, launcher, model-only tests, remote-worker adapter and this audit). The send-intent update also follows the existing `RunStorePort::mutate` contract (`Result<Run, String>` with a `Result<(), String>` mutation callback). Source paths and expected contract branches were reviewed. The editing environment did not provide a Rust toolchain or a local repository checkout, so Cargo, TypeScript, Chromium and all CI suites were **not executed** in this pass. Added tests are not presented as passed. The earlier Workers AI and llama.cpp measurements above are unchanged historical reference only; no new cost, local prefill, actual model quality, or production behavior measurements are claimed.
