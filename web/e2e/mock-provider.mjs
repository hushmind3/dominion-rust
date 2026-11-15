// Deterministic zero-cost provider fixture: browser -> real Rust API -> tool -> verifier.
import http from 'node:http'
const PORT=18991
const answer=(res,value)=>{
 const body=JSON.stringify({
  choices:[{message:value}],usage:{prompt_tokens:21,completion_tokens:9}
 })
 res.writeHead(200,{'Content-Type':'application/json','Content-Length':Buffer.byteLength(body)})
 res.end(body)
}
const server=http.createServer(async(req,res)=>{
 if(req.method==='GET'&&req.url==='/health'){
  res.writeHead(200,{'Content-Type':'application/json'})
  res.end('{"ok":true}')
  return
 }
 if(req.method!=='POST'||req.url!=='/chat/completions'){
  res.writeHead(404);res.end();return
 }
 const input=[]
 for await(const chunk of req)input.push(chunk)
 let request
 try{request=JSON.parse(Buffer.concat(input).toString('utf8'))}
 catch{res.writeHead(400);res.end();return}
 const messages=request.messages||[]
 const text=JSON.stringify(messages)
 const tools=request.tools||[]
 if(tools.length===0){
  const planning=messages.some(m=>m?.role==='system'&&
    typeof m.content==='string'&&m.content.includes('Produce a concise actionable numbered plan'))
  if(planning){
   if(text.includes('[slow-planning]')){
    await new Promise(resolve=>setTimeout(resolve,8000))
   }
   answer(res,{role:'assistant',content:'1. Write benchmark.txt. 2. Verify actual tool receipts.'})
  }else{
   answer(res,{role:'assistant',content:'PASS'})
  }
  return
 }
 const alreadyExecuted=messages.some(m=>m?.role==='tool')
 if(alreadyExecuted){
  answer(res,{role:'assistant',content:'Created benchmark.txt after recording a successful tool receipt.'})
  return
 }
 const write=tools.some(t=>t?.function?.name==='write_file')
 if(!write){
  answer(res,{role:'assistant',content:'No writable tool advertised; task cannot be completed.'})
  return
 }
 answer(res,{role:'assistant',content:null,tool_calls:[{
  id:'browser-fixture-write-1',type:'function',
  function:{name:'write_file',arguments:JSON.stringify({
    path:'benchmark.txt',content:'Dominion browser integration verified\n'
  })}
 }]})
})
server.listen(PORT,'127.0.0.1',()=>console.log('mock provider on '+PORT))
