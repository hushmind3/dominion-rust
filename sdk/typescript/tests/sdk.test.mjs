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

test('group and video endpoints use the same authenticated Rust API',async()=>{
  const seen=[];
  const client=new Dominion({token:'private',fetch:async(url,options)=>{
    seen.push({url,options});
    return new Response(JSON.stringify({id:'a',status:'open'}),{status:200,headers:{'content-type':'application/json'}});
  }});
  await client.groups.next('group-id');
  await client.videos.changeScene('video-id',1,{heading:'intro',narration:'text',seconds:4});
  assert.ok(seen[0].url.endsWith('/api/v1/groups/group-id/next'));
  assert.ok(seen[1].url.endsWith('/api/v1/videos/video-id/scenes/1'));
  assert.equal(seen[1].options.headers.Authorization,'Bearer private');
});

test('non-weight improvement has a separate explicit approve and adopt path',async()=>{
 const urls=[];
 const client=new Dominion({fetch:async url=>{
   urls.push(url);
   return new Response(JSON.stringify({id:'experiment'}),{status:200,headers:{'content-type':'application/json'}});
 }});
 await client.improvements.approve('test');
 await client.improvements.adopt('test');
 assert.ok(urls[0].endsWith('/improvements/test/approve'));
 assert.ok(urls[1].endsWith('/improvements/test/adopt'));
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

test('RSI multi-file target contract reaches the same native Rust route', async()=>{
  const seen=[];
  const client=new Dominion({fetch:async(url,options)=>{
    seen.push({url,options});
    return new Response(JSON.stringify({id:'trial'}),{status:201,headers:{'content-type':'application/json'}});
  }});
  await client.improvements.create({repository:'repo',target_file:'src/lib.rs',
    target_files:['src/lib.rs','src/engine.rs'],instruction:'improve',
    test_command:'cargo test',score_command:'echo 1'});
  assert.ok(seen[0].url.endsWith('/api/v1/improvements'));
  assert.deepEqual(JSON.parse(seen[0].options.body).target_files,['src/lib.rs','src/engine.rs']);
});


test('multi-shot production uses native product API and publishes the final assembled ID',async()=>{
  const calls=[];
  const client=new Dominion({token:'sub4-token',fetch:async(url,options)=>{
    calls.push({url,options});
    if(url.endsWith('/video/productions/job-1'))return new Response(JSON.stringify({
      id:'job-1',status:'ready',videoId:'assembled-1'
    }),{status:200});
    if(url.endsWith('/video/jobs/assembled-1/content'))return new Response(
      new Blob(['real bytes'],{type:'video/mp4'}),{status:200,headers:{'content-type':'video/mp4'}});
    if(url.endsWith('/video/productions')&&options.method==='GET')
      return new Response(JSON.stringify([]),{status:200});
    return new Response(JSON.stringify({id:'upload-1',status:'running'}),{status:200});
  }});
  assert.deepEqual(await client.productions.list(),[]);
  await client.productions.create({topic:'cinematic city',visualEngine:'comfyui',duration:30,
    maxCostKrw:2500,autoPublish:false});
  const result=await client.productions.download('job-1');
  assert.equal(await result.text(),'real bytes');
  const upload=await client.productions.publish('job-1',{title:'Final video',privacyStatus:'private'});
  assert.equal(upload.id,'upload-1');
  assert.ok(calls[0].url.endsWith('/api/video/productions'));
  assert.ok(calls[1].url.endsWith('/api/video/productions'));
  assert.equal(JSON.parse(calls[1].options.body).duration,30);
  assert.ok(calls.some(c=>c.url.endsWith('/api/video/jobs/assembled-1/content')));
  assert.ok(calls.some(c=>c.url.endsWith('/api/video/jobs/assembled-1/upload')));
  assert.ok(calls.every(c=>c.options.headers.Authorization==='Bearer sub4-token'));
});
