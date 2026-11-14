/**
 * Browser-only SSE transport for React. This is not an agent SDK:
 * all plans, decisions, tool effects and memory live in the native Rust core.
 * Never replay a POST after the stream breaks (external tool effects may exist).
 */
export type ChatStreamEvent = {type:string; [key:string]:unknown}
export class ChatStreamError extends Error {}
const MAX_FRAME = 4_194_304

function frameData(frame:string):string|undefined{
 const lines:string[]=[]
 for (const line of frame.split(/\r?\n/)) {
  if(line==='data')lines.push('')
  else if(line.startsWith('data:')){
   const value=line.slice(5)
   lines.push(value.startsWith(' ')?value.slice(1):value)
  }
 }
 return lines.length?lines.join('\n'):undefined
}

export async function* decodeChatEvents(response:Response):AsyncGenerator<ChatStreamEvent>{
 if(!response.ok)throw new ChatStreamError('HTTP '+response.status)
 if(!response.body || !response.headers.get('Content-Type')?.toLowerCase().startsWith('text/event-stream')){
  throw new ChatStreamError('Expected a DOMINION event stream')
 }
 const reader=response.body.getReader()
 const decoder=new TextDecoder()
 let pending=''
 let completed=false
 try{
  while(true){
   const {done,value}=await reader.read()
   pending+=decoder.decode(value||new Uint8Array(),{stream:!done})
   // Parse every complete frame; valid Unicode can straddle network chunks.
   let separator:RegExpExecArray|null
   while((separator=/\r?\n\r?\n/.exec(pending))!==null){
    const frame=pending.slice(0,separator.index)
    pending=pending.slice(separator.index+separator[0].length)
    if(frame.length>MAX_FRAME)throw new ChatStreamError('DOMINION event frame exceeds 4 MiB')
    const data=frameData(frame)
    if(data===undefined||!data.trim())continue
    let event:unknown
    try{event=JSON.parse(data)}catch{throw new ChatStreamError('Malformed DOMINION event JSON')}
    if(!event||typeof event!=='object'||Array.isArray(event)||typeof (event as ChatStreamEvent).type!=='string'){
     throw new ChatStreamError('Malformed DOMINION event envelope')
    }
    const typed=event as ChatStreamEvent
    yield typed
    if(typed.type==='done'){
     completed=true
     return
    }
   }
   if(pending.length>MAX_FRAME)throw new ChatStreamError('DOMINION event frame exceeds 4 MiB')
   if(done)break
  }
  if(!completed)throw new ChatStreamError('DOMINION stream interrupted before done')
 }finally{
  await reader.cancel().catch(()=>{})
  reader.releaseLock()
 }
}
