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
- `video/ltx.rs::cinematic_prompt` composes optional cinematic/product/social guidance while preserving the user scene and checking the complete 5,000-character provider budget; an explicit `original` mode sends the user's prompt unchanged. This avoids adding an auxiliary model call. It is **prompt packaging**, not independently proven visual improvement.
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
- `models.rs` shrank from 2,547 production-without-test lines to ~413 lines, with `models/{installer,management,loader,catalog,download}.rs` owning the executable work. `algorithms.rs` shrank from 1,792 to ~339, with `algorithms/{builtins,registry,feedback,management}.rs`. `chat/provider.rs` shrank from 1,828 to ~588, with `chat/provider/{openai,adapters,codex,anthropic}.rs`.
- Original top-level function inventory was statically compared: 75/75 model declarations, 28/28 algorithm declarations, 34/34 provider declarations are present in the new source tree. This is not a typecheck or runtime test.
- Test code was extracted from `rooms.rs`, `chat/room_context.rs`, `room_knowledge.rs`, `chat/provider.rs`, `models.rs`, `algorithms.rs`, `chat/context.rs` and `room_state.rs` without changing their invocation entry points. All Rust regression files are referenced with `#[cfg(test)]` only.
- Provider-reported usage is stored per room speaker in `rooms.rs::run_locked`. `rooms/usage_ledger.rs::summary` persists aggregate input, output, actual cached tokens and unknown-report counts in `finish` and its audit event. The React room header shows cache reuse **for only provider-reported calls**. Neither an estimated token count nor a missing cache field may be labeled as a measured saving.
- The long `chat/runtime/turn.rs` single-turn function, parts of `rooms.rs` and other legacy mixed-purpose source still require decomposition. Mixed concerns across these paths are an unfinished structural risk.

## 3. 2026 reference and independent claims

- **LTX-2.5** official provider model matrix (updated in 2026): https://docs.ltx.io/models/ltx-2-5/
- **LTX V2 video API + asynchronous polling:** https://docs.ltx.io/api-documentation/api-reference/async-video-generation/submit-text-to-video and https://docs.ltx.io/async-jobs
- **LTX migration**, old synchronous API retired Oct 2026: https://docs.ltx.io/migrate-v1-to-v2
- **OpenAI prompt caching**, source currently describes prefix reuse and discounted cached tokens; actual rate varies by model and usage: https://developers.openai.com/api/docs/guides/prompt-caching
- **HyperAgents** (Meta, Mar 2026) underpins recursively editable experiment methods: https://ai.meta.com/research/publications/hyperagents/
- **MetaSkill-Evolve** (Jul 2026) supports distinct adaptation timescales without model-weight changes: https://arxiv.org/abs/2607.05297

### Remaining required proof and upgrades

1. No build, compile, runtime test, benchmark, CI or paid-provider video render was executed for these source changes. Any individual API feature may require future adjustment to provider behavior.
2. Provider-reported cached tokens and per-speaker usage are now captured in `rooms/usage_ledger.rs` and `web/src/pages/Rooms.tsx`; next independently measure **identical-task uncached baselines, response quality, latency and monetary costs by provider**. Never treat a theoretical 40–90% discount as achieved.
3. Implement long-form shot orchestration, continuity controls and native video QA with independent scene receipts before claiming creator-grade turnkey long-form production.
4. Continue production responsibility splitting for `chat/runtime/turn.rs` and the room execution loop, and deduplicate expensive provider fallback logic. The reorganized model/algorithm/provider modules preserve original entry points but still require full compilation and real-run validation.
5. Prove the complete RSI loop: independently tested adopted source → new compiled descendant → another domain's measured improvement → return feedback → revised method → later gain, with matched frozen baseline and actual cost accounting.

### 2026-11-14: Autonomous agent harness integration

The previous generic `prompt` job on Home now coexists with a dedicated `GoalConsole` that sends owner goals to `api/goals.rs` and its **durable `goal` job**, including persistent code-index reconnaissance, grounded planning/repair, deterministic acceptance, reviewer only after deterministic checks, scoped live approval requests and delegated worker statuses, owner-approved recovery of uncertain side effects, tracked artifact hashes, last assistant execution report and scoped actual Git diff. The whole chain is documented in [Agent Harness Sub2](AGENT-HARNESS-SUB2-2026-11-14.md).

`rsi_system/agent_feedback.rs::observe` reads last 64 real, durable normal goal attempt receipts and injects bounded `ownerTaskFeedback` into the compiled descendant `agent_harness` source frontier. This helps select future edits based on observed user tasks without fabricating frozen paired evaluation success. No CI/compile/live model/paid video work was run.

### Non-negotiable constraints

Fixed base-model weights. No LoRA, fine-tuning, retraining, or source path that attaches weight learning to RSI. User hardware is not required for heavyweight local inference. Immutable evaluation/frozen cases remain outside candidate-edit authority.
