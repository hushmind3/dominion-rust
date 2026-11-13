# DOMINION Sub2 release integration — 2026-11-14

Branch: `dominion-sub2` · Evaluation basis: **entire engine–harness–algorithm call graph**, not source volume, documentation, or superficial feature registration.

## 1. Deployed source boundaries

| Capability | Entry point and actual execution | Ownership / durable evidence |
|---|---|---|
| Video workflow | `api/video.rs::create_job` → `engine/video.rs::enqueue` → `engine/jobs.rs` video dispatcher → `engine/video.rs::run` → `video/ltx.rs::render` | DB job, deterministic frozen input, remote ID + in-flight checkpoint, MP4 saved in per-job data directory |
| Video alternatives | `video.rs::run` → `video/native.rs::native` / `video/comfy.rs::comfy` | Rust title cards + FFmpeg or explicit local ComfyUI workflow, respectively |
| Video upload | `api/video.rs::upload` → `video/upload.rs::enqueue_upload/upload` | Separate YouTube OAuth and resumable upload state; no implicit publication |
| Room routing | `api/rooms.rs::validated_config` → persisted owner policy → `engine/rooms.rs::run_locked` → `rooms/model_tiering.rs::select` → `chat::runtime` | Native provider/model choice per actual speaker message, selection in `outputs[].modelRoute` |
| Room contextual intelligence | `chat/room_context.rs::compile` → `room_context/role_routing.rs` + existing `room_state`, `room_knowledge`, `memory` | Query-relevant provenance, original owner authority, mandatory self/peer history, per-turn estimated usage |
| Provider caching | `chat/continuity.rs::prepare/project` → `chat/provider.rs` streaming → `chat/continuity.rs::send` | Stable participant prefix and provider-specific cache hints; actual cache-hit usage reported by provider |
| Recursive improvement | `api/rsi.rs::start` → `jobs.rs` `rsi_evolve` → `rsi/execution.rs` → compiled child planner/worker → `rsi_worktree::compare_at` → `rsi/lineage.rs::adopt` → next descendant | Original frozen paired receipts, candidate patch fingerprints and source-revalidated inherited method outcomes |

### Video: turnkey generative route

- Cloud generation is deliberately **BYOK**, not inference on a user's low-VRAM machine. API keys are sealed by `AppState::save_secret("video:api")` in `api/video.rs::save_config`. Public config only returns `hasVideoKey`.
- `video/ltx.rs` sends `POST https://api.ltx.io/v2/text-to-video`, stores the returned ID in the original durable job and polls `GET .../{id}`. An ambiguous submission/network failure enters a `needs_review` state rather than blindly re-submitting a billable job.
- Explicit model+resolution+duration matrix: 2.5 Pro 6–10 s; Fast 6–20 s at 720p/1080p, 6–10 s at 1440p/4K. Actual model output is MP4 with generated audio, not a subtitle-card fallback. 24 fps; 9:16, 16:9, 1:1.
- Generated MP4 is downloaded from HTTPS provider-backed domains without following redirects, capped to 256 MiB, checked for MP4 header, then atomically renamed. User remains in control of any later YouTube upload.
- **Limit:** no fully automatic episode-length stitching, video QA judge or continuity scoring yet. A single request can produce multiple native LTX shots only within the supported clip length. Source wiring ≠ measured output quality.

### Multi-agent cost and quality boundary

- `rooms/model_tiering.rs` is an opt-in, deterministic, no-extra-model-call selector based on the owner's **current request**. The user registers a `light` and a `deep` route, backed by their existing registered providers. Unknown/disabled provider IDs and invalid reasoning settings fail at room configuration save. An explicit per-job model bypasses the automatic tier selection.
- Existing room `model_pool` and `continuity::prepare/project/send` already provide stable participant routing, cache affinity, and provider-specific cache hint negotiation. Preserve those rather than installing a redundant ad hoc cache or forcing a tiny local model for summarization.
- `room_context/role_routing.rs` only re-ranks **otherwise query-relevant** evidence by bounded participant role overlap. It never converts peer claims into owner facts and never displaces required owner state, actual tool/result pairing or participant continuity. It records `selectionPolicy`, `roleRelevantItems` and `prunedCandidateTokenEstimate`; those are estimates, **not measured money savings**.
- Semantic response caching was **not** added: live world state, multi-user/role separation, and durable conversation provenance require a reliable answer-validity contract before bypassing an LLM.

