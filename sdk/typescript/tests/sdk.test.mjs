import test from "node:test";
import assert from "node:assert/strict";
import {
  DominionClient, DominionHttpError, DominionProtocolError,
  DominionStreamInterrupted,
} from "../dist/index.js";

const json = (body, status = 200) => new Response(JSON.stringify(body), {
  status, headers: {"content-type": "application/json"},
});
const sse = (...parts) => new Response(new ReadableStream({
  start(controller) {
    const encoder = new TextEncoder();
    for (const part of parts) controller.enqueue(encoder.encode(part));
    controller.close();
  },
}), {headers: {"content-type": "text/event-stream; charset=utf-8"}});

test("typed goals use native Rust endpoint, owner's fields and admin header", async () => {
  const requests = [];
  const sdk = new DominionClient({
    baseUrl: "http://127.0.0.1:4317",
    adminToken: "local-secret",
    fetch: async (url, init) => {
      requests.push({url, init});
      return json({id:"goal-1",goal:"Build a release",status:"queued",jobId:"job-1"});
    },
  });
  const goal = await sdk.goals.create({
    goal:"Build a release",isolateWorktree:true,maxIterations:8,
    executor:{providerId:"p",model:"m"},
  });
  assert.equal(goal.id, "goal-1");
  assert.equal(requests[0].url, "http://127.0.0.1:4317/api/goals");
  assert.equal(requests[0].init.method, "POST");
  assert.equal(requests[0].init.headers.get("X-Dominion-Token"), "local-secret");
  const body = JSON.parse(requests[0].init.body);
  assert.equal(body.executor.model, "m");
  assert.equal(body.isolateWorktree, true);
});

test("server errors retain status, and writes never silently retry", async () => {
  let calls = 0;
  const sdk = new DominionClient({fetch: async () => {
    calls++;
    return json({error:"maintenance mode"},503);
  }});
  await assert.rejects(() => sdk.goals.create({goal:"run"}), error =>
    error instanceof DominionHttpError && error.status === 503 &&
    error.message.includes("maintenance mode"),
  );
  assert.equal(calls, 1);
});

test("SDK supports interrupted stream recovery ONLY through new owner action", async () => {
  const sdk = new DominionClient({
    fetch: async () => sse('data: {"type":"delta","content":"partial"}\n\n'),
  });
  const result = [];
  await assert.rejects(async () => {
    for await (const event of sdk.streamChat({sessionId:"s",input:"edit"})) result.push(event);
  }, DominionStreamInterrupted);
  assert.equal(result.length, 1);
});

test("chat SSE decodes split UTF-8, multi-line data, CRLF and terminal done", async () => {
  const sdk = new DominionClient({fetch: async () => sse(
    ': keep-alive\r\n\r',
    '\n',
    'data: {"type":"delta",\r\n',
    'data: "content":"완료"}\r\n\r\n',
    'data: {"type":"done"}\n\n',
  )});
  const events = [];
  for await (const event of sdk.streamChat({sessionId:"s",input:"go"})) events.push(event);
  assert.deepEqual(events.map(e => e.type), ["delta","done"]);
  assert.equal(events[0].content, "완료");
});

test("chat abort signal remains attached to the actual response stream", async () => {
  const ac = new AbortController();
  let requestSignal;
  const sdk = new DominionClient({fetch: async (_url, init) => {
    requestSignal = init.signal;
    return sse('data: {"type":"done"}\n\n');
  }});
  for await (const _event of sdk.streamChat({sessionId:"s",input:"go"},{signal:ac.signal})) {}
  assert.equal(requestSignal, ac.signal);
});

test("durable event watch reconnects from the last seen cursor and drops duplicates", async () => {
  const seen = [];
  const paths = [];
  const sdk = new DominionClient({fetch: async url => {
    paths.push(String(url));
    if (paths.length === 1) return sse(
      'id: 1\ndata: {"seq":1,"type":"record"}\n\n',
      'id: 2\ndata: {"seq":2,"type":"approval"}\n\n');
    return sse(
      'id: 2\ndata: {"seq":2,"type":"approval"}\n\n',
      'id: 3\ndata: {"seq":3,"type":"record"}\n\n');
  }});
  for await (const event of sdk.watchEvents({retryDelayMs:1,maxReconnects:2})) {
    seen.push(event.seq);
    if (seen.length === 3) break;
  }
  assert.deepEqual(seen,[1,2,3]);
  assert.deepEqual(paths,[
    "http://127.0.0.1:4317/api/events?after=0",
    "http://127.0.0.1:4317/api/events?after=2",
  ]);
});

test("malformed durable event does not become an endless reconnect loop", async () => {
  let count = 0;
  const sdk = new DominionClient({fetch: async () => {
    count++;
    return sse('data: NOT_JSON\n\n');
  }});
  await assert.rejects(async () => {
    for await (const _event of sdk.watchEvents()) {}
  }, DominionProtocolError);
  assert.equal(count,1);
});

test("goal poll observes durable terminal state without another agent turn", async () => {
  let count = 0;
  const updates = [];
  const sdk = new DominionClient({fetch: async () => {
    count++;
    return json({id:"g",goal:"release",status:count===1 ? "running" : "needs_review"});
  }});
  const record = await sdk.goals.wait("g",{
    pollIntervalMs:100,timeoutMs:5_000,onUpdate:g => updates.push(g.status),
  });
  assert.equal(record.status,"needs_review");
  assert.deepEqual(updates,["running","needs_review"]);
  assert.equal(count,2);
});

