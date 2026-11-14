import test from 'node:test'
import assert from 'node:assert/strict'
import {
  DominionSDK,
  DominionSDKError,
  parseSSEFrame,
} from '../src/index.ts'

function eventResponse(chunks) {
  return new Response(new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(chunk)
      controller.close()
    },
  }), {headers: {'content-type': 'text/event-stream; charset=utf-8'}})
}
const bytes = text => new TextEncoder().encode(text)
const asJson = (body, status = 200) => new Response(JSON.stringify(body), {
  status, headers: {'content-type': 'application/json'},
})

test('SSE decoder handles CRLF, split UTF-8, keepalive, multiple data fields and done', async () => {
  assert.equal(parseSSEFrame(': keepalive'), undefined)
  assert.equal(parseSSEFrame('event: custom\ndata: {"hello":\ndata: 1}'), '{"hello":\n1}')
  const payload = bytes(': ping\r\n\r\ndata: {"type":"text","delta":"한글"}\r\n\r\ndata: {"type":"done"}\r\n\r\n')
  const unicodeByte = payload.indexOf(0xed)
  assert.ok(unicodeByte > 0)
  const captured = []
  const sdk = new DominionSDK({fetch: async (_url, init) => {
    captured.push(JSON.parse(init.body))
    return eventResponse([
      payload.slice(0, unicodeByte + 1),
      payload.slice(unicodeByte + 1, unicodeByte + 2),
      payload.slice(unicodeByte + 2, 48),
      payload.slice(48),
    ])
  }})
  const observed = []
  for await (const event of sdk.streamChat({sessionId: 'session-1', input: '수정해라'})) observed.push(event)
  assert.deepEqual(observed, [{type:'text', delta:'한글'}, {type:'done'}])
  assert.equal(captured[0].input, '수정해라')
  assert.equal(captured[0].sessionId, 'session-1')
})

test('SDK starts a durable thread, streams a turn and reads the real persisted answer', async () => {
  const calls = []
  const sdk = new DominionSDK({
    token:'secret',
    fetch: async (url, init) => {
      calls.push({url, init})
      if (url === '/api/sessions') return asJson({id:'s1',title:'test'})
      if (url === '/api/chat') {
        assert.equal(init.method, 'POST')
        assert.equal(init.headers.get('X-Dominion-Token'), 'secret')
        assert.equal(JSON.parse(init.body).sessionId, 's1')
        return eventResponse([bytes(
          'data: {"type":"text","delta":"streamed"}\n\n' +
          'data: {"type":"assistant_saved","messageId":"m1","sessionId":"s1"}\n\n' +
          'data: {"type":"done","sessionId":"s1"}\n\n',
        )])
      }
      if (url === '/api/sessions/s1/messages') {
        return asJson([{id:'m1',role:'assistant',content:'Saved canonical answer',session_id:'s1'}])
      }
      throw new Error('Unexpected request: ' + url)
    },
  })
  const thread = await sdk.startThread({title:'test'})
  const result = await thread.run('작성해라')
  assert.equal(result.sessionId, 's1')
  assert.equal(result.messageId, 'm1')
  assert.equal(result.finalResponse, 'Saved canonical answer')
  assert.equal(result.events.at(-1).type, 'done')
  assert.equal(sdk.resumeThread(thread.id).id, 's1')
  assert.equal(calls.length, 3)
})

test('abort signal is forwarded and stream is not retried after interruption', async () => {
  let calls = 0
  const abort = new AbortController()
  const sdk = new DominionSDK({fetch: async (_url, init) => {
    calls++
    assert.equal(init.signal, abort.signal)
    return eventResponse([bytes('data: {"type":"text","delta":"partial"}\n\n')])
  }})
  await assert.rejects(async () => {
    for await (const _event of sdk.resumeThread('s').runStreamed('hello', {signal:abort.signal})) {}
  }, /interrupted before done/)
  assert.equal(calls, 1)
})

test('agent error event rejects run without silently returning success', async () => {
  const sdk = new DominionSDK({fetch: async () => eventResponse([bytes(
    'data: {"type":"error","message":"tool execution failed"}\n\n' +
    'data: {"type":"done"}\n\n',
  )])})
  await assert.rejects(sdk.resumeThread('s').run('do work'), /tool execution failed/)
})

test('malformed SSE envelope and oversize frame are rejected', async () => {
  const malformed = new DominionSDK({fetch: async () => eventResponse([bytes(
    'data: {"unknown":3}\n\n',
  )])})
  await assert.rejects(async () => {
    for await (const _event of malformed.streamChat({sessionId:'s',input:'task'})) {}
  }, /Malformed DOMINION event envelope/)
  const oversized = new DominionSDK({fetch: async () => eventResponse([bytes('a'.repeat(4_194_305))])})
  await assert.rejects(async () => {
    for await (const _event of oversized.streamChat({sessionId:'s',input:'task'})) {}
  }, /exceeds 4 MiB/)
})

test('durable event bus receives sequence numbers and supports explicit replay cursor', async () => {
  const seen = []
  const sdk = new DominionSDK({fetch: async (url, init) => {
    seen.push([url, init.method || 'GET'])
    return eventResponse([bytes(
      'id: 42\n' +
      'data: {"type":"record","seq":42,"kind":"job","id":"j1"}\n\n' +
      ': keepalive\n\n' +
      'id: 43\n' +
      'data: {"type":"approval","seq":43,"id":"a1"}\n\n',
    )])
  }})
  const events = []
  for await (const event of sdk.watchEvents({after:41})) events.push(event)
  assert.deepEqual(events.map(event => event.seq), [42,43])
  assert.deepEqual(seen, [['/api/events?after=41','GET']])
})

