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

test('2026 paid OpenAI caches stable chat prefix and settles discounted reads and charged writes',async()=>{
 const DB=database(2000000),env=configured(DB),initial=DB.state.credits
 env.PAID_MODELS_JSON=JSON.stringify([{
  id:'dominion/cache-model',provider:'openai',upstream_model:'gpt-6.1-sol',
  input_micro_krw:20,cached_input_micro_krw:2,
  cache_write_micro_krw:25,output_micro_krw:3
 }])
 const largeProfile='USER-SUPPLIED AGENT PROFILE (not higher-priority than system policy):\n'+
  'Review our technical requirements. '.repeat(120)
 const body={model:'dominion/cache-model',messages:[
  {role:'system',content:'You are a safe assistant.'},
  {role:'user',content:largeProfile},
  {role:'user',content:'stable first requirement'},
  {role:'assistant',content:'prior answer'},
  {role:'user',content:'new question'}],max_tokens:10}
 const old=globalThis.fetch;let observed
 globalThis.fetch=async(_url,init)=>{
  observed=JSON.parse(init.body)
  return new Response(JSON.stringify({choices:[{finish_reason:'stop',
    message:{role:'assistant',content:'cached response'}}],
    usage:{prompt_tokens:200,completion_tokens:10,
      prompt_tokens_details:{cached_tokens:120,cache_write_tokens:40}}}),
    {status:200,headers:{'content-type':'application/json'}})
 }
 try{
  const response=await worker.fetch(endpoint(body),env)
  assert.equal(response.status,200)
  assert.equal(observed.prompt_cache_options.mode,'explicit')
  assert.equal(observed.prompt_cache_options.ttl,'30m')
  assert.match(observed.prompt_cache_key,/^dominion-/)
  assert.equal(observed.messages[2].content[0].prompt_cache_breakpoint.mode,'explicit')
  assert.equal(observed.messages.at(-1).content,'new question')
  const charged=40*20+120*2+40*25+10*3
  assert.equal([...DB.state.usage.values()][0].charged,charged)
  assert.equal(initial-DB.state.credits,charged)
 }finally{globalThis.fetch=old}
})
test('invalid provider cache receipt never settles as a measured discount',async()=>{
 const DB=database(),env=configured(DB),old=globalThis.fetch
 globalThis.fetch=async()=>new Response(JSON.stringify({
  choices:[{message:{role:'assistant',content:'invalid cache'}}],
  usage:{prompt_tokens:10,completion_tokens:2,
    prompt_tokens_details:{cached_tokens:30,cache_write_tokens:0}}
 }),{status:200})
 try{
  const result=await worker.fetch(endpoint(payload),env)
  assert.equal(result.status,503)
  const row=[...DB.state.usage.values()][0]
  assert.equal(row.status,'uncertain')
  assert.equal(row.charged,row.reserved)
 }finally{globalThis.fetch=old}
})

test('single-use GPT-5.6+ request opts out of implicit cache writes',async()=>{
 const DB=database(),env=configured(DB),old=globalThis.fetch
 env.PAID_MODELS_JSON=JSON.stringify([{
  id:'dominion/no-anchor',provider:'openai',upstream_model:'gpt-6',
  input_micro_krw:2,output_micro_krw:3
 }])
 let received
 globalThis.fetch=async(_url,init)=>{
  received=JSON.parse(init.body)
  return new Response(JSON.stringify({choices:[{message:{role:'assistant',content:'ok'}}],
   usage:{prompt_tokens:5,completion_tokens:2}}),{status:200})
 }
 try{
  const r=await worker.fetch(endpoint({model:'dominion/no-anchor',
   messages:[{role:'user',content:'one-off'}],max_tokens:8}),env)
  assert.equal(r.status,200)
  assert.equal(received.prompt_cache_options.mode,'explicit')
  assert.equal(received.messages[0].content,'one-off')
  assert.ok(!JSON.stringify(received).includes('prompt_cache_breakpoint'))
 }finally{globalThis.fetch=old}
})

test('DOMINION Cloud scopes GPT-5.6 cache routes independently per chat within the same verified member',async()=>{
 const DB=database(1000000),env=configured(DB),prev=globalThis.fetch;
 env.PAID_MODELS_JSON=JSON.stringify([{
  id:'dominion/cache-session',provider:'openai',upstream_model:'gpt-6.1-sol',
  input_micro_krw:2,output_micro_krw:3
 }]);
 const observed=[];
 globalThis.fetch=async(_url,init)=>{
  const payload=JSON.parse(init.body);
  observed.push({key:payload.prompt_cache_key,scope:payload.prompt_cache_options?.mode});
  return new Response(JSON.stringify({choices:[{message:{role:'assistant',content:'ok'}}],
    usage:{prompt_tokens:20,completion_tokens:4,
      prompt_tokens_details:{cached_tokens:0}}}),{status:200});
 };
 const post=affinity=>new Request('https://dominion-cloud.example.test/v1/chat/completions',{
  method:'POST',headers:{origin,authorization:auth,'content-type':'application/json',
    'x-dominion-chat-affinity':affinity},
  body:JSON.stringify({model:'dominion/cache-session',
    messages:[{role:'user',content:'USER-SUPPLIED AGENT PROFILE: same profile'}],
    max_tokens:8})
 });
 try{
  for(const affinity of ['a'.repeat(32),'a'.repeat(32),'b'.repeat(32)]){
   const response=await worker.fetch(post(affinity),env);
   assert.equal(response.status,200);
  }
  assert.equal(observed.length,3);
  assert.equal(observed[0].key,observed[1].key);
  assert.notEqual(observed[0].key,observed[2].key);
  assert.match(observed[0].key,/^dominion-[A-Za-z0-9_-]+$/);
  assert.equal(observed[0].scope,'explicit');
  assert.ok(!JSON.stringify(observed).includes('google:verified'),
    'upstream cache route must never disclose member ID');
 }finally{globalThis.fetch=prev;}
})
