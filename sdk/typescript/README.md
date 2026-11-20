# Dominion TypeScript SDK

A typed HTTP/SSE client for Dominion's single **native Rust agent harness**. The client does not run a second agent or own an independent execution scheduler.

## Native work execution

```ts
import { Dominion } from '@dominion/sdk';
const sdk = new Dominion({baseUrl:'http://127.0.0.1:4317'});
const run = await sdk.runs.create({instruction:'Inspect the source, fix the bug, run tests and report evidence',budget_krw:3000});
for await (const event of sdk.events(run.id)) console.log(event.kind,event.message);
console.log((await sdk.runs.get(run.id)).status);
```

- `runs.create` or `runs.fromRepository`: invoke the same Rust work loop as Chat. Repository runs use isolated Git workspaces.
- `runs.instruct`, `runs.approve`, `runs.cancel`, `runs.resume`, `runs.reconcile`: user instructions, approval, cancellation and durable recovery.
- `runs.delegate`, `runs.subagents`: hierarchical budgeted worker delegation.
- `runs.files`, `runs.file`, `runs.skills`, `runs.skill`: inspect artifacts and load project instructions.
- `groups`: explicit agent collaboration.
- `memory.store/search/list/revise/forget`: durable, user-managed, non-weight notes.
- `strategies.search`: retrieve project-scoped verified evidence from earlier **explicitly opted-in** completed runs. No source auto-adoption.
- `frontend`: typed compatibility transport for the existing Chat, Profiles, Providers and Run views.

```ts
const coding = await sdk.runs.fromRepository({
  repository:'repos/my-project',
  instruction:'Fix the failing tests and inspect the Git diff',
  budget_krw:1200,
});
const reviewer = await sdk.runs.delegate(coding.id,{
  instruction:'Read-only investigation of potential regressions',
  budget_krw:200,read_only:true
});
await sdk.runs.instruct(coding.id,'Preserve the existing public API');
```

## Recovering uncertain side effects

The Rust journal records execution intent before mutating tools run. If a result cannot be established after an interruption, it **never** replays that mutation speculatively. Observe its actual effect and supply a receipt:

```ts
await sdk.runs.reconcile(run.id,{
  succeeded:true,
  result:'Verified the file exists with the expected bytes'
});
await sdk.runs.resume(run.id);
```

## Scope

The Rust harness owns planning, bounded context projection, permissions, MCP tool calls, parallel read-only inspections, delegation, limits, pause/resume, tool receipts and independent verification. Use connected tools and **MCP** for external capabilities. Historical local data and stored artifacts are not automatically deleted.

Automatic strategy learning is disabled by default. A user may explicitly save a note or opt into evidence recording, and the existing Run journal remains authoritative. SDK and frontend code do not perform hidden self-modification.

Requires Node 24.12+ or a modern browser with fetch and readable streaming responses.


## Event-driven SDK waiting

`await sdk.wait(run.id, {timeoutMs: 120000})` subscribes to the Rust Run event stream instead of polling a complete Run every second. The initial persisted sequence cursor prevents races between the first state read and subscription. The client returns when the run finishes, **requires approval**, or is **paused and no longer waiting for delegated children**; operators can then approve or resume explicitly. Timeouts and AbortSignal cancel the subscription. Existing `intervalMs` callers retain an explicit polling compatibility mode.

The Rust `/api/v1/runs/{id}/events` route now uses the same fsync-backed update notifications as the native SDK and chat stream. It no longer polls a full Run snapshot every 750 ms for each connected browser/client. Reconnect from a prior event sequence is still supported through `since`; a lagged receiver rereads durable state. This changes no wire format, run IDs, token accounting, or permissions.
