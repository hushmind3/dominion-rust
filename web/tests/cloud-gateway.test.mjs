import {test} from 'node:test'
import assert from 'node:assert/strict'
import worker from '../../cloud/src/index.js'

// Exercise the ported Worker request handler with deterministic stand-in D1
// queries. No supplier API, Google credentials, real payments or network calls.
const origin='http://127.0.0.1:5173'
const auth='Bearer '+'S'.repeat(43)
const endpoint=(path,method='GET',body,headers={})=>new Request(
 'https://dominion-cloud.example.test'+path,{
 method,headers:{origin,...headers},...(body===undefined?{}:{body:JSON.stringify(body),
 headers:{origin,'content-type':'application/json',...headers}})
})
const db=()=>{
 let revoked=false
 const counters=new Map()
 let deletions=0
 return {
  get revoked(){return revoked},
  get deletions(){return deletions},
  prepare(sql){
   return {bind(...args){
    return {
     async first(){
      if(sql.includes('FROM sessions'))return revoked?null:{id:'google:smoke',credits:0}
      if(sql.includes('FROM subscriptions'))return null
      if(sql.includes('FROM identity_verifications'))return null
      if(sql.includes('INSERT INTO free_quota')){
       const key=String(args[0]),limit=Number(args[1])
       const n=counters.get(key)||0
       if(n>=limit)return null
       counters.set(key,n+1);return {used:n+1}
      }
      if(sql.includes('SELECT used FROM free_quota'))return {used:counters.get(String(args[0]))||0}
      return null
     },
     async run(){
      if(sql.includes('DELETE FROM sessions')){revoked=true;deletions++}
      if(sql.includes('UPDATE free_quota')){const key=String(args[0]);counters.set(key,Math.max(0,(counters.get(key)||0)-1))}
      return {success:true}
     },
     async all(){return {results:[]}}
    }
   }}
  }
 }
}
test('Worker public health and runtime OAuth config never expose supplier secrets',async()=>{
 const DB=db()
 const env={DB,ALLOWED_ORIGIN:origin,GOOGLE_CLIENT_ID:'public-google-id',
  KAKAO_CLIENT_ID:'kakao-public',KAKAO_CLIENT_SECRET:'super-private-secret',
  AI:{},UPSTREAM_API_KEY:'operator-api-key'}
 const health=await worker.fetch(endpoint('/health'),env)
 assert.equal(health.status,200)
 assert.equal((await health.json()).service,'DOMINION Cloud')
 const response=await worker.fetch(endpoint('/v1/config'),env)
 assert.equal(response.status,200)
 const text=await response.text(),config=JSON.parse(text)
 assert.equal(config.google_client_id,'public-google-id')
 assert.equal(config.login.kakao,true)
 assert.equal(config.login.naver,false)
 assert.equal(config.checkout_enabled,false)
 assert.ok(!text.includes('super-private-secret'))
 assert.ok(!text.includes('operator-api-key'))
})
test('request origin and missing authorization are rejected without hitting models',async()=>{
 const env={DB:db(),ALLOWED_ORIGIN:origin,AI:{run:async()=>({response:'not called'})}}
 const forbidden=await worker.fetch(new Request(
 'https://dominion-cloud.example.test/v1/models',{headers:{origin:'https://evil.test'}}),env)
 assert.equal(forbidden.status,403)
 const unauthorized=await worker.fetch(endpoint('/v1/models'),env)
 assert.equal(unauthorized.status,401)
 const noDb=await worker.fetch(endpoint('/v1/config'),{ALLOWED_ORIGIN:origin})
 assert.equal(noDb.status,503)
})
test('one Dominion session lists models, streams quota-bound free text and can be revoked',async()=>{
 const DB=db()
 const env={DB,ALLOWED_ORIGIN:origin,AI:{run:async()=>({response:'native worker reply'})},
  FREE_DAILY_LIMIT:'1'}
 const list=await worker.fetch(endpoint('/v1/models','GET',undefined,
  {authorization:auth}),env)
 assert.equal(list.status,200)
 const available=(await list.json()).data
 assert.ok(available.some(x=>x.id==='dominion/free-auto'))
 assert.ok(available.some(x=>x.id==='@cf/meta/llama-3.1-8b-instruct-fast'))
 const payload={model:'@cf/meta/llama-3.1-8b-instruct-fast',
  messages:[{role:'user',content:'hello'}],stream:true}
 const output=await worker.fetch(endpoint('/v1/chat/completions','POST',payload,
  {authorization:auth}),env)
 assert.equal(output.status,200)
 assert.ok((output.headers.get('content-type')||'').includes('text/event-stream'))
 const text=await output.text()
 assert.ok(text.includes('native worker reply'))
 assert.ok(text.includes('data: [DONE]'))
 const quota=await worker.fetch(endpoint('/v1/chat/completions','POST',payload,
  {authorization:auth}),env)
 assert.equal(quota.status,429)
 const paid=await worker.fetch(endpoint('/v1/chat/completions','POST',{
  model:'some/paid-model',messages:payload.messages,stream:false
 },{authorization:auth}),env)
 assert.equal(paid.status,403)
 const revoked=await worker.fetch(endpoint('/v1/auth/logout','POST',{},
  {authorization:auth}),env)
 assert.equal(revoked.status,200)
 assert.equal((await revoked.json()).revoked,true)
 assert.equal(DB.deletions,1)
 const after=await worker.fetch(endpoint('/v1/models','GET',undefined,
  {authorization:auth}),env)
 assert.equal(after.status,401)
})
test('checkout requires real identity, pricing, and merchant setup before accepting payment',async()=>{
 const env={DB:db(),ALLOWED_ORIGIN:origin}
 const response=await worker.fetch(endpoint('/v1/billing/checkout','POST',{plan:'pro'},
  {authorization:auth}),env)
 assert.equal(response.status,403)
 const details=await response.json()
 assert.match(details.error,/Identity verification/)
})
