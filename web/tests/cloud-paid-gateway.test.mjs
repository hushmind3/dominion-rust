import {test} from 'node:test'
import assert from 'node:assert/strict'
import worker from '../../cloud/src/index.js'

// Pure D1 contract double. The same SQL admission/settlement queries execute
// synchronously in this mock; the real D1 primary serializes each statement.
const origin='http://127.0.0.1:5173'
const auth='Bearer '+'Q'.repeat(43)
const endpoint=(payload)=>new Request('https://dominion-cloud.example.test/v1/chat/completions',{
 method:'POST',headers:{origin,authorization:auth,'content-type':'application/json'},
 body:JSON.stringify(payload)
})
function database(credits=1000000){
 const state={credits,budget:0,limit:0,usage:new Map(),admissions:0}
 return {
  state,
  prepare(sql){
   return{bind(...args){
    return{
     async first(){
      if(sql.includes('FROM sessions s JOIN accounts'))return{id:'google:verified',credits:state.credits}
      if(sql.includes('FROM identity_verifications'))return{verified_at:123,provider:'portone'}
      if(sql.includes('FROM subscriptions'))return null
      if(sql.startsWith('INSERT INTO paid_budget_daily')){
       const amount=Number(args[1]),available=Number(args[2])
       if(state.budget>available)return null
       state.budget+=amount;state.admissions++
       return{reserved:state.budget}
      }
      if(sql.startsWith('UPDATE accounts SET credits=credits-?')){
       const amount=Number(args[0])
       if(state.credits<amount)return null
       state.credits-=amount
       return{credits:state.credits}
      }
      throw Error('unexpected first SQL: '+sql)
     },
     async run(){
      if(sql.startsWith('INSERT INTO usage')){
       const [id,user,model,reserved,status,createdAt]=args
       state.usage.set(id,{user,model,reserved,status,createdAt,charged:0});return{success:true}
      }
      if(sql.startsWith('UPDATE accounts SET credits=credits+?')){
       state.credits+=Number(args[0]);return{success:true}
      }
      if(sql.startsWith('UPDATE usage SET charged=')){
       const receipt=state.usage.get(args[2])
       assert.ok(receipt)
       receipt.charged=Number(args[0]);receipt.status=args[1];return{success:true}
      }
      if(sql.startsWith('UPDATE paid_budget_daily SET reserved_micro_krw=')){
       state.budget=Math.max(0,state.budget-Number(args[0]));return{success:true}
      }
      throw Error('unexpected run SQL: '+sql)
     }
    }
   }}
  },
  async batch(statements){
   for(const statement of statements)await statement.run()
   return statements.map(()=>({success:true}))
  }
 }
}
const configured=db=>({
 DB:db,ALLOWED_ORIGIN:origin,
 OPENAI_API_KEY:'secret-never-return',
 PAID_MODELS_JSON:JSON.stringify([{
  id:'dominion/paid-example',provider:'openai',upstream_model:'gpt-test',
  name:'Paid Test',input_micro_krw:2,output_micro_krw:3
 }]),
 PAID_DAILY_BUDGET_MICRO_KRW:'300000'
})
const payload={
 model:'dominion/paid-example',
 messages:[{role:'user',content:'Read the file'}],
 max_tokens:100,
 stream:false,
 tools:[{type:'function',function:{name:'read_file',description:'Read one file',
  parameters:{type:'object',properties:{path:{type:'string'}},required:['path']}}}],
 tool_choice:'auto'
}
test('a verified Dominion account uses an operator-owned paid model with tool schemas and measured debit',async()=>{
 const DB=database(),env=configured(DB),initial=DB.state.credits
 const previous=globalThis.fetch
 let received
 globalThis.fetch=async(url,init)=>{
  assert.equal(url,'https://api.openai.com/v1/chat/completions')
  assert.equal(init.headers.authorization,'Bearer secret-never-return')
  received=JSON.parse(init.body)
  return new Response(JSON.stringify({id:'chatcmpl-test',
   choices:[{index:0,finish_reason:'tool_calls',message:{role:'assistant',
    content:null,tool_calls:[{id:'call_1',type:'function',
     function:{name:'read_file',arguments:'{"path":"a.txt"}'}}]}}],
   usage:{prompt_tokens:31,completion_tokens:8,total_tokens:39}}),
   {status:200,headers:{'content-type':'application/json'}})
 }
 try{
  const res=await worker.fetch(endpoint(payload),env)
  assert.equal(res.status,200)
  const answer=await res.json()
  assert.equal(answer.choices[0].message.tool_calls[0].function.name,'read_file')
  assert.deepEqual(received.tools,payload.tools)
  assert.equal(received.tool_choice,'auto')
  const receipt=[...DB.state.usage.values()][0]
  const expected=31*2+8*3
  assert.equal(receipt.status,'completed')
  assert.equal(receipt.charged,expected)
  assert.equal(DB.state.credits,initial-expected)
  assert.equal(DB.state.budget,expected)
  assert.equal(DB.state.admissions,1)
  assert.ok(!JSON.stringify(answer).includes('secret-never-return'))
 }finally{globalThis.fetch=previous}
})
test('two simultaneous paid calls cannot both pass one atomic operator budget allowance',async()=>{
 const DB=database(),env=configured(DB)
 const encoded=JSON.stringify({messages:payload.messages,tools:payload.tools,tool_choice:'auto'})
 const reserve=new TextEncoder().encode(encoded).length*2+100*3
 env.PAID_DAILY_BUDGET_MICRO_KRW=String(reserve+1)
 const previous=globalThis.fetch
 let finish
 globalThis.fetch=()=>new Promise(resolve=>{finish=()=>{
  resolve(new Response(JSON.stringify({choices:[{message:{role:'assistant',content:'ok'}}],
   usage:{prompt_tokens:12,completion_tokens:3}}),{status:200}))
 }})
 try{
  const pending=worker.fetch(endpoint(payload),env)
  for(let i=0;i<100&&!finish;i++)await new Promise(resolve=>setTimeout(resolve,1))
  assert.equal(typeof finish,'function','first request must enter upstream')
  const duplicate=await worker.fetch(endpoint(payload),env)
  assert.equal(duplicate.status,429)
  assert.match((await duplicate.json()).error,/daily model budget/)
  assert.equal(DB.state.admissions,1)
  finish()
  assert.equal((await pending).status,200)
  assert.equal(DB.state.budget,12*2+3*3)
 }finally{globalThis.fetch=previous}
})
test('ambiguous paid transport failure retains bounded funds with a reconciliation ID',async()=>{
 const DB=database(),env=configured(DB),previous=globalThis.fetch
 const initial=DB.state.credits
 globalThis.fetch=async()=>{throw Error('socket closed after provider accepted request')}
 try{
  const answer=await worker.fetch(endpoint(payload),env)
  assert.equal(answer.status,503)
  const report=await answer.json()
  assert.equal(report.reconciliationRequired,true)
  assert.match(report.requestId,/^[0-9a-f-]{36}$/)
  const row=[...DB.state.usage.values()][0]
  assert.equal(row.status,'uncertain')
  assert.equal(row.charged,row.reserved)
  assert.equal(DB.state.credits,initial-row.reserved)
  assert.equal(DB.state.budget,row.reserved)
 }finally{globalThis.fetch=previous}
})
test('invalid paid tool requests never reserve the operator budget',async()=>{
 const DB=database(),env=configured(DB)
 const response=await worker.fetch(endpoint({...payload,
  tools:[{type:'function',function:{name:'not safe !',parameters:{type:'object'}}}]}),env)
 assert.equal(response.status,400)
 assert.equal(DB.state.admissions,0)
 assert.equal(DB.state.credits,1000000)
})
