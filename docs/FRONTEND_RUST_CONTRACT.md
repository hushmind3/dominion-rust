# Dominion · Native Rust backend for latest-main frontend

**Target main revision:** `879dc23dce309688691c200b001b180bead95a12` (the entire `web/` tree at `9e2be9b7376c1eefb6195e3ea7f9463b952456c0`). The frontend source is not modified.

**Architecture:** Existing main UI → original `/api/*` request → `src/frontend_api.rs` → Rust `Engine` / `Store` / `ModelClient` / `Knowledge` / `FrontendStore`. Native `/api/v1/*` routes remain in parallel, using the **same** process and execution authority. This is a compatibility boundary, not the older main engine nor another agent SDK.

## Source-implemented original main endpoints

| Main frontend flow | Endpoint(s) | Actual backing and behavior |
| --- | --- | --- |
| Main shell | `GET /api/health`, `/api/status` | Native build metadata (unknown if unset), live run/agent/session counts |
| App events | `GET /api/events` | SSE from actual changes to native run snapshots; no fabricated status |
| Owner profile editor | `GET/POST /api/settings/owner-profile` | Rust durable profile, input validation and retained image data |
| Home + model selection | `GET /api/providers`, `/api/providers/dominion-native/models` | **Only** configured native Rust model, active status and sanitized base URL |
| Provider connection probe | `POST /api/providers/dominion-native/test` | Actual user-invoked provider chat-completion call (metered by provider, no pretend connection) |
| Agent editor | `GET/POST /api/agents`, `DELETE /api/agents/{id}`, `POST /api/agents/bulk-model` | Durable agent profiles; selected model must really match the active Rust model; profiles affect actual chat/job prompts |
| Chat history | `GET/POST /api/sessions`, `/sessions/{id}`, `/messages`, `/usage`, `/rename`, `/fork`, `/clear`, `/cancel` | Persisted messages and session lifecycle; active-run restrictions, forked independent IDs, native cancellation |
| Chat send | `POST /api/chat` | Native run creation, real tool receipts and approval events over SSE; the assistant result is only saved once the run completes. A Rust background watcher persists completion even if the browser disconnects |
| Tool permission | `GET/POST /api/policy`, `GET/POST /api/approvals` | Actual native pending approval and decision. Blanket approval escalation is rejected, preserving Rust operator policy |
| Home native agent | `GET/POST /api/sdk/runs`, `GET /api/sdk/runs/{id}`, `POST /approve`, `/resume`, `/cancel`, `/reconcile` | Durable native run records projected into the original main checkpoint schema, correct call ID approval and real observed tool reconciliation |
| Jobs/monitor | `GET/POST /api/jobs`, `/jobs/{id}`, `/jobs/{id}/cancel`, `GET /api/executions`, `/executions/{id}/operations`, `POST /executions/{id}/cancel` | Main execution UI draws from the real run journal and cost/usage counters; no artificial jobs |
| Tools | `GET /api/tools` | Real native tool/MCP catalog after configured deny rules |
| Memory | `GET/POST /api/memories`, `DELETE /api/memories/{id}` | Actual Knowledge notes under global or agent scope; no parallel memory DB |
| Local runtime and connections | `GET /api/model-library/runtime`, `/installed`, `/channels`, `/providers/cli` | Truthful absent/unmanaged state; no pretend local runtime, channel or CLI connection |

The actual `src/frontend_api.rs` and `src/frontend_state.rs` are the source of truth. The list above describes implemented code paths, **not a verified running product**; build/tests/CI have not been invoked at the user's request.

## Deliberately unimplemented, not faked

- Provider registration/edit/delete, additional models and model routing are not wired to a live mutable native provider registry. Write requests return HTTP 501. The native model is operator-configured.
- Disabling tools per chat, disabling non-weight learning per individual chat, and choosing unsupported thinking/reasoning settings return 501. Those controls never silently continue with settings contrary to the user's choice.
- Arbitrary optional verifier commands sent by the original main native-work panel are rejected rather than ignored.
- Original main RSI **campaign UI**, multi-person room management semantics, ComfyUI/video library management, YouTube publishing, workflow scheduling, automations, channels, OAuth, skill/MCP management, and developer desktop controls still require their real backend models and end-to-end implementations.
- Any unimplemented `/api/*` route emits a documented HTTP 501 JSON error, not HTML 200 or a synthetic success list. Frontend source is left intact; unsupported screens will surface explicit service errors.

## Security and durability

- Both `/api` and `/api/v1` use the same bearer / X-Dominion-Token auth middleware; an unauthenticated remote UI cannot bypass the native execution checks.
- Model credentials remain only in operator-owned server configuration. Provider read responses expose no API keys or URL query credentials.
- User-created agents and sessions live in `frontend-state.json` under `Settings.data_dir`. Writes hold a mutex, clone data before mutation, enforce 8 MiB limit, use unique temporary files + fsync + atomic rename. No browser-local replica of authoritative agent or chat data is introduced.
- Native runs alone decide tool execution, tool approval, budget, cancellation and recovery. The compatibility layer only creates and observes runs; it never infers fabricated output or simulates tool success.
- Concurrent turns in one session are rejected. A terminal run is reconciled into the session transcript after browser reconnect. Pending run and transcript linkage is durable.
- Streamed SSE may be incomplete if the HTTP connection breaks, but completed actual run answers persist in Rust. Current default frontend `EventSource` cannot send custom auth headers in remote token-protected environments; real authenticated SSE transport support requires later backend/browser integration, **not** disabling auth.
- The main development server scripts proxy `/api` to port **4317**, so Rust's default bind is 127.0.0.1:4317. The original `--home` and `--open` CLI flags are accepted without editing the frontend.

## Remaining engineering work

1. Native **provider/model registry** connected to actual `ModelClient` routing, provider authentication and validation, catalog, local runtime management.
2. Main **chat controls** for tool-free mode, per-turn model reasoning and strategy-learning controls, plus robust approval stream reconnect and explicit session compaction/learning.
3. Original **rooms, video, RSI** page-specific schemas and richer operator controls, backed by corresponding native Rust engines.
4. Workflow/automation, MCP/skills management, channels/OAuth, developer desktop and update flows.
5. Original frontend browser/desktop end-to-end verification, persisted old-main data migration and cleanup after full feature parity.

**No UI source changes. No fake implementation claims. No tests or CI executed in this stage.**

## 2026-11-15: Main Video page, real native rendering

Main's exact frontend sources are unchanged. Rust `src/frontend_video.rs` implements `GET /api/video/status`, `/config`, `/catalog`, `/outputs`, `/workflows`, `/models/downloads`, `/jobs`, `/jobs/{id}`, `/jobs/{id}/content`, and `POST /api/video/jobs`. Native jobs create a persisted `Videos` record and actually render MP4 through FFmpeg, with requested aspect ratio and exact 3–180 second duration; output metadata/URL are read from real completed files. The user's title is persisted with the video job. The native runtime checks FFmpeg/drawtext and available speech before reporting readiness. Legacy media download streams use the same authentication as native `/api/v1`.

The existing renderer makes **FFmpeg title-card and optional narration videos**, not synthesized photorealistic motion. ComfyUI and YouTube remain unconnected; no synthetic upload success or model download results are returned. Video configuration choices unsupported by the current engine reject instead of silently being ignored. Other unresolved original main features are still pending.

This stage is not compiled or tested by user instruction; the code needs normal runtime verification before claims of error-free production behavior.
