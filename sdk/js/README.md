# DOMINION native agent SDK (JavaScript/TypeScript)

The SDK drives DOMINION's own Rust harness, **not** GitHub Copilot CLI,
Codex/Claude Code, or any external agent policy runtime.

Install locally from this repository with npm (no third-party runtime packages):

    npm install ./sdk/js

Example:

    import {DominionClient} from '@dominion/agent-sdk';
    const dom = new DominionClient({baseUrl: 'http://127.0.0.1:4317', token: process.env.DOMINION_TOKEN});
    const session = await dom.sessions.create({title:'New session'});
    const result = await dom.chat.run({sessionId:session.id,input:'Inspect this codebase',toolsEnabled:true,toolsExecutionEnabled:true});
    console.log(result.message?.content);

Long-running isolated coding work:

    const goal = await dom.goals.create({
      goal:'Fix the parser and run tests',
      isolateWorktree:true,
      verifyCommands:['cargo test --release'],
      maxIterations:16
    });
    const done = await dom.goals.wait(goal.id,{onUpdate: g => console.log(g.status)});
    // A needs_review state requires explicit review before dom.goals.resume(goal.id).

Events:

    for await (const e of dom.events.watch({after:0})) console.log(e.type, e.seq);

## Architecture

App/Node.js → @dominion/agent-sdk → /api/chat (SSE), /api/goals,
 /api/jobs, /api/events (durable SSE), /api/approvals → DOMINION Rust
  → native model+tool loop, MCP, permissions/approvals, SQLite evidence,
    agent sessions, Git worktree, verification and independent RSI.

No refusal, censorship or external preference policy is bundled with the
SDK. Current user input goes verbatim to /api/chat and /api/goals.
The server still enforces its *existing* execution permissions, budget,
explicit change approval and tool availability.

An aborted SDK waiter does not cancel an existing job: call jobs.cancel
explicitly. An interrupted /api/chat stream is an error, not a completed
agent turn. Chat run fetches the actual saved assistant message by ID.
Durable events reconnect by seq; replay is bounded by the server's log
retention. Remote deployments should use HTTPS and a protected token.

API contracts are implemented by src/api/{chat,goals,jobs,approvals,
sessions,system}.rs and src/engine/chat. No model API fees in SDK tests.

    cd sdk/js
    npm test

## Optional native Rust process lifecycle (Node only)

The main SDK connects to an already running DOMINION server. To start the
**DOMINION Rust binary itself** (not Copilot CLI), use the Node entrypoint:

    import {DominionRuntime} from '@dominion/agent-sdk/node';
    const runtime = await DominionRuntime.start({binaryPath:'/path/to/dominion-rust',port:0});
    const answer = await runtime.client.chat.run({input:'Implement my request',toolsEnabled:true,toolsExecutionEnabled:true});
    await runtime.close();

The launcher owns its child process, chooses an available port with port=0,
checks /api/health against a unique process instance ID to avoid connecting to
an unrelated server, and shuts down via DOMINION_DESKTOP_PARENT stdin EOF.
It does not download, bundle or substitute another vendor's agent engine.

The SDK refuses to forward the DOMINION owner token to a different origin or follow HTTP redirects. Native launcher shutdown first closes the parent-owned stdin and then escalates to process termination if its own Rust child ignores EOF.
