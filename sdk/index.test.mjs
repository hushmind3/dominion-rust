import test from 'node:test';
import assert from 'node:assert/strict';
import { DominionClient, DominionError } from './index.mjs';

function client(fetch) { return new DominionClient({ baseUrl: 'http://localhost:4317/', token: 'private-key', fetch }); }
function response(body, options = {}) { return new Response(JSON.stringify(body), { headers: { 'content-type': 'application/json' }, ...options }); }
function sse(chunks) {
  const source = new TextEncoder().encode(chunks.join(''));
  const bytes = [source.slice(0, 7), source.slice(7, 14), source.slice(14, 19), source.slice(19)];
  return new Response(new ReadableStream({ start(ctrl) { bytes.forEach(b => ctrl.enqueue(b)); ctrl.close(); } }),
    { headers: { 'content-type': 'text/event-stream; charset=utf-8' } });
}

test('session creation sends token and the native server-owned schema', async () => {
  let req;
  const sdk = client(async (url, options) => {
    req = { url, ...options };
    return response({ id: 's1', agentId: 'general' });
  });
  const result = await sdk.sessions.create({ agentId: 'general' });
  assert.equal(req.url, 'http://localhost:4317/api/sessions');
  assert.equal(req.headers['X-Dominion-Token'], 'private-key');
  assert.deepEqual(JSON.parse(req.body), { agentId: 'general' });
  assert.equal(result.id, 's1');
});

test('SSE decodes fragmented UTF-8 and waits for the terminal done receipt', async () => {
  const sdk = client(async () => sse([': heartbeat\n\n', 'data: {"type":"text","text":"안녕"}\r\n\r\n',
    'data: {"type":"reasoning","text":"생각"}\n\n', 'data: {"type":"done","saved":true}\n\n']));
  const result = await sdk.session('s1').run('질문', { agentId: 'general' });
  assert.equal(result.text, '안녕');
  assert.equal(result.reasoning, '생각');
  assert.equal(result.done.saved, true);
  assert.equal(result.events.length, 3);
});

test('prematurely closed SSE is never treated as success', async () => {
  const sdk = client(async () => sse(['data: {"type":"text","text":"partial"}\n\n']));
  await assert.rejects(() => sdk.chat.run({ sessionId: 's', agentId: 'a', input: 'x' }),
    err => err instanceof DominionError && err.code === 'STREAM_INTERRUPTED');
});

test('HTTP error preserves status and structured server reason', async () => {
  const sdk = client(async () => response({ error: 'invalid goal' }, { status: 400 }));
  await assert.rejects(() => sdk.goals.create({ goal: 'go' }),
    err => err instanceof DominionError && err.status === 400 && err.message === 'invalid goal');
});

test('durable job polling reaches a recorded terminal job', async () => {
  const statuses = ['queued', 'running', 'completed'];
  const sdk = client(async () => response({ id: 'j1', status: statuses.shift() }));
  const result = await sdk.jobs.wait('j1', { intervalMs: 10, maxPolls: 3 });
  assert.equal(result.status, 'completed');
});

test('approval and verified worktree application use exact Rust API contracts', async () => {
  const requests = [];
  const sdk = client(async (url, options) => {
    requests.push({ url, body: JSON.parse(options.body) });
    return response({ ok: true });
  });
  await sdk.approvals.resolve('ap1', true);
  await sdk.goals.apply('g1', 'abc');
  assert.deepEqual(requests.map(x => x.body), [{ id: 'ap1', allow: true }, { expectedPatchSha256: 'abc' }]);
});

test('SDK forwards owner goal unaltered without third-party policy decisions', async () => {
  let captured;
  const sdk = client(async (_url, options) => { captured = JSON.parse(options.body); return response({ id: 'goal-1' }); });
  const input = { goal: '원하는 작업을 그대로 실행', maxIterations: 16, successCriteria: [] };
  await sdk.goals.create(input);
  assert.deepEqual(captured, input);
});

test('owner steering sends literal correction to the native Rust inbox', async () => {
  let request;
  const sdk = client(async (url, options) => {
    request = { url, ...options };
    return response({ id: 'steer-1', status: 'queued' });
  });
  const exact = '  지금은 이전 계획 중단.\n새 목표를 우선해 주세요.  ';
  const result = await sdk.session('session-one').steer(exact);
  assert.equal(request.url, 'http://localhost:4317/api/sessions/session-one/steer');
  assert.equal(request.method, 'POST');
  assert.deepEqual(JSON.parse(request.body), { instruction: exact });
  assert.equal(result.status, 'queued');
});
