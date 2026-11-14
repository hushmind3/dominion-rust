# Sub2 rejudge follow-up — cinematic films, goal workspace, owner memory safety
**Date:** 2026-11-14 · **Branch:** `dominion-sub2` · **Judge baseline:** 68/100 on `7fbc8a9`

This describes code paths and measurable gates, **not** a new awarded score or a claimed real paid production film. The judge's video bottleneck was 5/15 (vs 9/15 for Sub1/Sub4) and agent discoverability 19/25. Runtime costs and semantic visual continuity remain unmeasured without real clips and provider invoices.

## Product navigation
`web/src/components/WorkspaceShell.tsx` now offers **15 sidebar entries**, including `goals` / **자율 목표**. `web/src/pages/Goals.tsx` lazily mounts the existing `GoalConsole` — no separate implementation of agent state. The same backend handles: `POST /api/goals` (goal creation), `POST /api/goals/:id/plan` (owner's plan corrections), `POST /api/jobs/:id/cancel` (stop), `POST /api/goals/:id/resume` (acknowledged recovery), `GET /api/goals/:id/diff` (SHA review), `POST /api/goals/:id/apply` (pre-checked Git patch). The homepage still shows the console, and its shortcut enters this dedicated page. See [complete menu and route mapping](SUB2-NAVIGATION-AND-NOTIFICATIONS-2026-11-14.md).

## LTX V2: explicit and inspectable multi-shot paid work
```
web/pages/Video.tsx
   POST /api/video/jobs {engine:"ltx",ltxFilm:true,confirmPaidMultiShot:true,technicalGate:true}
   -> api/video.rs::create_job
   -> engine/video.rs::enqueue
       -> film::spec  [2..6 shots; model-specific bounded clips; ffmpeg prerequisite]
       -> durable job "render.film.clips" frozen before charging
   -> engine/video.rs::run -> film::run
       -> clip-NN/output.mp4 + receipt.json [SHA-256, technical status]
       -> LTX V2 async POST, remote ID checkpoint, status poll, bounded MP4 download
       -> quality::inspect ffprobe, receipt persisted with fsync and rename
       -> ffmpeg concat -> final output.mp4 -> final ffprobe quality
   -> GET /api/video/jobs/:id, GET /api/video/jobs/:id/content
```

A film request is rejected if multiple paid clip calls were not explicitly acknowledged. A completed clip with a valid SHA receipt is reused rather than repurchased after restart/retake. Malformed, mutated or missing receipts become `needs_review` and cannot silently trigger a new paid request. Unknown status of an in-flight provider request is checked from persisted job state **before** the next scene checkpoint, never cleared as part of UI progress. It cannot be replayed blindly.

An owner may inspect `GET /api/video/jobs/:id/scenes` for clip metadata, play an individual scene through `GET /api/video/jobs/:id/scenes/:index/content` (UUID-owned dir, bounds 0..5, regular file, canonical containment, ServeFile), then submit `POST /api/video/jobs/:id/retake`. The retake request includes the source SHA-256, exact number of expected new paid calls and a separate confirmation flag. The new job ID is deterministic from the retake identity, so the same approval does not create unlimited duplicate jobs. Verified other scenes are reused with their receipts; the retake is a new job, not an unbounded provider retry.

### Real cost boundary
- **Maximum planned generation orders:** 6 per new film (2–6 clips depending on the model/resolution/duration), with owner consent for each retake's estimated count.
- **GPU requirement:** no local model or dedicated GPU for hosted LTX; FFmpeg + FFprobe needed for stitching and strict technical QA.
- **Scope of evidence:** ffprobe checks stream existence, aspect, timing, dimensions, audio metadata. It does **not** measure cinematic artistry, character consistency, or viewer satisfaction. Any provider pricing is not hardcoded here, and no actual KRW charge is claimed without receipts.
- Audio is requested for each paid LTX clip; ffmpeg stream copy might fail when provider-generated clips differ in stream parameters. In that case preserve paid clips, return an explicit composition error rather than repurchasing clips.

## Local ComfyUI cinematic option
```
Video.tsx: ComfyUI + "로컬 ComfyUI 멀티샷 영화"
   -> video.rs::enqueue [cinematic=true]
   -> cinematic::preflight [workflow {{prompt}} & {{duration}} templates,
                            FFmpeg, FFprobe, bounded storyboard]
   -> cinematic::render [up to 15 shots / up to 120 seconds]
       -> fixed visual continuity bible + per-shot action + camera motion + seed
       -> comfy::comfy() existing native HTTP /prompt /history /view transport
       -> ffprobe technical check
       -> SHA-256 receipt for safe reuse, reject previous unknown inFlight
       -> ffmpeg concat+scale/pad 1080p/libx264 CRF20/AAC final.mp4
       -> quality::inspect final receipt
   -> normal video job result: shotResults + finalQuality
```
This option avoids paid LTX calls **when the configured ComfyUI workflow runs locally**. It still consumes local GPU/CPU time and may depend on externally hosted ComfyUI or models; compute, quality, and real wall-clock cost have not been measured. There is no unsupported guarantee that ComfyUI produces native audio, image-to-video interpolation, or identity consistency. Strict template validation prevents running identical static prompts across many shots. The UI explicitly sets zero automatic bad-shot retries.

## Owner-corrected memory provenance
```
Memory.tsx (edit only active source=user with no evidence quote or source ids)
  -> POST /api/memories/:id/edit {text, expectedText}
  -> collections.rs::action checks source/status/evidence and text limits
  -> db.rs::Store::patch_if_fields under SQLite lock
     CAS text, source, status, scope, agentId, partitionKey,
         verification, evidenceQuote, sourceMessageIds
  -> source=user, original scope preserved; verification="owner_revision"
  -> bus record + store audit; stale concurrent edits HTTP 409;
     learned/archived/quoted evidence HTTP 403
```
User authoring and source-grounded machine learning are intentionally different trust classes. A learned memory cannot be rewritten in place to falsely attribute a paraphrase to the original speaker. It must be corrected in the learning/provenance workflow rather than through inline edit.

## Test levels and limits
- GitHub Actions runs `npm run typecheck --prefix web`, `npm run build --prefix web`, and `cargo test --release` (see [CI branch history](https://github.com/hushmind3/dominion-rust/actions?query=branch%3Adominion-sub2)).
- The film port's `97c98959` run passed full workflow, including Rust film unit tests (segmentation, SHA tampering, paid retake count) and existing tests. Newer local ComfyUI, page, memory protection, and scene preview commits require their own CI result.
- No external video model, commercial API bill, actual 120-second output, graphical browser click-through, semantic continuity scoring or persistent 3-generation RSI has been run as part of this commit series.

## Upstream implementation references
- [LTX official 2.5 video API specification](https://docs.ltx.io/models/ltx-2-5/)
- [LTX API changelog](https://docs.ltx.io/api-changelog/)
- [ComfyUI official openAPI source](https://github.com/Comfy-Org/ComfyUI/blob/master/openapi.yaml)
- Adapted existing repository implementations on `dominion-sub1` (`engine/video/film.rs`, `web/pages/video/FilmRetake.tsx`) and `dominion-sub4` (`engine/video/cinematic.rs`), keeping Sub2 LTX V2 transport, SHA check and local ComfyUI renderer.
