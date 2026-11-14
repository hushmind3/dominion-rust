import test from 'node:test'
import assert from 'node:assert/strict'
import {decodeChatEvents,ChatStreamError} from '../web/src/chat-stream.ts'

const encoder=new TextEncoder()
function response(chunks,headers={'Content-Type':'text/event-stream'}){
 const stream=new ReadableStream({
  start(controller){for(const chunk of chunks)controller.enqueue(chunk);controller.close()},
 })
 return new Response(stream,{status:200,headers})
}
async function collect(res){
 const events=[]
 for await(const event of decodeChatEvents(res))events.push(event)
 return events
}

test('React transport handles Unicode and split multi-event SSE frames',async()=>{
 const data=encoder.encode(
  ': keepalive\n\n' +
  'data: {"type":"text","delta":"한글🌏"}\r\n\r\n' +
  'data: {"type":"done","sessionId":"s1"}\n\n'
 )
 const chunks=[]
 for(let i=0;i<data.length;i+=3)chunks.push(data.slice(i,i+3))
 const result=await collect(response(chunks))
 assert.equal(result[0].delta,'한글🌏')
 assert.equal(result[1].type,'done')
})

test('incomplete event stream fails rather than silently succeeding or reissuing tools',async()=>{
 await assert.rejects(collect(response([encoder.encode(
  'data: {"type":"text","delta":"partial"}\n\n'
 )])),/interrupted before done/)
})

test('malformed event envelope fails with protocol error',async()=>{
 await assert.rejects(collect(response([encoder.encode(
  'data: {"content":"no type"}\n\n'
 )])),error=>error instanceof ChatStreamError&&/envelope/.test(error.message))
})

test('oversize unfinished frame is rejected',async()=>{
 await assert.rejects(collect(response([encoder.encode('x'.repeat(4_194_305))])),
  /exceeds 4 MiB/)
})

test('SSE requires the correct media type',async()=>{
 await assert.rejects(collect(response([encoder.encode('')],{'Content-Type':'text/html'})),
  /Expected a DOMINION event stream/)
})
