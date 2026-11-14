# DOMINION Native Agent SDK (TypeScript)

A **first-party client** for the existing DOMINION Rust engine. It does not use the Copilot, Claude Code, or Codex agent runtime. The client sends commands to the native DOMINION service, which owns models, tools, execution, approvals, evidence and persistent state.

## Example

```ts
import {DominionSDK} from '@dominion/native-agent-sdk'

const sdk = new DominionSDK({
  baseUrl: 'http://127.0.0.1:4317',
  token: process.env.DOMINION_TOKEN,
})
const thread = await sdk.startThread({title: 'Refactor', agentId: 'optional-agent-id'})
const result = await thread.run({
  input: 'Inspect the repo, make the requested code change, and verify it',
  toolsEnabled: true,
}, {
  onEvent: event => console.log(event.type),
})
console.log(result.finalResponse)

// Continue on the same Rust-owned persisted session after a restart:
const resumed = sdk.resumeThread(thread.id)
for await (const event of resumed.runStreamed('Continue and report actual evidence')) {
  console.log(event)
}

// Real durable goal/job (can survive UI disconnection):
const goal = await sdk.startGoal({goal:'Fix failing tests', isolateWorktree:true})
console.log(await goal.status())
```

## Ownership and API contract

- `startThread()` creates a real `/api/sessions` record; `resumeThread(id)` reconnects to that ID, not a new model conversation.
- `runStreamed()` consumes the existing `POST /api/chat` SSE event protocol; supports split UTF-8, CRLF, heartbeat frames, cancel and structured events.
- `run()` collects events, stops on engine errors and reads the persisted final assistant message by `messageId`.
- `startGoal()` → `/api/goals` creates a **durable native Rust job**; `resumeGoal()` reconnects to an existing goal. Retrying an interrupted write is **not automatic**.
- `sdk.tools()` lists installed native/MCP tools. `sdk.approvals()` and `sdk.resolveApproval()` use the existing user approval endpoints, with no new SDK refusal layer.
- `sdk.watchEvents({after: lastSeq})` reconnects to durable job/approval/event updates through `/api/events?after=...` and retains source `seq` values. It does not auto-retry a potentially side-effecting turn.
- `sdk.setToken()` uses the existing `X-Dominion-Token` header. Never embed production tokens into distributed browser code.
- `DominionThread.cancel()`, `fork()`, and `DominionGoal.status()/resume()/cancel()/changes()/applyChanges()/discardChanges()` map to real existing Rust API routes.
- The SDK does not execute tools, alter model system instructions or replace DOMINION's independent validation/recovery engine.

## Development

```sh
cd sdk/typescript
npm install
npm run check
```

The existing React UI directly uses this SDK's `streamChat` parser via `web/src/api.ts`, so SDK streaming is exercised in the regular product path.

Validation levels: standalone source and tests are committed; full Rust+React integration tests and real-provider completion measurements remain separate.
