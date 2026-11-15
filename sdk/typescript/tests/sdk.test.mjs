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
