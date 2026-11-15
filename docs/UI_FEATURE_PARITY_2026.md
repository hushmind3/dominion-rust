# Dominion sub4 — latest main frontend baseline and native API gap

**As of 2026-11-15:** latest `main` HEAD is `879dc23dce309688691c200b001b180bead95a12`. Its `web/` Git tree `9e2be9b7376c1eefb6195e3ea7f9463b952456c0` is copied intact into `dominion-sub4`.

## Accurate distinction

The **full frontend source is matched**, including all main navigation and page implementations. The native sub4 Rust server is **not** the main server: the frontend expects `/api/*` but the sub4 backend currently serves `/api/v1/*`. A page whose API is missing will not fully function. We deliberately removed the sub4 substitute screens instead of displaying incomplete features as if complete.

| Original main page | Some original main service contracts | Sub4 native service(s) requiring true frontend compatibility |
| --- | --- | --- |
| Chat | `/api/sessions`, `/api/agents`, `/api/approvals` | `/api/v1/runs`, run events/approval; no full session/profile API |
| Models | `/api/providers`, `/api/model-library/runtime` | `/api/v1/config`, configured model provider; no full provider CRUD |
| Agents | `/api/agents`, `/api/personas/import` | `/api/v1/runs/{id}/subagents`; no profile CRUD |
| Rooms | `/api/rooms`, room session/control/knowledge | `/api/v1/groups`; different schema/capabilities |
| Video | `/api/video/*`, `/api/youtube/*` | `/api/v1/videos`; different operations |
| RSI | `/api/rsi/campaigns`, `/api/rsi/start` | `/api/v1/improvements`; different experiment UI/data |
| Memory | `/api/memories`, `/api/agents` | `/api/v1/memory`; distinct entry schema |
| Workflows / automations | `/api/workflows`, `/api/automations`, `/api/jobs` | No equivalent complete workflow/scheduler contract |
| Extensions / connections | `/api/mcp`, `/api/skills`, `/api/channels` | Rust skills/MCP execution, incomplete management API |
| Operations / developer | `/api/executions`, `/api/dev/*` | Rust run journal, tools; distinct operational contract |
| Profile | `/api/settings/owner-profile` | `/api/v1/profile`; not routed under legacy name |
| Development server | Proxy to `127.0.0.1:4317` | Sub4 Rust binds `127.0.0.1:8787`; no full route adaptation |

The correct follow-on work is to implement actual main API semantics **in Rust**, with durable state and proper security, while preserving original frontend source and behavior. Do not create fabricated lists, dummy buttons, synthetic success responses, or simplified page replacements.

See `docs/FRONTEND_ALIGNMENT.md` for exact Git object evidence. The frontend-only tree replacement was not compiled or tested; no CI was run at user direction.
