# Dominion TypeScript SDK

A thin, typed client to the **same Rust execution engine** used by Dominion Studio.

```ts
import { Dominion } from '@dominion/sdk';
const sdk = new Dominion({baseUrl:'http://127.0.0.1:8787'});
const run = await sdk.runs.create({instruction:'Create a landing page', budget_krw:3000});
for await (const event of sdk.events(run.id)) console.log(event.kind, event.message);
const finished = await sdk.runs.get(run.id);
console.log(finished.status, finished.answer);
```

- `sdk.runs.approve(id, allow)` resolves a persisted tool approval.
- `sdk.runs.resume(id)` restarts an interrupted task without recreating it.
- `sdk.runs.files(id)` and `sdk.runs.file(id, path)` retrieve generated artifacts.
- `sdk.runs.instruct(id, instruction)` injects a mid-task instruction.
- `sdk.wait(id)` waits for a terminal execution state with cancellation/timeout support.

Node 20+ and modern browsers with `fetch`/streaming support. `npm run build` compiles the SDK with TypeScript.
