# Dominion Agent SDK (TypeScript)

## Native Rust self-improvement campaign

The TypeScript SDK is an external program's integration client, not
another vendor's agent execution engine. Creation is a single explicit POST;
monitoring is read-only GET polling; commit adoption requires a separate
explicit call to apply and remains verified by the Rust server.

```ts
import {DominionClient} from "@hushmind3/dominion-sdk";
const sdk = new DominionClient({baseUrl:"http://127.0.0.1:4317"});
const job = await sdk.rsi.start({
  request:"Improve tool recovery using verified failures",
  providerId:"configured-provider",model:"configured-model",
  selectionPolicy:"manual",maxAttempts:2,tokenBudget:10_000,
});
const campaign = await sdk.rsi.wait(job.id);
console.log(campaign.status, campaign.recursiveProof);
// Optional after explicit operator confirmation:
// await sdk.rsi.apply(job.id, campaign.headCommit);
```

Use top-level maxAttempts, tokenBudget and timeBudgetSeconds. A configured
provider/model and a budget appropriate to the allowed spend are required.


**Own SDK. Own Rust engine.** No vendor agent subprocess, policy, or SDK dependency.
The package is a typed client for Dominion's native Axum API, not a copy of
Codex/Copilot/Claude and not an HTTP proxy for their runtimes.

## Build and test

```bash
cd sdk/typescript
npm install --no-audit --no-fund
npm test
```

This first-party package is stored in the repository and **not yet published** to npm.
For a local application:

```bash
npm install /absolute/path/to/dominion-rust/sdk/typescript
```

Start the native Dominion service on `http://127.0.0.1:4317`.

## Autonomous goals

```ts
import { DominionClient } from "@hushmind3/dominion-sdk";

const client = new DominionClient({
  baseUrl: "http://127.0.0.1:4317",
  adminToken: process.env.DOMINION_ADMIN_TOKEN, // server-side only
});

const goal = await client.goals.create({
  goal: "Implement an API endpoint, test it, and deliver the patch",
  executor: { providerId: "provider-id", model: "model-id" },
  maxIterations: 16,
  isolateWorktree: true,
});

const finished = await client.goals.wait(goal.id, {
  timeoutMs: 600_000,
  onUpdate: state => console.log(state.phase, state.status),
});

if (finished.status === "completed") console.log(finished.evidence);
if (finished.status === "needs_review") console.log(finished.gaps);
```

`goals.create` saves an engine-owned goal and matching durable job in the same
transaction. `goals.wait` only polls, does not start new model turns.
`goals.steer(id, plan)` applies an owner's instruction on the next safe iteration.
`goals.cancel(id)` cancels the durable worker. `goals.resume(id, true)` resumes
a failed or review-pending goal after the owner acknowledges uncertain external effects;
it does not replay an unconfirmed side effect without that explicit decision.

## Durable conversation threads

```ts
const thread = await client.startThread({title: "Maintenance", providerId: "p", model: "m"});
const result = await thread.run("Find the regression and fix it");
console.log(result.finalResponse);

const resumed = client.resumeThread(thread.id, {providerId: "p", model: "m"});
for await (const event of resumed.runStreamed("Continue and verify")) {
  console.log(event.type);
}
```

`thread.run` reads the already-persisted messages before and after the streaming
turn to select the **new** answer. A stream ending without the server's `done`
event raises `DominionStreamInterrupted`; the SDK **never retries the POST**,
because tools could have caused real side effects.

## Resume live event monitoring

```ts
const stop = new AbortController();
for await (const event of client.watchEvents({
  after: lastPersistedSequence,
  signal: stop.signal,
})) {
  console.log(event.seq, event.type);
  // Save event.seq in YOUR storage when a side effect has been handled.
}
```

The server's `GET /api/events?after=<seq>` stores replayable events and sends
a monotonically increasing sequence. The SDK reopens the **read-only GET**
with the last sequence and drops duplicate events, but does not persist
your cursor; application code must do so. The server replays up to 500
historical events per connection, so very slow consumers must reconcile
current resource state after larger gaps.

## Surface

- `goals`: create/list/get/cancel/steer/resume/changes/wait
- `jobs`: durable prompt worker/list/get/cancel/wait
- `sessions`: create/list/get/messages/fork/cancel
- `executions`: list/get/operations/cancel
- `approvals`: pending/resolve
- `catalog`: providers/agents/tools/capabilities
- `streamChat`, `startThread`, `resumeThread`, `watchEvents`

Transport failures, malformed SSE, and HTTP errors are distinct typed errors.
All requests target the current Rust API routes. No extra refusal policy is
inserted. Existing engine tools, session permissions and user approval behavior
still apply.

Node process hosting is implemented. ACP and custom TypeScript callback MCP tooling remain separate enhancements.

## Development evidence

`npm test` uses an in-process mock HTTP/SSE transport and does **not**
claim a live provider execution, real workspace patch, or production recovery
measurement. See `tests/sdk.test.mjs`. Live acceptance is separate.

## Node runtime ownership (native Rust engine)

The `@hushmind3/dominion-sdk/node` entrypoint can spawn **DOMINION's own Rust executable** and validate its instance ID, PID, workspace and SQLite state before exposing a typed API client. The browser entrypoint never spawns processes.

```js
import {DominionRuntime} from '@hushmind3/dominion-sdk/node';
const host=await DominionRuntime.spawn({binary:'./target/release/dominion-rust',workspace:'./repository',dataDir:'./dominion-state'});
const goal=await host.client.goals.create({goal:'Implement, run tests, and verify'});
await host.shutdown();
```

`goals.appendInstruction(goalId,text,requestId)` and `goals.replacePlan(goalId,text,requestId)` send **durable owner-authored instructions** to `/api/goals/:id/steer`; keep the same `requestId` if retrying an ambiguous network failure. Native subprocess integration tests run when `DOMINION_RUST_BINARY` is supplied. Node-only tests do not claim genuine model inference.
