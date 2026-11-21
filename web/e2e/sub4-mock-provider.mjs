// Loopback-only deterministic provider for end-to-end browser+Rust tests.
// This is an HTTP server, never a fake React route or token-saving claim.
import {createServer} from 'node:http'

const calls=[]
const MAX_BODY=160_000
function send(res,status,payload){
 res.writeHead(status,{'content-type':'application/json; charset=utf-8','cache-control':'no-store'})
 res.end(JSON.stringify(payload))
}
const server=createServer(async(req,res)=>{
 if(req.method==='GET'&&req.url==='/ _health'.replace(' ','')){
  return send(res,200,{ok:true,engine:'loopback-provider'})
 }
 if(req.method==='GET'&&req.url==='/_calls'){
  return send(res,200,{count:calls.length,requests:calls.slice(-30)})
 }
 if(req.method==='POST'&&req.url==='/v1/chat/completions'){
  let bytes=0,buf=''
  for await(const chunk of req){
   bytes+=chunk.length
   if(bytes>MAX_BODY){return send(res,413,{error:'large mock prompt'})}
   buf+=chunk.toString('utf8')
  }
  let input
  try{input=JSON.parse(buf)}catch{return send(res,400,{error:'invalid JSON'})}
  if(!Array.isArray(input.messages)||!input.messages.length)
   return send(res,400,{error:'missing messages'})
  // Never infer success from an HTTP mock: its provider receipt is a known
  // fixture and not treated as a real OpenAI or llama.cpp cache hit.
  const index=calls.length
  calls.push({model:input.model,messages:input.messages,
   toolNames:(input.tools||[]).map(t=>t?.function?.name).filter(Boolean),
   promptCacheKey:input.prompt_cache_key??null})
  // A syntactically valid HTTP provider response with an invalid empty
  // completion must end the real Rust Run without inventing an assistant.
  if(input.messages.some(message=>typeof message.content==='string'
      &&message.content.includes('DOMINION_E2E_FAILED_CHAT_RECEIPT'))){
   return send(res,200,{id:'dominion-e2e-empty-'+index,model:input.model,
    choices:[{index:0,message:{role:'assistant',content:''},finish_reason:'stop'}],
    usage:{prompt_tokens:40,completion_tokens:0,total_tokens:40}})
  }
  // Deterministic one-shot latency fixture: exercise navigation away from a
  // real Rust Run while its provider call is still outstanding. No live API.
  if(input.messages.some(message=>typeof message.content==='string'
      &&message.content.includes('DOMINION_E2E_BACKGROUND_TURN'))){
   await new Promise(resolve=>setTimeout(resolve,4500))
  }
  return send(res,200,{id:'dominion-e2e-chat-'+index,model:input.model,
   choices:[{index:0,message:{role:'assistant',content:'Verified native mock answer '+index},
    finish_reason:'stop'}],
   usage:{prompt_tokens:40,completion_tokens:6,
    total_tokens:46,prompt_tokens_details:{cached_tokens:index===0?0:8}}})
 }
 send(res,404,{error:'not found'})
})
server.listen(4876,'127.0.0.1',()=>console.log('Sub4 loopback provider listening on 4876'))
