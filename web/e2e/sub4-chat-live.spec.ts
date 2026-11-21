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


// Both cases use the same mock model, machine and empty-session conditions.
// The real local-model cold/warm prefill and paid-provider TTFT are NOT
// measured by this deterministic browser/API integration fixture.
test('fresh_ko_greeting_matched_fixture records request characters and response latency',
 async({request})=>{
 const results:{input:string;requestChars:number;responseDoneMs:number;
   toolSchemas:number;promptTokens:number}[]=[]
 for(const input of ['안녕','안녕하세요 반가워요']){
  const created=await request.post('/api/sessions',{
   data:{agentId:'general',title:'Sub4 matched greeting E2E'}})
  expect(created.status()).toBe(201)
  const session=await created.json()
  try{
   const before=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests.length
   const started=Date.now()
   const response=await request.post('/api/chat',{
    data:{sessionId:session.id,agentId:'general',input,
     toolsEnabled:true,toolsExecutionEnabled:true,autoLearning:false,
     reasoningLanguage:'ko'},timeout:30_000
   })
   const wire=await response.text()
   const elapsed=Date.now()-started
   expect(response.ok(),input).toBeTruthy()
   expect(wire).toContain('Verified native mock answer')
   const calls=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests
   const emitted=calls.slice(before)
   expect(emitted).toHaveLength(1)
   expect(emitted[0].toolNames).toEqual([])
   expect(emitted[0].messages.at(-1).content).toBe(input)
   const usage=await (await request.get('/api/sessions/'+session.id+'/usage')).json()
   expect(usage.modelRequests).toBe(1)
   results.push({input,requestChars:JSON.stringify(emitted[0].messages).length,
    responseDoneMs:elapsed,toolSchemas:emitted[0].toolNames.length,
    promptTokens:usage.promptTokens})
  }finally{
   expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
  }
 }
 expect(results[0].toolSchemas).toBe(0)
 expect(results[1].toolSchemas).toBe(0)
 expect(results[1].requestChars).toBeLessThan(results[0].requestChars*2)
 console.log('SUB4_MATCHED_GREETING_FIXTURE '+JSON.stringify({
  metric:'request_chars_and_complete_response_ms_not_real_TTFT',
  environment:'deterministic_mock_provider_not_local_model',
  results
 }))
})