test('approval and durable goal APIs use native server contracts', async () => {
  const seen = []
  const sdk = new DominionSDK({fetch: async (url, init) => {
    seen.push([url, init.method || 'GET', init.body && JSON.parse(init.body)])
    if (url === '/api/approvals') return asJson({resolved:true,status:'approved'})
    if (url === '/api/goals' && init.method === 'POST') {
      return asJson({id:'g1', jobId:'goal-job-g1', status:'queued',goal:'task'})
    }
    if (url === '/api/goals/g1/steer') return asJson({id:'g1',status:'running'})
    if (url === '/api/goals/g1/resume') return asJson({id:'g1',status:'queued'})
    if (url === '/api/goals/g1') return asJson({id:'g1',jobId:'goal-job-g1',status:'running',goal:'task'})
    throw new Error('Unexpected path: ' + url)
  }})
  await sdk.resolveApproval('approval-1', true)
  const goal = await sdk.startGoal({goal:'task',isolateWorktree:true})
  assert.equal(goal.id, 'g1')
  await goal.steer('execute updated plan', 3)
  await goal.steerInstruction('최신 지시에 따라 테스트를 먼저 수행', 4)
  await goal.resume({acknowledgeUnknownEffects:true})
  assert.equal((await goal.status()).status, 'running')
  assert.deepEqual(seen[0][2], {id:'approval-1',allow:true})
  assert.deepEqual(seen[2][2], {plan:'execute updated plan',expectedRevision:3})
  assert.deepEqual(seen[3][2], {instruction:'최신 지시에 따라 테스트를 먼저 수행',expectedRevision:4})
})

test('MCP configuration flows into the native engine rather than a third-party SDK', async () => {
  const calls = []
  const sdk = new DominionSDK({fetch: async (url, init) => {
    const payload = typeof init.body === 'string' ? JSON.parse(init.body) : null
    calls.push([url, init.method || 'GET', payload])
    if (url === '/api/mcp' && init.method === 'POST') {
      return asJson({id:'mcp-1',name:payload.name,transport:payload.transport,enabled:true})
    }
    if (url === '/api/mcp') return asJson([{id:'mcp-1',name:'lab',transport:'http'}])
    if (url === '/api/mcp/discover') return asJson([{name:'mcp__lab__query'}])
    if (url === '/api/mcp/mcp-1' && init.method === 'DELETE') return asJson({ok:true})
    throw new Error('Unexpected route ' + url)
  }})
  const saved = await sdk.registerMcpServer({name:'lab',transport:'http',url:'http://127.0.0.1:9999/mcp',apiKey:'secret'})
  assert.equal(saved.id,'mcp-1')
  assert.equal(saved.apiKey,undefined)
  assert.equal((await sdk.mcpServers()).length,1)
  assert.equal((await sdk.discoverMcpTools())[0].name,'mcp__lab__query')
  await sdk.removeMcpServer('mcp-1')
  assert.equal(calls[0][2].url,'http://127.0.0.1:9999/mcp')
  assert.equal(calls[3][1],'DELETE')
})

test('non-2xx responses return typed errors without accidental successful result', async () => {
  const sdk = new DominionSDK({fetch: async () => asJson({error:'not allowed'}, 403)})
  await assert.rejects(
    sdk.startThread(),
    error => error instanceof DominionSDKError && error.status === 403 && error.message === 'not allowed',
  )
})

test('goal lifecycle SDK uses the exact Rust resume and reviewed-diff contracts', async () => {
  const calls = []
  const sdk = new DominionSDK({fetch: async (url, init) => {
    const body = init.body && JSON.parse(init.body)
    calls.push({url,body})
    if (url === '/api/goals/g1/changes') return asJson({reviewDigest:'sha256-reviewed-1'})
    if (url === '/api/goals/g1/apply') return asJson({applied:true})
    if (url === '/api/goals/g1/resume') return asJson({status:'queued'})
    throw Error('Unexpected route '+url)
  }})
  const goal = sdk.resumeGoal('g1')
  assert.equal((await goal.changes()).reviewDigest,'sha256-reviewed-1')
  assert.throws(() => goal.applyChanges(''), /reviewed change digest/)
  assert.throws(() => goal.resume({}), /acknowledgeUnknownEffects/)
  await goal.applyChanges('sha256-reviewed-1')
  await goal.resume({acknowledgeUnknownEffects:true})
  assert.deepEqual(calls[1], {url:'/api/goals/g1/apply',body:{
    reviewDigest:'sha256-reviewed-1',acknowledgeReview:true,
  }})
  assert.deepEqual(calls[2], {url:'/api/goals/g1/resume',body:{
    acknowledgeUnknownEffects:true,
  }})
})

test('owner steering rejects empty corrections before network dispatch', () => {
  const goal = new DominionSDK({fetch: async () => {throw Error('must not dispatch')}}).resumeGoal('test')
  assert.throws(() => goal.steerInstruction(' ', 1), /owner instruction cannot be empty/)
})
