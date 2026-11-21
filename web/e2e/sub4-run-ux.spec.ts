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