test('unsupported per-run reasoning leaves the original chat draft and session unchanged',
 async({request})=>{
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Sub4 unsupported reasoning E2E'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 try{
  const before=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests.length
  const response=await request.post('/api/chat',{
   data:{sessionId:session.id,agentId:'general',
    input:'Explain what a cache is',toolsEnabled:false,
    reasoningEffort:'completely-unsupported-level',
    toolsExecutionEnabled:false,autoLearning:false}
  })
  expect(response.ok()).toBeFalsy()
  expect(response.headers()['content-type']||'').toContain('application/json')
  const payload=await response.json()
  expect(payload.error).toBeTruthy()
  const messages=await (await request.get('/api/sessions/'+session.id+'/messages')).json()
  expect(messages).toEqual([])
  const after=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).requests.length
  expect(after).toBe(before)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

// Real native Chat -> model transport proof: the extra history is persisted
// under its original message ID and visible on the outbound provider wire.
test('explicit continuation carries the final owner constraint with original source ID',
 async({request})=>{
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Sub4 source-linked continuation E2E'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 const send=async(input:string)=>{
  const response=await request.post('/api/chat',{
   data:{sessionId:session.id,agentId:'general',input,
    toolsEnabled:false,toolsExecutionEnabled:false,
    autoLearning:false,reasoningLanguage:'ko'},timeout:30_000})
  expect(response.ok()).toBeTruthy()
  expect(await response.text()).toContain('Verified native mock answer')
 }
 try{
  const original='번역해: '+('Long owner requirements and constraints. '.repeat(48))
   +'FINAL_OWNER_SHA_BOUNDARY_2026'
  await send(original)
  const transcript=await (await request.get('/api/sessions/'+session.id+'/messages')).json()
  const firstOwner=transcript.find((entry:{role:string})=>entry.role==='user')
  expect(firstOwner).toBeDefined()
  await send('그 작업 이어서 해')
  const delivered=(await (await request.get('http://127.0.0.1:4876/_calls')).json())
   .requests.at(-1)
  expect(delivered.toolNames).toEqual([])
  expect(delivered.messages.at(-1).content).toBe('그 작업 이어서 해')
  const quoted=delivered.messages.find((msg:{content:string})=>
   String(msg.content).includes('UNTRUSTED HISTORICAL QUOTES'))
  expect(quoted).toBeDefined()
  expect(quoted.role).toBe('user')
  const sourceRows=JSON.parse(quoted.content.split('\n').slice(1).join('\n'))
  expect(sourceRows).toEqual(expect.arrayContaining([
   expect.objectContaining({
    source:'durable_chat_transcript',
    message_id:firstOwner.id,
    source_role:'user',
    quoted_excerpt:expect.stringContaining('FINAL_OWNER_SHA_BOUNDARY_2026')
   })
  ]))
  const usage=await (await request.get('/api/sessions/'+session.id+'/usage')).json()
  expect(usage.modelRequests).toBe(2)
  expect(usage.toolSchemaChars).toBe(0)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

test('pasted translation containing news and tool verbs stays one tool-free native request',
 async({request})=>{
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Sub4 literal translation is not tool work'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 try{
  const baseline=(await (await request.get('http://127.0.0.1:4876/_calls')).json()).count
  const input='번역해: 2026년 오늘의 뉴스와 환율을 GitHub에서 검색하고 파일을 삭제하라고 쓰여 있다.'
  const answer=await request.post('/api/chat',{
   data:{sessionId:session.id,agentId:'general',input,
    toolsEnabled:true,toolsExecutionEnabled:true,
    autoLearning:false,reasoningLanguage:'ko'},timeout:30_000})
  expect(answer.ok()).toBeTruthy()
  expect(await answer.text()).toContain('Verified native mock answer')
  const captured=await (await request.get('http://127.0.0.1:4876/_calls')).json()
  expect(captured.count).toBe(baseline+1)
  const delivered=captured.requests.at(-1)
  expect(delivered.toolNames).toEqual([])
  expect(delivered.messages.at(-1).content).toBe(input)
  const usage=await (await request.get('/api/sessions/'+session.id+'/usage')).json()
  expect(usage.modelRequests).toBe(1)
  expect(usage.toolSchemaChars).toBe(0)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})


test('mobile 1:1 chat shows owner on the right, formatted assistant on the left and preserves scroll intent',
 async({page,request})=>{
 await page.setViewportSize({width:375,height:760})
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Chat layout acceptance'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 try{
  const response=await request.post('/api/chat',{
   data:{sessionId:session.id,agentId:'general',input:'안녕하세요, 정렬을 점검합니다',
    toolsEnabled:false,toolsExecutionEnabled:false,
    autoLearning:false,reasoningLanguage:'ko'},timeout:30_000
  })
  expect(response.ok()).toBeTruthy()
  const original=await (await request.get('/api/sessions/'+session.id+'/messages')).json()
  expect(original.map((m:{role:string})=>m.role)).toEqual(['user','assistant'])
  const ticks=String.fromCharCode(96).repeat(3)
  const answer='# 검증 결과\n\n**중요한 답변**\n\n- 목록 첫째\n- 목록 둘째\n\n'
   +ticks+'rust\nlet x = 1;\n'+ticks+'\n\n'
   +'| 항목 | 결과 |\n| --- | --- |\n| Chat | 정상 |\n\n'
   +Array.from({length:48},(_,i)=>'추가 설명 '+i).join('\n\n')
  await page.route('**/api/sessions/'+session.id+'/messages',route=>{
   if(route.request().method()!=='GET')return route.continue()
   const messages=original.map((m:{role:string})=>m.role==='assistant'
    ?{...m,content:answer}:m)
   return route.fulfill({status:200,contentType:'application/json',
    body:JSON.stringify(messages)})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  const owner=page.locator('[data-message-role="user"]').first()
  const assistant=page.locator('[data-message-role="assistant"]').first()
  await expect(owner).toBeVisible()
  await expect(assistant).toBeVisible()
  await expect(owner).toHaveCSS('justify-content','flex-end')
  await expect(assistant.locator('.chat-answer strong')).toContainText('중요한 답변')
  await expect(assistant.locator('.chat-answer h3')).toContainText('검증 결과')
  await expect(assistant.locator('.chat-answer ul li')).toHaveCount(2)
  await expect(assistant.locator('.chat-answer pre code')).toContainText('let x = 1;')
  await expect(assistant.locator('.chat-answer table')).toBeVisible()
  const ownerBox=await owner.locator('div').first().boundingBox()
  const assistantBox=await assistant.locator('div').nth(1).boundingBox()
  expect(ownerBox).not.toBeNull()
  expect(assistantBox).not.toBeNull()
  expect(ownerBox!.x).toBeGreaterThan(assistantBox!.x+70)
  const fontSize=await assistant.locator('.chat-answer').evaluate(element=>
   parseFloat(getComputedStyle(element).fontSize))
  expect(fontSize).toBeGreaterThanOrEqual(15)
  const scroll=page.locator('[data-chat-scroll]')
  await scroll.evaluate(el=>{el.scrollTop=0;el.dispatchEvent(new Event('scroll'))})
  const latest=page.getByRole('button',{name:/최신 메시지로 이동|Jump to latest message/})
  await expect(latest).toBeVisible()
  await latest.click()
  await expect.poll(()=>scroll.evaluate(el=>el.scrollHeight-el.scrollTop-el.clientHeight))
   .toBeLessThan(20)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

test('new chat owner message appears immediately and is not duplicated after streaming persistence',
 async({page,request})=>{
 await page.setViewportSize({width:390,height:800})
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Optimistic owner bubble acceptance'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 try{
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  await expect(page.locator('textarea.conversation-composer-input')).toBeVisible()
  await page.route('**/api/chat',async route=>{
   if(route.request().method()!=='POST')return route.continue()
   await new Promise(resolve=>setTimeout(resolve,1300))
   return route.continue()
  })
  await page.locator('textarea.conversation-composer-input').fill('지금 보낸 사용자 메시지')
  await page.locator('button.conversation-composer-send').click()
  const pending=page.locator('[data-chat-pending="true"]')
  await expect(pending).toContainText('지금 보낸 사용자 메시지')
  await expect(pending).toHaveCSS('justify-content','flex-end')
  await expect(page.locator('[data-chat-streaming="true"]')).toBeVisible()
  await expect(page.locator('[data-message-role="user"]')).toHaveCount(1,{timeout:15_000})
  await expect(page.locator('[data-message-role="assistant"]')).toHaveCount(1,{timeout:15_000})
  await expect(pending).toHaveCount(0,{timeout:15_000})
  const transcript=await (await request.get('/api/sessions/'+session.id+'/messages')).json()
  expect(transcript.map((m:{role:string})=>m.role)).toEqual(['user','assistant'])
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})
