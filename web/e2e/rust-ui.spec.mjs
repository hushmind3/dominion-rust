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
 await expect(page.getByRole('button',{name:'benchmark.txt',exact:true})).toBeVisible()
 await page.getByRole('button',{name:'benchmark.txt',exact:true}).click()
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


test('matched benchmark adapter uses actual Rust isolated worktree and real verifier',async()=>{
 const {spawnSync}=await import('node:child_process')
 const {fileURLToPath}=await import('node:url')
 const {dirname,resolve,join}=await import('node:path')
 const {mkdtempSync,mkdirSync,writeFileSync,readFileSync}=await import('node:fs')
 const runtime=process.env.DOMINION_BROWSER_RUNTIME
 expect(runtime).toBeTruthy()
 const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../..')
 const fixture=join(runtime,'fixture')
 mkdirSync(fixture,{recursive:true})
 writeFileSync(join(fixture,'base.txt'),'untouched baseline\n')
 const verify="from pathlib import Path;assert Path('benchmark.txt').read_text()=='Dominion browser integration verified\\n'"
 const matrix={
  version:1,model:'mock-benchmark',budget_krw:1000,max_steps:8,timeout_seconds:70,
  harnesses:[{
   name:'dominion-rust-fixture',model:'mock-benchmark',limits_enforced:false,
   argv:['python3','{dominion_adapter}','--workspace','{workspace}',
    '--prompt-file','{prompt_file}','--model','{model}',
    '--budget-krw','{budget_krw}','--max-steps','{max_steps}',
    '--telemetry-file','{telemetry_file}']
  }],
  tasks:[{
   id:'rust-actual-worktree',fixture:'fixture',
   prompt:'Create benchmark.txt containing the acceptance fixture.',
   verify:['python3','-c',verify],artifacts:['benchmark.txt']
  }]
 }
 const manifest=join(runtime,'matrix.json')
 const output=join(runtime,'matched.json')
 writeFileSync(manifest,JSON.stringify(matrix))
 const proc=spawnSync('python3',[join(repo,'benchmarks/compare.py'),
  '--matrix',manifest,'--output',output],{
  cwd:repo,encoding:'utf8',timeout:85000,
  env:{...process.env,DOMINION_WORKSPACE_DIR:join(runtime,'workspaces')}
 })
 expect(proc.status,proc.stderr||proc.stdout).toBe(0)
 const result=JSON.parse(readFileSync(output,'utf8'))
 expect(result.summary[0].passed).toBe(1)
 expect(result.attempts[0].passed).toBe(true)
 expect(result.attempts[0].usage.input_tokens).toBeGreaterThan(0)
 expect(result.attempts[0].usage.tool_calls).toBe(1)
 expect(result.attempts[0].usage.cost_krw).toBe(0)
 expect(readFileSync(join(fixture,'base.txt'),'utf8')).toBe('untouched baseline\n')
})
