# DOMINION Sub2 — First-party SDK integration (2026-11-14)

## Problem solved

DOMINION already implements an agent loop in Rust, but external applications lacked a stable
client contract and extensible, developer-defined tools. Embedding GitHub Copilot SDK
would place a second agent planner, policy and memory layer above DOMINION.
This change instead exposes DOMINION's own engine to other applications via a small,
typed library and an independently implemented MCP tool server.

## Executable entry points

| Entry | Actual production path | Durability / authority |
| --- | --- | --- |
| `DominionClient.chat.stream` | `sdk/index.mjs` → `POST /api/chat` → `src/api/chat.rs::chat` → `engine::chat::run` | Rust owns tools, model, agent, SQLite transcript and approvals |
| `DominionClient.session` | `GET /api/sessions/:id/messages` → `api/sessions.rs` | Reuses original server-owned session; avoids replay duplication |
| `DominionClient.jobs` | `/api/jobs` → `api/jobs.rs` → `engine::jobs` | Durable status, cancellation and result in persisted job |
| `DominionClient.goals` | `/api/goals` → `api/goals.rs` → `engine::goals` / `goal_workspace` | Frozen verifier, explicit patch digest, resume/steering remain server-side |
| `DominionClient.approvals` | `POST /api/approvals` → `api/approvals.rs` | Owner-controlled permissions; SDK never auto-approves |
| `DominionClient.mcp.register` | `POST /api/mcp` → `api/collections.rs` → `engine::mcp` | Encrypted MCP credential + runtime catalog, 120s discovery cache |
| `DominionToolServer.tool` | `sdk/mcp-server.mjs` HTTP JSON-RPC → `engine::mcp::call` → Rust chat tool dispatch | User-provided JS handler executes; Rust still owns tool approvals |
| React chat | `web/src/pages/Chat.tsx` → `web/src/api.ts::streamChat` → SDK | SDK now sits in real product UI flow |

The SDK handles HTTP errors, cancellation, status polling, fragmented UTF-8 SSE, JSON frames
and terminal `done` receipts without introducing another agent model call. User task input is
forwarded without any SDK-level classifier or refusal policy. Server-configuration and provider
constraints are unchanged.

## MCP protocol compatibility and independent architecture

2026-07-28 MCP is stateless, each request is an HTTP POST carrying protocol metadata.
DOMINION's current Rust MCP HTTP transport still uses the 2024-11-05 initialization exchange,
so the first-party tool server accepts **both** Rust's current legacy handshake and
2026-07-28 request metadata. Newer transport features such as MRTR/subscriptions/listen and
automatic remote MCP version negotiation remain outside this implementation.
This is deliberate, not claimed full protocol conformance.

The local server binds 127.0.0.1 by default. Cross-origin browser calls are rejected, bearer
authentication is available, non-loopback binding requires a token, and JSON body size is limited.
SDK tool callbacks do not bypass Rust's existing approval decisions.

## Evidence and verification

- `node --test sdk/index.test.mjs`: 7 local mock-transport tests passed; validates SSE, error,
  stored job polling, approvals and literal input transfer.
- `node --test sdk/mcp-server.test.mjs`: 3 local real HTTP tests passed; validates legacy
  initialization, real callback invocation, 2026 metadata and origin/bearer rejection.
- `.github/workflows/ci.yml`: runs both SDK suites plus `web` TypeScript/build and Rust tests.
- GitHub Actions `43597073734` confirmed SDK suites, web typecheck and build success while
  Rust tests were still running at observation time. See Actions for final status.

These tests validate SDK correctness and wire interoperability. Live-provider task success,
real external app installation, third-party MCP server interoperability and model quality
changes have not been established by the mock test alone.

## 2026 research

- GitHub Copilot SDK (GA Jun 2 2026):
  https://github.blog/changelog/2026-06-02-copilot-sdk-is-now-generally-available/
- GitHub Copilot SDK architecture (runtime over JSON-RPC):
  https://github.com/github/copilot-sdk
- OpenAI Agents SDK state/run/stream semantics:
  https://developers.openai.com/api/docs/guides/agents/running-agents
- MCP 2026-07-28 spec (stateless HTTP transport, headers, origin protection):
  https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx

## Future safe extensions

- Add Rust MCP modern stateless client negotiation with **no unsafe replay of side-effecting tools**.
- Build a live Rust HTTP + SDK + custom MCP callback E2E proof in a local runner.
- Extend SDK with typed run execution traces, token usage and queued job event subscriptions.
- Publish a versioned package only after release-compatible integration tests.
