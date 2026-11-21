import {test,expect} from 'playwright/test'

// Actual loopback Rust server, not the Vite proxy. The Vite proxy sets
// Host to its target, which would mask DNS-rebinding defects in this test.
const rust='http://127.0.0.1:4317'

test('owner reads and mutations reject untrusted Host on both API routers',async({request})=>{
 const good=await request.get(rust+'/api/sessions')
 expect(good.status()).toBe(200)
 const before=(await good.json()).length
 const failures=[
  {host:'attacker.example:4317',path:'/api/sessions',verb:'GET'},
  {host:'127.0.0.1.attacker.example:4317',path:'/api/sessions',verb:'GET'},
  {host:'localhost.attacker.example:4317',path:'/api/v1/runs',verb:'GET'},
  {host:'attacker.example:4317',path:'/',verb:'GET'},
  {host:'attacker.example:4317',path:'/api/sessions',verb:'POST'}
 ] as const
 for(const entry of failures){
  const opts={headers:{Host:entry.host},...(entry.verb==='POST'?{
   data:{agentId:'general',title:'MUST_NOT_BE_CREATED'}}:{})}
  const result=entry.verb==='POST'
   ?await request.post(rust+entry.path,opts)
   :await request.get(rust+entry.path,opts)
  expect(result.status(),entry.host+' '+entry.path).toBe(403)
  expect(result.headers()['content-type']||'').toContain('application/json')
 }
 const after=await (await request.get(rust+'/api/sessions')).json()
 expect(after.length).toBe(before,'hostile Host cannot create an owner session')
})

test('local owner API enforces Origin and Fetch Metadata without breaking Vite',async({request})=>{
 for(const headers of [
  {Origin:'https://attacker.example'},
  {Origin:'null'},
  {Origin:'http://127.0.0.1:9999'},
  {'Sec-Fetch-Site':'cross-site'}
 ]){
  const forbidden=await request.get(rust+'/api/sessions',{headers})
  expect(forbidden.status(),JSON.stringify(headers)).toBe(403)
 }
 const sameOrigin=await request.get(rust+'/api/sessions',{
  headers:{Origin:'http://127.0.0.1:4317',
   'Sec-Fetch-Site':'same-origin'}})
 expect(sameOrigin.status()).toBe(200)
 const vite=await request.get('/api/sessions')
 expect(vite.status()).toBe(200)
 const viaVite=await request.get('/api/sessions',{
  headers:{Origin:'http://localhost:5173','Sec-Fetch-Site':'same-site'}})
 expect(viaVite.status()).toBe(200)
})

test('OAuth redirects retain one-time state path while general owner APIs do not',async({request})=>{
 const callback=await request.get(rust+'/api/oauth/github/callback?code=invalid&state=invalid',{
  headers:{Origin:'https://github.com','Sec-Fetch-Site':'cross-site'},
  maxRedirects:0
 })
 // An invalid OAuth state must be rejected by the callback handler; the
 // shared Origin guard must not block the trusted callback path as owner API.
 expect(callback.status()).not.toBe(403)
 const owner=await request.get(rust+'/api/sessions',{
  headers:{Origin:'https://github.com','Sec-Fetch-Site':'cross-site'}})
 expect(owner.status()).toBe(403)
})
