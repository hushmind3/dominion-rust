# Sub4 9-speaker, 3-owner-turn live measurement protocol (2026-11-19)

## Transport and failure boundary

The native UI now sends `POST /api/rooms/{id}/messages/background` with the same ordinary JSON `input`, `agentIds`, and stable `clientMessageId` contract. A newly accepted turn returns HTTP 202 with `job.status=running`. The worker continues via the same native `frontend_rooms::generate` → `Groups::next_for` model path and persists each actual speaker reply/usage receipt. The old synchronous `/messages` endpoint is retained for compatibility and isolated low-latency tests.

The client must query `GET /api/rooms/{id}/state` and `GET /api/rooms/{id}/messages`. While in flight, `state.status=running`, `jobs[0].stepIndex` tracks completed responses, and `queueDepth` / `activeAgentId` describe pending speakers. After all requested speakers, `state.status=idle` with no running job. `interrupted` means the last provider call failed; the durable `lastProviderFailure` stores safe `http_status`, `upstream_code`, `participant`, `context_chars` and progress. Same-ID retries while running return 202 status rather than restarting the model. After a process crash, the original same ID may resume speakers missing from durable history, but a provider request that died after inference may have been billed without a persisted reply; never claim exactly-once external billing.

HTTP 502 provenance must not be invented:
- A structured application 502 with `providerStatus`/a durable `lastProviderFailure.http_status` means the provider adapter observed a model error.
- If the browser/edge returns 502 while `/state` continues showing progress and completed receipts, investigate frontend/proxy request-duration constraints.
- If the browser/edge returns 502 with no durable provider failure and no activity, inspect the proxy/server logs and connection lifecycle. A status code alone does not identify the origin.

## Fair comparison with other branches

Use the *same* 9 participants, selected provider/model and budget, role prompts, three owner messages, cache/TTL settings and routing. Do not hardcode a participant name as an @mention in the owner prompt unless single-speaker routing is intended. A plain unprefixed participant name does not narrow Sub4 discussion routing.

For each owner turn, wait for its durable response count (normally 9 distinct speaker replies) before sending the next. After 3 complete turns there must be 3 owner entries and 27 distinct agent messages (30 durable transcript entries), with 27 usage receipts. Record per-turn deltas and sums from `state.usage`, including:
- `promptTokens`, `completionTokens`, `cachedPromptTokens`, `uncachedPromptTokens`, `cacheWriteTokens`, `providerUsageReported`, `unmeteredCalls`
- `serverContinuationCalls`, `expiredContinuationRebases`, `fullPromptCalls` and `wirePromptChars` (wire *characters*, not billed tokens)
- `byAgent` per-model receipts; failures and partially completed turns must not enter a complete-turn ranking.

The complete historical discussion is durable, but stateless Chat Completions uploads selected history every time. OpenAI Responses `previous_response_id` can upload only the delta, while prior chain history remains input-billable. The prompt cache may reduce paid *uncached* tokens only when the actual provider receipt confirms cache reads. Compare actual `uncachedPromptTokens` and KRW costs rather than equating lower outbound request bytes or transcript length with savings.

## Relevant files

- `src/group.rs`: strict shared indexed context and durable speaker receipts; owner-turn lease.
- `src/frontend_rooms.rs`: synchronous legacy and new background POST, progress and failure classification.
- `web/src/pages/Rooms.tsx`: native UI POST uses background acceptance and existing SSE/10-second GET fallback.
- `src/frontend_rooms/fanout_tests.rs`: nine-speaker 3-turn, same-ID retries, full fanout despite a plain name mention, 400 interruptions and background dispatch.
- `.github/workflows/sub4-dev-rust.yml`: targeted tests on sub4 pushes.

**Verification status:** source change and test cases exist on Sub4. GitHub Actions results must be read for the final head commit. Live 9-speaker paid-model 3-turn measurements and proxy-level 502 reproduction have not been run as part of this code change.
