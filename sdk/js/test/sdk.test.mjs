import test from 'node:test';
import assert from 'node:assert/strict';
import {DominionClient, DominionError, decodeSSE} from '../index.js';

const enc = new TextEncoder();
const sse = (...chunks) => new Response(new ReadableStream({
  start(controller) {
    for (const chunk of chunks) controller.enqueue(enc.encode(chunk));
    controller.close();
  }
}), {headers: {'content-type': 'text/event-stream'}});
const json = (data, status = 200) => new Response(JSON.stringify(data), {
  status, headers: {'content-type': 'application/json'}
});

test('SSE handles split CRLF, multiline and comments', async () => {
  const items = [];
  for await (const item of decodeSSE(sse(
    ': heartbeat\r\n\r\nid: 7\r\ndata: first\r', '\ndata: second\r\n\r\n'
  ).body)) items.push(item);
  assert.deepEqual(items, [{id: '7', data: 'first\nsecond'}]);
});

test('run sends raw user request and resolves persisted response, not fake text', async () => {
  const sent = [];
  const sdk = new DominionClient({token: 'owner-token', fetch: async (url, init) => {
    sent.push({url, init});
    if (url.endsWith('/api/chat')) return sse(
      'data: {"type":"assistant_saved","messageId":"m1","sessionId":"s1"}\n\n',
      'data: {"type":"done","sessionId":"s1"}\n\n'
    );
    return json([{id:'m1',role:'assistant',content:'done',session_id:'s1'}]);
  }});
  const input = '사용자 지시를 우선 적용해 코드 구현';
  const result = await sdk.chat.run({input, toolsEnabled:true, toolsExecutionEnabled:true});
  assert.equal(result.message.content, 'done');
  assert.equal(JSON.parse(sent[0].init.body).input, input);
  assert.equal(sent[0].init.headers.get('X-Dominion-Token'), 'owner-token');
});

test('chat does not falsely succeed on engine error or interrupted SSE', async () => {
  const errSdk = new DominionClient({fetch: async () => sse(
    'data: {"type":"error","message":"provider unavailable"}\n\n',
    'data: {"type":"done","sessionId":"s"}\n\n'
  )});
  await assert.rejects(errSdk.chat.run({input:'hello'}), /provider unavailable/);
  const interrupted = new DominionClient({fetch: async () => sse('data: {"type":"token"}\n\n')});
  await assert.rejects(async () => {
    for await (const _ of interrupted.chat.stream({input:'hello'})) {}
  }, /without a done event/);
});

test('goal run keeps durable review and exact owner patch approval contract', async () => {
  const sent = [];
  let reads = 0;
  const sdk = new DominionClient({fetch: async (url, init) => {
    const path = new URL(url).pathname; sent.push({path, body:init.body});
    if (path === '/api/goals') return json({id:'g',jobId:'j',status:'queued'});
    if (path === '/api/goals/g' && init.method === 'GET')
      return json({id:'g',status: ++reads === 1 ? 'running' : 'needs_review'});
    return json({id:'g',status:'queued'});
  }});
  const states = [];
  const result = await sdk.goals.run({goal:'fix source',isolateWorktree:true}, {
    pollIntervalMs: 1, onUpdate: x => states.push(x.status)
  });
  assert.equal(result.status, 'needs_review');
  assert.deepEqual(states, ['running','needs_review']);
  await sdk.goals.resume('g');
  assert.deepEqual(JSON.parse(sent.at(-1).body), {acknowledgeUnknownEffects:true});
  await sdk.goals.applyReviewed('g', 'a'.repeat(64));
  assert.deepEqual(JSON.parse(sent.at(-1).body), {expectedPatchSha256:'a'.repeat(64)});
});

test('durable SSE replay reconnects after latest sequence without duplicates', async () => {
  let count = 0; const urls = [];
  const sdk = new DominionClient({fetch: async url => {
    urls.push(url); count++;
    return count === 1 ? sse('id: 9\ndata: {"seq":9,"type":"record"}\n\n') :
      sse('id: 9\ndata: {"seq":9,"type":"record"}\n\n',
          'id: 10\ndata: {"seq":10,"type":"approval"}\n\n');
  }});
  const seen = [];
  for await (const event of sdk.events.watch({after:8,reconnectDelayMs:1})) {
    seen.push(event.seq);
    if (seen.length === 2) break;
  }
  assert.deepEqual(seen,[9,10]);
  assert.match(urls[1], /after=9/);
});

test('steer transmits real owner text to exact session and never spoofs completion', async () => {
  const requests = [];
  const sdk = new DominionClient({token:'owner-token',fetch:async(url,init)=>{
    requests.push({path:new URL(url).pathname,init});
    return json({id:'steer-1',status:'queued',executionId:'active-run'},202);
  }});
  const outcome=await sdk.sessions.steer('chat:1','기존 계획을 버리고 새 지시를 수행해라');
  assert.equal(outcome.status,'queued');
  assert.equal(requests[0].path,'/api/sessions/chat%3A1/steer');
  assert.deepEqual(JSON.parse(requests[0].init.body),
    {input:'기존 계획을 버리고 새 지시를 수행해라'});
  assert.equal(requests[0].init.headers.get('X-Dominion-Token'),'owner-token');
  await assert.rejects(sdk.sessions.steer('chat:1','  '),/non-empty owner instruction/);
  assert.equal(requests.length,1);
});

test('HTTP errors and local abort do not claim remote cancellation', async () => {
  const fail = new DominionClient({fetch: async () => json({error:'conflict'},409)});
  await assert.rejects(fail.approvals.resolve('a',true), error =>
    error instanceof DominionError && error.status === 409 && error.message === 'conflict');
  const ac = new AbortController();
  const calls = [];
  const sdk = new DominionClient({fetch: async url => {
    calls.push(new URL(url).pathname);
    ac.abort(new Error('local stop'));
    return json({id:'g',status:'running'});
  }});
  await assert.rejects(sdk.goals.wait('g',{signal:ac.signal,pollIntervalMs:1}),/local stop/);
  assert.deepEqual(calls,['/api/goals/g']);
});

test('SDK token is never sent to another origin and requests reject redirects', async () => {
  let called = 0;
  const sdk = new DominionClient({baseUrl:'http://127.0.0.1:4317',token:'secret',
    fetch: async (_url, init) => {
      called++;
      assert.equal(init.redirect,'error');
      return json({ok:true});
    }});
  await assert.rejects(sdk.request('GET','https://other.example/steal'), /cannot cross/);
  assert.equal(called,0);
  assert.deepEqual(await sdk.request('GET','api/health'),{ok:true});
  assert.equal(called,1);
});
