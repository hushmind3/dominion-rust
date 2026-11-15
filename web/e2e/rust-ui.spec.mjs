import {test,expect} from 'playwright/test'

test('real Rust API and tool journal complete a browser-submitted task',async({page,request})=>{
 await page.goto('/#/agents')
 await expect(page.getByRole('heading',{name:'새 자율 작업'})).toBeVisible()
 const goal='[browser-e2e] Create benchmark.txt and verify it.'
 await page.getByRole('textbox',{name:'목표'}).fill(goal)
 await page.getByRole('button',{name:/실행 시작/}).click()
 await expect(page.getByText('새 작업을 시작했습니다.')).toBeVisible()
 await expect.poll(async()=>{
  const response=await request.get('http://127.0.0.1:8787/api/v1/runs')
  if(!response.ok())return ''
  const runs=await response.json()
  return runs.find(run=>run.instruction===goal)?.id||''
 },{timeout:25000}).not.toBe('')
 const runs=await (await request.get('http://127.0.0.1:8787/api/v1/runs')).json()
 const id=runs.find(run=>run.instruction===goal)?.id
 expect(id).toBeTruthy()
 await expect.poll(async()=>{
  const result=await request.get('http://127.0.0.1:8787/api/v1/runs/'+id)
  return (await result.json()).status
 },{timeout:30000}).toBe('completed')
 await expect(page.getByText('최종 결과')).toBeVisible()
 await page.getByRole('button',{name:'결과 파일'}).click()
 await expect(page.getByRole('button',{name:'benchmark.txt'})).toBeVisible()
 await page.getByRole('button',{name:'benchmark.txt'}).click()
 await expect(page.getByText('Dominion browser integration verified')).toBeVisible()
 const detail=await (await request.get('http://127.0.0.1:8787/api/v1/runs/'+id)).json()
 expect(detail.events.some(e=>e.kind==='tool_started')).toBe(true)
 expect(detail.events.some(e=>e.kind==='tool_result')).toBe(true)
 expect(detail.in_flight).toBeNull()
 expect(detail.messages.filter(m=>m.role==='tool')).toHaveLength(1)
})

test('browser cancellation drops delayed model plan without stale state',async({page,request})=>{
 await page.goto('/#/agents')
 const goal='[slow-planning] intentionally slow planning fixture; cancel from UI'
 await page.getByRole('textbox',{name:'목표'}).fill(goal)
 await page.getByRole('button',{name:/실행 시작/}).click()
 await expect(page.getByRole('button',{name:/중단/})).toBeVisible()
 await page.getByRole('button',{name:/중단/}).click()
 await expect.poll(async()=>{
  const runs=await (await request.get('http://127.0.0.1:8787/api/v1/runs')).json()
  return runs.find(r=>r.instruction===goal)?.status||'not-found'
 },{timeout:25000}).toBe('cancelled')
 const runs=await (await request.get('http://127.0.0.1:8787/api/v1/runs')).json()
 const found=runs.find(r=>r.instruction===goal)
 expect(found?.plan).toBeNull()
 expect(found?.events?.some(e=>e.kind==='tool_started')).toBe(false)
})
