import {test} from 'node:test';
import assert from 'node:assert/strict';
import {Dominion, DominionError} from '../dist/index.js';
test('typed client points all operations to the same execution API', async () => {
  const seen=[];
  const client = new Dominion({
    baseUrl:'https://example.invalid/',
    token:'secret',
    fetch:async (url, options) => {
      seen.push({url,options});
      if (url.endsWith('/runs')) return new Response(JSON.stringify([]),{status:200,headers:{'content-type':'application/json'}});
      return new Response(JSON.stringify({ok:true}),{status:200,headers:{'content-type':'application/json'}});
    }
  });
  assert.deepEqual(await client.runs.list(),[]);
  assert.equal((await client.runs.resume('test-id')).ok,true);
  assert.equal(seen[0].url,'https://example.invalid/api/v1/runs');
  assert.equal(seen[1].options.headers.Authorization,'Bearer secret');
});
test('client exposes structured http error',async () => {
  const client = new Dominion({fetch:async()=>new Response(JSON.stringify({error:'denied'}),{status:403})});
  await assert.rejects(()=>client.runs.list(), error=>error instanceof DominionError && error.status===403 && error.message==='denied');
});
test('SSE stream parses split frames and does not replay duplicate sequence',async () => {
  const encoder=new TextEncoder();
  const stream=new ReadableStream({start(controller){
    controller.enqueue(encoder.encode('id: 1\ndata: {"seq":1,"kind":"plan","message":"ok","detail":null,"at_ms":1}\n'));
    controller.enqueue(encoder.encode('\nid: 2\ndata: {"seq":2,"kind":"completed","message":"done","detail":null,"at_ms":2}\n\n'));
    controller.close();
  }});
  const client=new Dominion({fetch:async()=>new Response(stream,{status:200,headers:{'content-type':'text/event-stream'}})});
  const events=[];
  for await (const event of client.events('run')) events.push(event.seq);
  assert.deepEqual(events,[1,2]);
});

test('group operations stay on the authenticated native Rust API',async()=>{
 const seen=[];
 const sdk=new Dominion({token:'private',fetch:async(url,options)=>{
  seen.push({url,options});
  return new Response(JSON.stringify({id:'room-1',status:'open'}),{status:200});
 }});
 await sdk.groups.next('room-1');
 assert.ok(seen[0].url.endsWith('/api/v1/groups/room-1/next'));
 assert.equal(seen[0].options.headers.Authorization,'Bearer private');
});

test('reconcile records observed tool outcome through Rust API',async()=>{
  const seen=[];
  const sdk=new Dominion({token:'private',fetch:async(url,options)=>{
    seen.push({url,options});
    return new Response(JSON.stringify({status:'paused',in_flight:null}),{status:200});
  }});
  const run=await sdk.runs.reconcile('run-id',{succeeded:true,result:'file verified'});
  assert.equal(run.status,'paused');
  assert.ok(seen[0].url.endsWith('/api/v1/runs/run-id/reconcile'));
  assert.deepEqual(JSON.parse(seen[0].options.body),{succeeded:true,result:'file verified'});
  assert.equal(seen[0].options.headers.Authorization,'Bearer private');
});

test('native Rust subagent routes are exposed by thin TypeScript client',async()=>{
  const urls=[];
  const sdk=new Dominion({fetch:async(url,options)=>{
    urls.push({url,options});
    return new Response(JSON.stringify([]),{status:200});
  }});
  await sdk.runs.subagents('parent-id');
  await sdk.runs.delegate('parent-id',{instruction:'inspect docs',budget_krw:100,max_steps:3});
  assert.ok(urls[0].url.endsWith('/api/v1/runs/parent-id/subagents'));
  assert.equal(urls[0].options.method,'GET');
  assert.equal(urls[1].options.method,'POST');
  assert.equal(JSON.parse(urls[1].options.body).budget_krw,100);
});


