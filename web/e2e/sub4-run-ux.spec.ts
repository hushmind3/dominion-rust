import {test,expect} from 'playwright/test'

// Real Rust + Vite + browser for Run persistence; the second test replaces
// only one GET Run detail to exercise mobile controls without executing writes.
async function createCompletedChatRun(request:any){
 const created=await request.post('/api/sessions',{
  data:{agentId:'general',title:'Sub4 Run result reopen E2E'}})
 expect(created.status()).toBe(201)
 const session=await created.json()
 const response=await request.post('/api/chat',{
  data:{sessionId:session.id,agentId:'general',input:'안녕하세요',
   toolsEnabled:true,toolsExecutionEnabled:true,
   autoLearning:false,reasoningLanguage:'ko'},timeout:30_000
 })
 expect(response.ok()).toBeTruthy()
 expect(await response.text()).toContain('Verified native mock answer')
 const messages=await (await request.get('/api/sessions/'+session.id+'/messages')).json()
 const assistant=messages.find((entry:any)=>entry.role==='assistant')
 const runId=assistant?.metadata?.executionId
 expect(typeof runId).toBe('string')
 return {session,runId}
}
test('mobile browser reopens exact completed Run and its file page without losing conversation',
 async({page,request})=>{
 await page.setViewportSize({width:375,height:760})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  const open=page.getByRole('button',{name:/실행 기록|Run evidence/}).last()
  await expect(open).toBeVisible()
  await open.click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  await expect(dialog).toBeVisible()
  await expect(dialog.getByText(runId,{exact:true})).toBeVisible()
  const files=dialog.getByRole('button',{name:/Skills.*생성 파일|Skills.*generated files/})
  await files.click()
  await expect(dialog.getByText(/작업 파일|Workspace files/).first()).toBeVisible()
  await dialog.getByRole('button',{name:/실행 상세 닫기|Close execution detail/}).click()
  await expect(dialog).toHaveCount(0)
  await expect(page.getByText('Verified native mock answer',{exact:false}).first()).toBeVisible()
  await open.click()
  await expect(dialog.getByText(runId,{exact:true})).toBeVisible()
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

test('mobile pending-approval detail visibly separates approve and deny without issuing either',
 async({page,request})=>{
 await page.setViewportSize({width:360,height:720})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  const original=await (await request.get('/api/v1/runs/'+runId)).json()
  let approvalsPosted=0
  await page.route('**/api/v1/runs/'+runId,route=>{
   if(route.request().method()==='POST'){approvalsPosted++;return route.abort()}
   return route.fulfill({status:200,contentType:'application/json',
    body:JSON.stringify({...original,status:'awaiting_approval',
     pending_approval:{id:'exact-tool-once',name:'write_file',
       arguments:{path:'approved.txt',content:'fixture'}},
     approval_decision:null})})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  await page.getByRole('button',{name:/실행 기록|Run evidence/}).last().click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  const approve=dialog.getByRole('button',{name:/이 작업 승인|Approve tool/})
  const deny=dialog.getByRole('button',{name:/거절|Deny/})
  await expect(approve).toBeVisible()
  await expect(deny).toBeVisible()
  await expect(dialog.getByText('write_file',{exact:true}).first()).toBeVisible()
  expect(approvalsPosted).toBe(0,'approval requires explicit owner interaction')
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

test('mobile chat recovers source-linked paused agent Run without exposing automatic paid retry',
 async({page,request})=>{
 await page.setViewportSize({width:360,height:720})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  const original=await (await request.get('/api/v1/runs/'+runId)).json()
  // Only model an interrupted read view. The actual Rust admission and
  // at-most-once billing fence are tested independently in Rust.
  await page.route('**/api/sessions',async route=>{
   if(route.request().method()!=='GET')return route.continue()
   const actual=await route.fetch()
   const sessions=await actual.json()
   return route.fulfill({response:actual,json:sessions.map((item:any)=>
    item.id===session.id?{...item,pendingRun:runId}:item)})
  })
  await page.route('**/api/v1/runs/'+runId,route=>{
   const events=original.events||[]
   const latest=Math.max(0,...events.map((item:{seq:number})=>item.seq))
   return route.fulfill({status:200,contentType:'application/json',
    body:JSON.stringify({...original,model_only:false,status:'paused',
     events:[...events,{seq:latest+1,kind:'model_request_intent',
      message:'Provider model call started',detail:{stage:'agent_turn'},
      at_ms:Date.now()}]})})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  // At narrow widths the model picker overlaps extra header controls.
  // The source Run recovery action belongs in the composer, never the header.
  const source=page.getByRole('button',{name:/원본 실행·복구 열기|Inspect original Run/}).first()
  await expect(source).toBeVisible()
  await source.click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  await expect(dialog.getByText(runId,{exact:true})).toBeVisible()
  await expect(dialog.getByText(/모델 응답 확인 필요|Model response requires reconciliation/))
   .toBeVisible()
  await expect(dialog.getByRole('button',{name:/이어 실행|Resume/})).toHaveCount(0)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})

test('mobile Team editor rejects cyclic dependencies before creating a real Rust draft',
 async({page})=>{
 await page.setViewportSize({width:375,height:760})
 await page.goto('/#/activity')
 await page.getByRole('button',{name:/멀티에이전트 작업 팀|Multi-agent Task Team/}).click()
 const preview=page.getByRole('region',{name:/작업 의존성 미리보기|Task dependency preview/})
 await expect(preview.getByText('research',{exact:true})).toBeVisible()
 await page.getByLabel(/팀 전체 목표|Team goal/).fill('Implement a small change with source evidence')
 const dependency=page.getByLabel(/선행 작업 ID|Depends on/).nth(1)
 await dependency.fill('verify')
 await expect(preview.getByRole('alert')).toContainText(/순환 의존성|dependencies form a cycle/)
 const create=page.getByRole('button',{name:/팀 초안 생성|Create team draft/})
 await expect(create).toBeDisabled()
 await dependency.fill('research')
 await expect(preview.getByText(/단계 3|Stage 3/)).toBeVisible()
 const responsePromise=page.waitForResponse(response=>
  response.url().endsWith('/api/v1/task-teams')&&response.request().method()==='POST')
 await create.click()
 const response=await responsePromise
 expect(response.status()).toBe(201)
 const saved=await response.json()
 expect(saved.status).toBe('paused')
 expect(saved.team.tasks).toHaveLength(3)
 await expect(page.getByText(saved.id,{exact:true})).toBeVisible()
})


test('Run details distinguish missing required test from a fresh approved test receipt',
 async({page,request})=>{
 await page.setViewportSize({width:375,height:760})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  const original=await (await request.get('/api/v1/runs/'+runId)).json()
  const previous=original.events||[]
  const latest=Math.max(0,...previous.map((event:{seq:number})=>event.seq))
  let executed=false
  await page.route('**/api/v1/runs/'+runId,route=>{
   if(route.request().method()!=='GET')return route.continue()
   const event={seq:latest+1,
    kind:executed?'test_evidence_checked':'test_evidence_missing',
    message:executed?'Approved test command succeeded after edits':'Requested tests not yet executed',
    at_ms:Date.now(),detail:{testExecutionVerified:executed}}
   return route.fulfill({status:200,contentType:'application/json',
    body:JSON.stringify({...original,status:executed?'completed':'running',
     answer:executed?original.answer:null,events:[...previous,event]})})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  await page.getByRole('button',{name:/실행 기록|Run evidence/}).last().click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  const evidence=dialog.getByRole('status').filter({hasText:/요청한 테스트의 실제 실행 근거|Requested test execution evidence/})
  await expect(evidence).toContainText(/성공 기록이 없어|No fresh successful test receipt/)
  await evidence.getByRole('button',{name:/원본 실행 영수증 보기|Show original execution receipts/}).click()
  await expect(dialog.getByText('test_evidence_missing',{exact:true})).toBeVisible()
  executed=true
  await dialog.getByRole('button',{name:/새로고침|Refresh/}).click()
  await expect(evidence).toContainText(/성공 기록이 있습니다|approved test command succeeded/)
  await expect(dialog.getByText('test_evidence_checked',{exact:true})).toBeVisible()
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})


test('mobile Run shows exact durable program cursor and approved test step',
 async({page,request})=>{
 await page.setViewportSize({width:360,height:720})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  const original=await (await request.get('/api/v1/runs/'+runId)).json()
  const previous=original.events||[]
  const latest=Math.max(0,...previous.map((event:{seq:number})=>event.seq))
  let finished=false
  let approvalPosts=0
  await page.route('**/api/v1/runs/'+runId,route=>{
   if(route.request().method()==='POST'){
    approvalPosts++
    return route.abort()
   }
   if(route.request().method()!=='GET')return route.continue()
   const program={id:'program-one-turn',cursor:3,
    pending_step:'program:program-one-turn:test',halt_requested:false,
    steps:[{id:'inspect',tool:'read_file'},
     {id:'edit',tool:'edit_file'},{id:'write',tool:'write_file'},
     {id:'test',tool:'run_tests'}],
    observations:{inspect:{status:'ok'},edit:{status:'ok'},write:{status:'ok'}}}
   const finalEvent={seq:latest+1,kind:'program_completed',
    message:'4 successful, 0 failed, 0 skipped',
    at_ms:Date.now(),detail:{successful:4,failed:0,skipped:0,model_calls_inside:0}}
   return route.fulfill({status:200,contentType:'application/json',
    body:JSON.stringify({...original,
     status:finished?'completed':'awaiting_approval',
     program_session:finished?null:program,
     pending_approval:finished?null:{id:'program:program-one-turn:test',
      name:'run_tests',arguments:{command:'python3 -m unittest discover'}},
     approval_decision:null,
     events:finished?[...previous,finalEvent]:previous})})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  await page.getByRole('button',{name:/실행 기록|Run evidence/}).last().click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  const progress=dialog.getByRole('region',{name:/실행 프로그램 단계|Native program progress/})
  await expect(progress).toBeVisible()
  await expect(progress).toContainText('3 / 4')
  await expect(progress).toContainText(/test · run_tests/)
  await expect(progress).toContainText(/승인 대기|Approval needed/)
  await expect(dialog.getByRole('button',{name:/이 작업 승인|Approve tool/})).toBeVisible()
  await expect(dialog.getByRole('button',{name:/거절|Deny/})).toBeVisible()
  expect(approvalPosts).toBe(0,'viewing program progress must not authorize a tool')
  finished=true
  await dialog.getByRole('button',{name:/새로고침|Refresh/}).click()
  await expect(progress).toHaveCount(0)
  const receipt=dialog.getByRole('status').filter({hasText:/마지막 작업 프로그램 기록|Last native program receipt/})
  await expect(receipt).toContainText(/4단계|4 steps/)
  await expect(receipt).toContainText(/별개입니다|not overall task acceptance/)
  expect(approvalPosts).toBe(0)
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})


test('mobile Run can locate an old test receipt and search the entire durable journal',
 async({page,request})=>{
 await page.setViewportSize({width:360,height:720})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  const original=await (await request.get('/api/v1/runs/'+runId)).json()
  const events=Array.from({length:86},(_,i)=>({
   seq:i+1,kind:i===2?'test_evidence_missing':'tool_result',
   message:i===2?'EARLIEST_RECOVERY_RECEIPT':'step '+(i+1),
   at_ms:Date.now(),detail:{original_call_id:'call-'+(i+1)}
  }))
  await page.route('**/api/v1/runs/'+runId,route=>{
   if(route.request().method()!=='GET')return route.continue()
   return route.fulfill({status:200,contentType:'application/json',
    body:JSON.stringify({...original,events})})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  await page.getByRole('button',{name:/실행 기록|Run evidence/}).last().click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  const evidence=dialog.getByRole('status')
   .filter({hasText:/요청한 테스트의 실제 실행 근거|Requested test execution evidence/})
  await evidence.getByRole('button',{name:/원본 실행 영수증 보기|Show original execution receipts/}).click()
  const journal=dialog.getByRole('region',{name:/실행 기록 탐색|Browse execution events/})
  await expect(journal.locator('#run-event-3')).toBeVisible()
  await expect(journal.locator('#run-event-3')).toBeInViewport()
  await expect(journal.getByText('EARLIEST_RECOVERY_RECEIPT')).toBeVisible()
  await expect(journal.locator('li')).toHaveCount(16)
  await journal.getByRole('textbox',{name:/실행 기록 검색|Search execution events/}).fill('call-47')
  await expect(journal.locator('li')).toHaveCount(1)
  await expect(journal.locator('#run-event-47')).toBeVisible()
  await journal.getByRole('textbox',{name:/실행 기록 검색|Search execution events/}).fill('')
  await expect(journal.locator('#run-event-86')).toBeVisible()
  await expect(journal.locator('#run-event-3')).toHaveCount(0)
  await journal.getByRole('button',{name:/이전 기록|Older/}).click()
  await expect(journal.locator('#run-event-50')).toBeVisible()
  await journal.getByRole('button',{name:/새 기록|Newer/}).click()
  await expect(journal.locator('#run-event-86')).toBeVisible()
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})


test('Run workspace filename search reaches files beyond the first hundred',
 async({page,request})=>{
 await page.setViewportSize({width:375,height:760})
 const {session,runId}=await createCompletedChatRun(request)
 try{
  const names=Array.from({length:260},(_,i)=>'file-'+String(i).padStart(3,'0')+'.txt')
  const seen:string[]=[]
  await page.route('**/api/v1/runs/'+runId+'/files?*',route=>{
   const url=new URL(route.request().url())
   const term=(url.searchParams.get('name_contains')||'').toLowerCase()
   seen.push(term)
   const namesForPage=names.filter(name=>name.toLowerCase().includes(term))
   const offset=Number(url.searchParams.get('offset')||0)
   const limit=Number(url.searchParams.get('limit')||100)
   return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({
    files:namesForPage.slice(offset,offset+limit),
    offset,limit,hasMore:namesForPage.length>offset+limit
   })})
  })
  const pages:number[]=[]
  await page.route('**/api/v1/runs/'+runId+'/files/file-250.txt?*',route=>{
   const url=new URL(route.request().url())
   const startLine=Number(url.searchParams.get('start_line')||1)
   pages.push(startLine)
   const endLine=Math.min(450,startLine+199)
   const nextStartLine=endLine<450?endLine+1:null
   return route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({
    path:'file-250.txt',
    content:startLine+': PAGE_'+startLine+'_MARKER\n'+endLine+': PAGE_'+endLine+'_MARKER\n',
    startLine,endLine,totalLines:450,nextStartLine,partial:nextStartLine!==null
   })})
  })
  await page.goto('/#/chat?session='+encodeURIComponent(session.id))
  await page.getByRole('button',{name:/실행 기록|Run evidence/}).last().click()
  const dialog=page.getByRole('dialog',{name:/에이전트 실행 증거|Agent execution evidence/})
  await dialog.getByRole('button',{name:/Skills.*생성 파일|Skills.*generated files/}).click()
  await expect(dialog.getByRole('button',{name:/다음 파일|Next/})).toBeVisible()
  const search=dialog.getByRole('textbox',{name:/작업 파일 검색|Search workspace files/})
  await search.fill('FILE-250')
  await expect(dialog.getByRole('button',{name:'file-250.txt'})).toBeVisible()
  await expect(dialog.getByRole('button',{name:'file-249.txt'})).toHaveCount(0)
  await dialog.getByRole('button',{name:'file-250.txt'}).click()
  await expect(dialog.getByText(/PAGE_1_MARKER/)).toBeVisible()
  await expect(dialog.getByRole('button',{name:/다음 줄|Next lines/})).toBeVisible()
  await dialog.getByRole('button',{name:/다음 줄|Next lines/}).click()
  await expect(dialog.getByText(/PAGE_201_MARKER/)).toBeVisible()
  await dialog.getByRole('button',{name:/이전 줄|Previous lines/}).click()
  await expect(dialog.getByText(/PAGE_1_MARKER/)).toBeVisible()
  expect(pages).toEqual([1,201,1])
  expect(seen).toContain('file-250')
  await search.fill('missing-123')
  await expect(dialog.getByText(/일치하는 파일이 없습니다|No matching workspace files/)).toBeVisible()
  await search.fill('')
  await expect(dialog.getByRole('button',{name:'file-000.txt'})).toBeVisible()
 }finally{
  expect((await request.delete('/api/sessions/'+session.id)).ok()).toBeTruthy()
 }
})