## 2. Recursive whole-system architecture inclusion

`rsi_system.rs::source_inventory` inventories compiled engine, API, frontend and worker sources. Its `source_role` now classifies `engine/video.rs`, `engine/video/*`, `api/video.rs` as `video`, and `engine/rooms.rs`, `engine/rooms/*`, `api/rooms.rs`, `room_state.rs`, `room_knowledge.rs`, `chat/room_context.rs`, `chat/room_context/*` as `multiagent`. `architecture_role_domain` maps these to the existing valid evaluation domains `tools` / `context`.

`rsi_system/blueprint.rs::blueprint` and `rsi_system.rs::architecture_frontier` thus offer **real executable source files** as the descendant's candidate improvement targets. No independent evaluator, immutable worktree comparator, policy/budget gate or frozen suite is put into the model-editable set by this change. A source target appearing in the frontier is only permission to propose/edit, not a reward, performance proof or adopted descendant.

### Stability: production vs regression source

- Video was split from one 746-line mixed-purpose module into `video.rs` coordinator plus `native.rs`/ `comfy.rs`/ `ltx.rs`/ `upload.rs`/ `tests.rs`.
- Test code was extracted from `rooms.rs`, `chat/room_context.rs`, `room_knowledge.rs`, `chat/provider.rs`, `models.rs`, `algorithms.rs`, `chat/context.rs` and `room_state.rs` without changing their invocation entry points. All Rust regression files are referenced with `#[cfg(test)]` only.
- The high-complexity `models.rs`, `algorithms.rs`, `chat/provider.rs` still contain mixed concerns. Their remaining decomposition is a structural priority; do not claim one edit has removed all large-module bottlenecks.

## 3. 2026 reference and independent claims

- **LTX-2.5** official provider model matrix (updated in 2026): https://docs.ltx.io/models/ltx-2-5/
- **LTX V2 video API + asynchronous polling:** https://docs.ltx.io/api-documentation/api-reference/async-video-generation/submit-text-to-video and https://docs.ltx.io/async-jobs
- **LTX migration**, old synchronous API retired Oct 2026: https://docs.ltx.io/migrate-v1-to-v2
- **OpenAI prompt caching**, source currently describes prefix reuse and discounted cached tokens; actual rate varies by model and usage: https://developers.openai.com/api/docs/guides/prompt-caching
- **HyperAgents** (Meta, Mar 2026) underpins recursively editable experiment methods: https://ai.meta.com/research/publications/hyperagents/
- **MetaSkill-Evolve** (Jul 2026) supports distinct adaptation timescales without model-weight changes: https://arxiv.org/abs/2607.05297

### Remaining required proof and upgrades

1. No build, compile, runtime test, benchmark, CI or paid-provider video render was executed for these source changes. Any individual API feature may require future adjustment to provider behavior.
2. Measure **provider-reported cached tokens, uncached tokens, repeated room context tokens, output quality, latency and cost by provider**, with an equal-task baseline. Avoid treating a theoretical 40–90% discount as achieved.
3. Implement long-form shot orchestration, continuity controls and native video QA with independent scene receipts before claiming creator-grade turnkey long-form production.
4. Continue production module responsibility splitting for `models.rs`, `algorithms.rs`, `chat/provider.rs`; remove duplicated fallback and source-inventory coupling.
5. Prove the complete RSI loop: independently tested adopted source → new compiled descendant → another domain's measured improvement → return feedback → revised method → later gain, with matched frozen baseline and actual cost accounting.

### Non-negotiable constraints

Fixed base-model weights. No LoRA, fine-tuning, retraining, or source path that attaches weight learning to RSI. User hardware is not required for heavyweight local inference. Immutable evaluation/frozen cases remain outside candidate-edit authority.
