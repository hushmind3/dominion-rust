import {test,expect} from 'playwright/test'

// Real Rust HTTP server, Vite proxy and browser; only the selected language
// model is a transparent deterministic loopback fixture.
test('native 1:1 chat streams responses, persists actual provider receipts and omits tool schemas for direct answers',
 async({page,request})=>{
 const created=await request.post('/api/sessions',{data:{agentId:'general',title:'Sub4 chat token E2E'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 try{
  for(const input of ['안녕하세요','번역해: good morning','KV 캐시란 무엇인가요?']){
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
   'user','assistant','user','assistant','user','assistant'
  ])
  const usage=await (await request.get('/api/sessions/'+session.id+'/usage')).json()
  expect(usage.modelRequests).toBe(3)
  expect(usage.promptTokens).toBe(120)
  expect(usage.cachedPromptTokens).toBe(16)
  expect(usage.completionTokens).toBe(18)
  expect(usage.cacheUsageReported).toBe(true)
  expect(usage.providerUsageReported).toBe(true)
  expect(usage.measuredRequestFootprints).toBe(3)
  expect(usage.toolSchemaChars).toBe(0)
  expect(usage.contextChars).toBeGreaterThan(0)
  const provider=await request.get('http://127.0.0.1:4876/_calls')
  expect(provider.ok()).toBeTruthy()
  const calls=(await provider.json()).requests
  const responses=calls.slice(-3)
  expect(responses).toHaveLength(3)
  expect(responses.every((c:{toolNames:string[]})=>c.toolNames.length===0)).toBe(true)
  expect(responses[0].messages[1].content).toEqual(responses[1].messages[1].content)
  expect(responses[1].messages.at(-1).content).toBe('번역해: good morning')
  expect(responses[2].messages.at(-1).content).toBe('KV 캐시란 무엇인가요?')
  await page.goto('/#/chat')
  await expect(page.locator('#root')).toBeVisible()
  expect(await page.locator('body').innerText()).not.toContain('Server response is not JSON')
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

test('older chat evidence survives native context selection and manual compaction without full-history replay',
 async({request})=>{
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Sub4 bounded history integration'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 const send=async(input:string)=>{
  const response=await request.post('/api/chat',{
   data:{sessionId:session.id,agentId:'general',input,
    toolsEnabled:false,toolsExecutionEnabled:false,
    autoLearning:false,reasoningLanguage:'ko'},
   timeout:30_000
  })
  expect(response.ok(),input+' returned '+response.status()).toBeTruthy()
  expect(await response.text()).toContain('Verified native mock answer')
 }
 try{
  for(let i=0;i<14;i++){
   await send(i===4?'번역해: QM5 차량 안전성 관련 기록':'번역해: archived engineering note '+i)
  }
  const before=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests
  const chat=before.at(-1).messages
  expect(chat.some((m:{content:string})=>String(m.content).includes('QM5'))).toBe(false)
  await send('예전에 QM5 차량 안전성 기록 얘기했죠?')
  const recall=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests.at(-1)
  expect(recall.toolNames).toEqual([])
  expect(recall.messages.some((m:{content:string})=>
   String(m.content).includes('QM5 차량 안전성 관련 기록'))).toBe(true)
  const compact=await request.post('/api/sessions/'+session.id+'/compact')
  expect(compact.ok()).toBeTruthy()
  const details=await compact.json()
  expect(details.transcriptPreserved).toBe(true)
  await send('예전에 QM5 차량 안전성 기록 얘기했죠?')
  const after=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests.at(-1)
  expect(after.messages.some((m:{content:string})=>
   String(m.content).includes('QM5 차량 안전성 관련 기록'))).toBe(true)
  const usage=await (await request.get('/api/sessions/'+session.id+'/usage')).json()
  expect(usage.modelRequests).toBe(16)
  expect(usage.providerUsageReported).toBe(true)
  expect(usage.toolSchemaChars).toBe(0)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})
