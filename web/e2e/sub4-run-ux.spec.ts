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