test('read-only delegation, lazy skills and durable memory use native API',async()=>{
  const calls=[];
  const sdk=new Dominion({fetch:async(url,options)=>{
    calls.push({url,options});
    return new Response(JSON.stringify({ok:true}),{status:200,headers:{'content-type':'application/json'}});
  }});
  await sdk.runs.delegate('parent',{instruction:'investigate only',read_only:true,budget_krw:120});
  await sdk.runs.skills('child');
  await sdk.runs.skill('child','review');
  await sdk.memory.store({scope:'projects/demo',title:'build',content:'cargo test'});
  await sdk.memory.search('projects/demo','cargo');
  assert.equal(JSON.parse(calls[0].options.body).read_only,true);
  assert.ok(calls[1].url.endsWith('/runs/child/skills'));
  assert.ok(calls[2].url.endsWith('/runs/child/skills/review'));
  assert.ok(calls[3].url.endsWith('/memory'));
  assert.ok(calls[4].url.includes('scope=projects%2Fdemo'));
  assert.ok(calls[4].url.includes('query=cargo'));
});

test('verified procedure lookup and explicit memory remain SDK operations',async()=>{
 const seen=[];
 const sdk=new Dominion({token:'secret',fetch:async(url,options)=>{
  seen.push({url,options});
  return new Response(JSON.stringify([]),{status:200});
 }});
 assert.deepEqual(await sdk.strategies.search('repo','compile fix',3),[]);
 assert.deepEqual(await sdk.memory.list('repo'),[]);
 assert.ok(seen[0].url.includes('/api/v1/strategies?repository=repo'));
 assert.ok(seen[0].url.includes('query=compile%20fix'));
 assert.ok(seen[1].url.endsWith('/api/v1/memory/all?scope=repo'));
 assert.ok(seen.every(c=>c.options.headers.Authorization==='Bearer secret'));
});

test('main SQLite preview and explicit import use one authenticated frontend adapter',async()=>{
  const requests=[];
  const client=new Dominion({token:'secret',fetch:async(url,options)=>{
    requests.push({url,options});
    return new Response(JSON.stringify({importableSessions:2,importedSessions:2}),{status:200});
  }});
  const path='/opt/dominion/runtime/dominion.sqlite';
  const preview=await client.frontend.legacySqlite.preview(path);
  assert.equal(preview.importableSessions,2);
  const result=await client.frontend.legacySqlite.import(path);
  assert.equal(result.importedSessions,2);
  assert.ok(requests[0].url.endsWith('/api/system/legacy-sqlite/preview'));
  assert.ok(requests[1].url.endsWith('/api/system/legacy-sqlite/import'));
  assert.equal(requests[0].options.headers.Authorization,'Bearer secret');
  assert.deepEqual(JSON.parse(requests[0].options.body),{path});
  assert.deepEqual(JSON.parse(requests[1].options.body),{path,confirm:true});
});

test('archived jobs and memories remain inert until the owner explicitly promotes one',async()=>{
  const seen=[];
  const sdk=new Dominion({token:'operator',fetch:async(url,options)=>{
    seen.push({url,options});
    return new Response(JSON.stringify({rows:[],total:0,limit:20,offset:0}),{status:200});
  }});
  await sdk.frontend.legacySqlite.archive('/runtime/dominion.sqlite');
  await sdk.frontend.legacyHistory.list('job',0,20);
  await sdk.frontend.legacyHistory.item('memory','old/record');
  await sdk.frontend.legacyHistory.promoteMemory('old/record','projects/demo');
  assert.equal(JSON.parse(seen[0].options.body).confirm,true);
  assert.ok(seen[1].url.includes('/api/system/legacy-archive?kind=job'));
  assert.ok(seen[2].url.includes('id=old%2Frecord'));
  assert.deepEqual(JSON.parse(seen[3].options.body),{
    id:'old/record',scope:'projects/demo',confirm:true
  });
  assert.ok(seen.every(request=>request.options.headers.Authorization==='Bearer operator'));
});
