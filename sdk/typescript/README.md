# Dominion TypeScript SDK

A typed HTTP/SSE client for Dominion's single **native Rust agent harness**. The client does not run a second agent, ship a video renderer or include an independent RSI self-modification system.

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
- `strategies.search`: retrieve project-scoped verified evidence from earlier **explicitly opted-in** completed runs. No trial generations or source auto-adoption.
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

The Rust harness owns planning, bounded context projection, permissions, MCP tool calls, parallel read-only inspections, delegation, limits, pause/resume, tool receipts and independent verification. Built-in audio/video generation and standalone RSI campaign endpoints have been retired; attach external creative applications through **MCP** as needed. Historical local data and stored artifacts are not deleted during this migration.

Automatic strategy learning is disabled by default. A user may explicitly save a note or opt into evidence recording, and the existing Run journal remains authoritative. SDK and frontend code do not perform hidden self-modification.

Requires Node 24.12+ or a modern browser with fetch and readable streaming responses.
