# @dominion/agent-sdk — native DOMINION Rust harness SDK

**First-party, dependency-free JavaScript SDK** (Node 20+ and modern browsers) for DOMINION's existing Rust/axum API. The agent loop, model/provider selection, tool dispatch, MCP, approvals, memory, durable jobs, goal planning/retries, worktrees and RSI remain **inside the DOMINION Rust engine**, not inside GitHub Copilot, Codex, Claude, or this client library.

This SDK neither adds an upstream assistant policy nor rewrites, classifies or rejects task descriptions. The user's text is forwarded to the configured DOMINION server verbatim. The server's own permissions, available tools, configured models, acceptance checks and approvals remain in effect. SDK clients cannot fabricate tool execution or verification receipts.

## Install

From a local monorepo checkout:

```sh
npm install ./sdk
```

If an app directly imports from the monorepo:

```js
import { DominionClient } from './sdk/index.mjs';
```

## JavaScript example: persistent session, real tool-capable stream

```js
import { DominionClient } from '@dominion/agent-sdk';

const dominion = new DominionClient({
  baseUrl: 'http://127.0.0.1:4317',
  token: process.env.DOMINION_TOKEN ?? '',
});

const session = await dominion.sessions.create({ agentId: 'general' });
const request = {
  sessionId: session.id,
  agentId: 'general',
  input: 'Read the workspace, implement the requested feature and verify it.',
  toolsEnabled: true,
  toolsExecutionEnabled: true,
  autoLearning: false,
};

for await (const event of dominion.chat.stream(request)) {
  if (event.type === 'text') process.stdout.write(event.text ?? '');
  if (event.type === 'approval') console.log('Needs approval:', event);
  if (event.type === 'error') console.error('Engine error:', event);
  if (event.type === 'done') console.log('Turn finished:', event);
}

// A later process can continue from the same Rust/SQLite session:
const priorMessages = await dominion.sessions.messages(session.id);
```

## Autonomous goal and persisted job

```js
const goal = await dominion.goals.create({
  goal: 'Implement my project goal, run checks and save the artifacts.',
  maxIterations: 16,
  isolateWorktree: true,
  successCriteria: [],
});

const job = await dominion.jobs.wait(goal.jobId, {
  intervalMs: 1000,
  maxPolls: 1200,
});
console.log(job.status, await dominion.goals.get(goal.id));
const proof = await dominion.goals.receipt(goal.id); // 404 until genuine receipt exists
const diff = await dominion.goals.diff(goal.id);
// The owner may explicitly apply a verified diff via goals.apply(id, sha256).
```

The SDK exposes: `sessions`, `chat`, `goals`, `jobs`, `approvals`, `agents`, `providers`, `tools`, `rooms`, and `rsi`.

- `chat.stream(...)` is a cancellable `AsyncGenerator` of original native Rust SSE events. It requires the terminal `done` event; a disconnected or malformed stream is never reported as a successful response.
- `chat.run(...)` gathers text, reasoning and original events; it raises an error for an engine `error` event.
- `sessions` preserves server-owned history, avoids model transcript duplication and supports cancellation, fork and compaction.
- `jobs.wait(...)` reads durable jobs instead of inventing progress, handles cancellation, and has bounded polling.
- `goals.receipt(...)`, `goals.diff(...)` and `goals.apply(...)` use the original verifier and explicit patch digest.
- `approvals.resolve(...)` exposes the Rust permission decision path; the SDK does not silently approve tools.
- Errors include `status`, `code`, `method`, `path`, and original structured server details.

The React chat production path in `web/src/api.ts::streamChat` now uses `DominionClient.chat.stream`. This is a **wired product dependency**, not an unused example package.

## Test and compatibility

```sh
node --test sdk/index.test.mjs
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
```

Tests use a mock Fetch transport for the Rust HTTP/SSE wire contract. These are not proof of live-provider task completion, nor do they imply any unmeasured model improvement or billing savings.

### Design references, 2026

- [GitHub Copilot SDK architecture](https://github.com/github/copilot-sdk) — Copilot hosts its *own* agent runtime over JSON-RPC/CLI; do not embed it as DOMINION's authority.
- [OpenAI Agents SDK run/state/stream model](https://developers.openai.com/api/docs/guides/agents/running-agents) — separate the loop from the transport, preserve continuation state and typed streaming events.
- [MCP TypeScript SDK v2](https://github.com/modelcontextprotocol/typescript-sdk) — MCP is a tool transport, not a replacement for the product-level agent or user intent.


## Developer-defined tools: no Copilot/Codex agent runtime

The SDK includes `@dominion/agent-sdk/mcp`, a first-party local Node HTTP MCP tool server. Developers implement executable functions directly; DOMINION discovers and invokes them through its existing Rust MCP tool dispatch and approval layer.

```js
import { DominionClient } from '@dominion/agent-sdk';
import { DominionToolServer } from '@dominion/agent-sdk/mcp';

const sdk = new DominionClient({ baseUrl: 'http://127.0.0.1:4317' });
const token = process.env.DOMINION_MCP_TOKEN ?? '';
const tools = new DominionToolServer({ name: 'project-helpers', token });

tools.tool('calculate_area', {
  description: 'Calculate the area of a rectangle',
  inputSchema: {
    type: 'object',
    properties: { width: { type: 'number' }, height: { type: 'number' } },
    required: ['width', 'height'],
  },
}, ({ width, height }) => Number(width) * Number(height));

const url = await tools.listen();
await sdk.mcp.register({
  name: 'project-helpers',
  transport: 'http',
  url,
  apiKey: token, // Rust stores this in its encrypted vault, not the public record.
  enabled: true,
});
console.log('DOMINION can discover:', await sdk.mcp.discover());
// Keep this Node process alive while DOMINION uses the tools.
// On shutdown: await tools.close().
```

MCP extension traffic uses a local loopback endpoint by default, origin rejection,
optional bearer authentication, request-size limits and tool-result error receipts.
The server accepts the legacy `2024-11-05` initialize exchange used by the current
DOMINION Rust MCP client, and the stateless `2026-07-28` request metadata format.
No external CLI or third-party agent execution policy is installed. New tools may
take up to the Rust MCP client's 120-second discovery cache to appear.
Run `node --test sdk/index.test.mjs sdk/mcp-server.test.mjs`.

### Live owner steering

`await sdk.session(sessionId).steer('새 방향: ...')` submits the exact owner instruction to the native Rust durable step inbox. This does not spawn a competing chat run or import a third-party policy. If the current model turn has finished, the instruction remains queued for the next native turn.
