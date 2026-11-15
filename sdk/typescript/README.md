# Dominion TypeScript SDK

A thin, typed convenience client to the native **Rust SDK engine** used by Dominion Studio. It is not a model execution SDK or separate agent harness.

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
