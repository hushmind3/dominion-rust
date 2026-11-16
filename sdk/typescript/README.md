# Dominion TypeScript SDK

A thin, typed convenience client to the native **Rust SDK engine** used by Dominion Studio. It is not a model execution SDK or separate agent harness.

```ts
import { Dominion } from '@dominion/sdk';
const sdk = new Dominion({baseUrl:'http://127.0.0.1:4317'});
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

## Group collaboration

```ts
const room = await sdk.groups.create({
  topic: 'Review product architecture',
  participants: [
    {name:'architect', role:'design', economy:false},
    {name:'critic', role:'failure analysis', economy:true},
  ],
  budget_krw: 5000
});
await sdk.groups.say(room.id, '@critic challenge the proposal');
const response = await sdk.groups.next(room.id);
```

## Local-first video rendering

```ts
const video = await sdk.videos.create({
  prompt:'Five-second product teaser',
  scenes:[{heading:'Hello', narration:'Welcome to Dominion', seconds:5}]
});
// Poll sdk.videos.get(video.id) until status === 'completed'.
const mp4 = await sdk.videos.download(video.id);
```

The video subsystem uses installed FFmpeg; `espeak` is optional and narration quality depends on the local voice backend. Individual scenes can be revised and re-rendered.

## Non-weight improvement experiments

```ts
const trial = await sdk.improvements.create({
  repository: 'repos/my-project',
  target_file: 'src/optimizer.rs',
  instruction: 'Improve throughput without regressing behavior',
  test_command: 'cargo test',
  score_command: './scripts/throughput-score.sh'
});
await sdk.improvements.approve(trial.id);  // Explicit approval to execute supplied shell commands
// Wait until the trial has status 'evaluated' and accepted === true.
await sdk.improvements.adopt(trial.id);    // Explicitly apply the winning Git commit
const next = await sdk.improvements.descendant(trial.id);
```

Each trial uses detached, independent Git worktrees and runs fixed evaluation commands on baseline and candidate. Improvements inherit text-based reflections, never train or update model weights.

## Resume a crashed tool safely

If a run includes `in_flight`, the server refuses to replay the tool.
Inspect the artifacts or command outcome first, then:

```ts
await sdk.runs.reconcile(run.id,{succeeded:true,result:'Observed file on disk'});
await sdk.runs.resume(run.id);
```

Reconciliation records an observed receipt; it never repeats an external operation.

## Existing project work and subagents

```ts
const coding = await sdk.runs.fromRepository({
  repository: 'repos/my-project',
  instruction: 'Fix the failing tests, inspect the diff and report actual output',
  budget_krw: 1000,
});
const subagent = await sdk.runs.delegate(coding.id, {
  instruction: 'Investigate the module boundaries; report useful findings',
  budget_krw: 200,
});
const children = await sdk.runs.subagents(coding.id);
```

Both routes are thin adapters to native Rust. Repository paths are relative
to DOMINION_WORKSPACE_DIR; the agent receives an isolated Git worktree.


## Native RSI multi-file revisions

The TypeScript client shares Rust's POST /api/v1/improvements route. Pass the
primary target_file and optionally target_files including the primary, with
1-8 source paths. The server persists attempts and real candidate evaluation
results, requires a separate approve call, and accepts adopt only after a
frozen worktree judge reports better evidence. Declared interfaces mirror
the native Rust experiment record; TypeScript does not execute an agent loop.

### Automatic RSI source selection

Call `improvements.create({ repository, instruction, test_command, score_command })` without `target_file` to request goal-based Git source discovery. After explicit evaluation approval, Rust records the chosen `target_files` and `selection_reason` in `Improvement`. Supply `target_file` and `target_files` to select files manually. This is Rust engine behavior, not an alternate TypeScript agent implementation.

## Shared Rust strategy retrieval (2026-11-15)

`await dominion.strategies.search(repository, goal, 4)` returns repository-scoped case evidence. Entries distinguish adopted RSI experiments, independent-verifier completed agent runs and rejected RSI counterexamples. These are direct Rust HTTP records, not a TypeScript implementation of a self-improvement loop. Use `improvements.get(id).strategy` to inspect any recorded conditional strategy hypothesis for an individual trial.

### Memory CRUD against the Rust engine

Dominion.memory.list(scope), .scopes(), .store({scope,title,content}), .revise(id,{scope,title,content}) and .forget(id,scope) call the native Rust memory store. This is a typed transport wrapper, not a separate TypeScript memory engine. The main MemoryPage uses the same REST routes.

## Owner profile API

`dominion.owner.get()` returns the current owner's display profile. `dominion.owner.save({name,avatar})` persists it in Rust. The original frontend owner editor from commit `879dc23d` calls the same service, and no separate frontend-owned profile database is required.

## Main frontend compatibility

The TypeScript SDK now supports `sdk.frontend.agents.list/save/remove`, `sdk.frontend.sessions.list/create/messages/rename/clear/fork/cancel`, `sdk.frontend.providers.list/models`, `sdk.frontend.executions.list/create/get/cancel/resume` and `sdk.frontend.status()`. These call the real Rust-owned `/api/*` compatibility endpoints used by the unchanged latest-main frontend; the original `sdk.runs` still uses `/api/v1/*`. Unsupported native features intentionally reject with status 501.


### Autonomous multi-shot video production (sub4, November 2026)

The TypeScript SDK's productions object calls the same native Rust /api/video/productions journal used by Video Studio. It does not embed a second renderer.

- create({topic,visualEngine:'comfyui',duration:30,maxCostKrw:3000,autoPublish:false}) creates one journaled production.
- get(id) and list() retrieve durable status, scene IDs, completed shot count and artifactRevisionId.
- regenerateShot(id, index, visualPrompt) regenerates only one selected scene, reuses other clips and atomically switches to a newly verified immutable MP4.
- download(id) retrieves the assembled product. publish(id,{title,privacyStatus:'private'}) uses the existing OAuth-backed upload path.

During interruption, pendingRevisionJobId, pendingRevisionShot and pendingRevisionPrompt preserve the identity of the previous remote request. The same shot/prompt can be reconciled and reused if the original ComfyUI receipt is terminal and successful. An uncertain remote result is never automatically reposted.

maxCostKrw covers script model API calls; GPU time, electricity and storage are measured separately and not capped by this value. Visual continuity currently uses a stable text brief, not reference-frame conditioning.
