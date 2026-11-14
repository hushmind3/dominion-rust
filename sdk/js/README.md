# DOMINION native agent SDK (JavaScript/TypeScript)

The SDK drives DOMINION's own Rust harness, **not** GitHub Copilot CLI,
Codex/Claude Code, or any external agent policy runtime.

Install locally from this repository with npm (no third-party runtime packages):

    npm install ./sdk/js

Example:

    import {DominionClient} from '@dominion/agent-sdk';
    const dom = new DominionClient({baseUrl: 'http://127.0.0.1:3927', token: process.env.DOMINION_TOKEN});
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