test("plan steering, resume acknowledgement and approvals use exact APIs", async () => {
  const requests = [];
  const sdk = new DominionClient({fetch: async (url,init) => {
    requests.push({path:new URL(url).pathname,method:init.method,body:JSON.parse(init.body ?? '{}')});
    return json({id:"g",status:"running",ok:true,resolved:true});
  }});
  await sdk.goals.steer("g","Read actual files, implement and test");
  await sdk.goals.resume("g",true);
  await sdk.approvals.resolve("a1",true);
  assert.deepEqual(requests.map(x => x.path),[
    "/api/goals/g/plan","/api/goals/g/resume","/api/approvals",
  ]);
  assert.equal(requests[0].body.plan,"Read actual files, implement and test");
  assert.equal(requests[1].body.acknowledgeUnknownEffects,true);
  assert.equal(requests[2].body.allow,true);
});

test("thread run captures only the newly persisted assistant reply", async () => {
  let reads = 0;
  const calls = [];
  const sdk = new DominionClient({fetch: async (url,init) => {
    const path = new URL(url).pathname;
    calls.push(path);
    if (path === "/api/sessions") return json({id:"s1",title:"agent"});
    if (path === "/api/sessions/s1/messages") {
      reads++;
      const prior = [{id:"old",session_id:"s1",role:"assistant",content:"old answer"}];
      return json(reads === 1 ? prior : [...prior,{
        id:"fresh",session_id:"s1",role:"assistant",content:"new answer",
      }]);
    }
    if (path === "/api/chat") {
      const body = JSON.parse(init.body);
      assert.equal(body.sessionId,"s1");
      assert.equal(body.input,"write a plan");
      return sse('data: {"type":"done"}\n\n');
    }
    return json({error:"unexpected endpoint"},404);
  }});
  const thread = await sdk.startThread({title:"agent"});
  const result = await thread.run("write a plan");
  assert.equal(thread.id,"s1");
  assert.equal(result.finalResponse,"new answer");
  assert.equal(result.message.id,"fresh");
  assert.deepEqual(calls,[
    "/api/sessions","/api/sessions/s1/messages","/api/chat","/api/sessions/s1/messages",
  ]);
});

test("SDK validates base URL and never injects credentials into the URL", () => {
  assert.throws(() => new DominionClient({baseUrl:"file:///tmp/demo"}), TypeError);
  assert.throws(() => new DominionClient({baseUrl:"https://admin:pass@example.com"}), TypeError);
});

test("durable owner steering passes exact instruction, mode and stable requestId",async()=>{
 const requests=[];
 const sdk=new DominionClient({fetch:async(url,init)=>{
   requests.push({path:new URL(url).pathname,body:JSON.parse(init.body)});
   return json({id:"g",goal:"test",status:"running"});
 }});
 await sdk.goals.appendInstruction("g","Read the sources first","owner-100");
 await sdk.goals.replacePlan("g","Use a new implementation","owner-101");
 assert.deepEqual(requests,[
  {path:"/api/goals/g/steer",body:{
    text:"Read the sources first",mode:"append",requestId:"owner-100"}},
  {path:"/api/goals/g/steer",body:{
    text:"Use a new implementation",mode:"replace",requestId:"owner-101"}}
 ]);
 assert.throws(()=>sdk.goals.direct("g",{
   text:"inject",mode:"append",requestId:"../../invalid"
 }),TypeError);
});


test("RSI SDK launches native Rust campaign once and read-only polls proof", async () => {
  const calls=[];
  let reads=0;
  const sdk=new DominionClient({fetch:async (url,init) => {
    const path=new URL(url).pathname;
    calls.push({path,method:init.method,body:init.body?JSON.parse(init.body):null});
    if (path==="/api/rsi/start") return json({id:"rsi-1",kind:"rsi_evolve",status:"queued"});
    if (path==="/api/rsi/campaigns/rsi-1") return json({
      id:"rsi-1",status:++reads===1?"running":"needs_review",
      phase:"evaluation",recursiveProof:{closedLoopObserved:false},
      capabilityDemonstrated:false,
    });
    return json({error:"unexpected"},404);
  }});
  const started=await sdk.rsi.start({
    request:"improve recovery",providerId:"native",model:"m",selectionPolicy:"automatic",
    limits:{maxGenerations:2},
  });
  assert.equal(started.kind,"rsi_evolve");
  assert.equal(calls[0].body.mode,"continuous");
  assert.equal(calls[0].body.confirmation,"START_RSI_EVOLUTION");
  assert.equal(calls[0].body.selectionPolicy,"automatic");
  assert.equal(calls[0].body.limits.maxGenerations,2);
  const finished=await sdk.rsi.wait(started.id,{pollIntervalMs:100,timeoutMs:5000});
  assert.equal(finished.status,"needs_review");
  assert.equal(finished.recursiveProof.closedLoopObserved,false);
  assert.deepEqual(calls.map(c=>c.method),["POST","GET","GET"]);
  await assert.rejects(()=>sdk.rsi.apply("rsi-1","not-a-commit"),TypeError);
  assert.equal(calls.length,3);
});
