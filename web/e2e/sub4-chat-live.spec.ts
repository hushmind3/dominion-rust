import {test,expect} from 'playwright/test'

// Real Rust HTTP server, Vite proxy and browser; only the selected language
// model is a transparent deterministic loopback fixture.
test('native 1:1 chat streams responses, persists actual provider receipts and omits tool schemas for direct answers',
 async({page,request})=>{
 const created=await request.post('/api/sessions',{data:{agentId:'general',title:'Sub4 chat token E2E'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 try{
  for(const input of ['안녕하세요','번역해: good morning']){
   const response=await request.post('/api/chat',{
    data:{sessionId:session.id,agentId:'general',input,
     toolsEnabled:true,toolsExecutionEnabled:true,
     autoLearning:false,reasoningLanguage:'ko'},
    timeout:30_000
   })
   expect(response.ok()).toBeTruthy()
   const wire=await response.text()
   expect(wire).toContain('Verified native mock answer')
   expect(wire).toContain('"type":"done"')
  }
  const messages=await (await request.get('/api/sessions/'+session.id+'/messages')).json()
  expect(messages.map((m:{role:string})=>m.role)).toEqual([
   'user','assistant','user','assistant'
  ])
  const usage=await (await request.get('/api/sessions/'+session.id+'/usage')).json()
  expect(usage.modelRequests).toBe(2)
  expect(usage.promptTokens).toBe(80)
  expect(usage.cachedPromptTokens).toBe(8)
  expect(usage.completionTokens).toBe(12)
  expect(usage.cacheUsageReported).toBe(true)
  expect(usage.providerUsageReported).toBe(true)
  expect(usage.measuredRequestFootprints).toBe(2)
  expect(usage.toolSchemaChars).toBe(0)
  expect(usage.contextChars).toBeGreaterThan(0)
  const provider=await request.get('http://127.0.0.1:4876/_calls')
  expect(provider.ok()).toBeTruthy()
  const calls=(await provider.json()).requests
  const responses=calls.slice(-2)
  expect(responses).toHaveLength(2)
  expect(responses.every((c:{toolNames:string[]})=>c.toolNames.length===0)).toBe(true)
  expect(responses[0].messages[1].content).toEqual(responses[1].messages[1].content)
  expect(responses[1].messages.at(-1).content).toBe('번역해: good morning')
  await page.goto('/#/chat')
  await expect(page.locator('#root')).toBeVisible()
  expect(await page.locator('body').innerText()).not.toContain('Server response is not JSON')
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})
