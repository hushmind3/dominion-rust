import {test,expect} from 'playwright/test'

// Live Rust + Vite + Chromium; no route interception or mocked API payloads.
test('main model menu has live, typed Rust endpoints for models and installations',async({page,request})=>{
 const endpoints=[
  '/api/health','/api/status','/api/providers',
  '/api/model-library/installed','/api/model-library/jobs',
  '/api/model-library/runtime','/api/model-library/runtimes',
  '/api/providers/cli','/api/providers/codex/login/status',
  '/api/skills','/api/mcp','/api/capabilities','/api/oauth/providers'
 ];
 for(const endpoint of endpoints){
  const response=await request.get(endpoint);
  expect(response.ok(),endpoint+' returned '+response.status()).toBeTruthy();
  const contentType=response.headers()['content-type']||'';
  expect(contentType,endpoint).toContain('application/json');
  const payload=await response.json();
  expect(payload).toBeDefined();
  if(['installed','jobs','providers','skills','mcp'].some(name=>
     endpoint.endsWith('/'+name)))expect(Array.isArray(payload),endpoint).toBeTruthy();
 }
 const runtimes=await (await request.get('/api/model-library/runtimes')).json();
 expect(Array.isArray(runtimes.items)).toBeTruthy();
 const cli=await (await request.get('/api/providers/cli')).json();
 expect(cli.find((item:{id:string})=>item.id==='codex')).toBeTruthy();
 expect(JSON.stringify(cli)).not.toContain('access_token');
 const health=await (await request.get('/api/health')).json();
 expect(health.engine).toBe('rust');
 await page.goto('/#/models');
 await expect(page.locator('#root')).toBeVisible();
 expect(await page.locator('body').innerText()).not.toContain('Server response is not JSON');
});

test('native UI session create, rename, query and delete actually persist through HTTP',async({request})=>{
 const before=await request.get('/api/sessions');
 expect(before.ok()).toBeTruthy();
 const created=await request.post('/api/sessions',{data:{agentId:'general',title:'Sub4 live API E2E'}});
 expect(created.status()).toBe(201);
 const session=await created.json();
 expect(session.id).toBeTruthy();
 try{
  const messages=await request.get('/api/sessions/'+session.id+'/messages');
  expect(messages.ok()).toBeTruthy();
  expect(await messages.json()).toEqual([]);
  const renamed=await request.post('/api/sessions/'+session.id+'/rename',
   {data:{title:'Sub4 native persisted session'}});
  expect(renamed.ok()).toBeTruthy();
  expect((await renamed.json()).title).toBe('Sub4 native persisted session');
  const check=await request.get('/api/sessions/'+session.id);
  expect((await check.json()).title).toBe('Sub4 native persisted session');
 }finally{
  const removed=await request.delete('/api/sessions/'+session.id);
  expect(removed.ok()).toBeTruthy();
 }
 const after=await request.get('/api/sessions/'+session.id);
 expect(after.status()).toBe(404);
});

test('native UI memories can be created, read and deleted without leaving stale data',async({request})=>{
 const content='sub4-live-smoke-'+Math.random().toString(36).slice(2);
 const created=await request.post('/api/memories',
  {data:{text:content,scope:'shared'}});
 expect(created.status()).toBe(201);
 const memory=await created.json();
 expect(memory.id).toBeTruthy();
 try{
  const list=await request.get('/api/memories');
  expect(list.ok()).toBeTruthy();
  expect((await list.json()).some((row:{id:string;text:string})=>
   row.id===memory.id&&row.text===content)).toBeTruthy();
 }finally{
  expect((await request.delete('/api/memories/'+memory.id)).ok()).toBeTruthy();
 }
 const after=await request.get('/api/memories');
 expect((await after.json()).some((row:{id:string})=>row.id===memory.id)).toBeFalsy();
});

test('native learning endpoints work and removed RSI endpoints explicitly reject without fake success',async({page,request})=>{
 for(const endpoint of ['/api/experience','/api/transfer']){
  const response=await request.get(endpoint);
  expect(response.ok(),endpoint+' returned '+response.status()).toBeTruthy();
  expect(response.headers()['content-type']||'').toContain('application/json');
  await response.json();
 }
 // The 2026 trimmed Rust kernel intentionally retired the old RSI and
 // experiment managers. A real 501 JSON error is correct, not a fake 200
 // that makes React show an empty or misleading data-driven panel.
 for(const endpoint of ['/api/improvements','/api/algorithms','/api/algorithms/evaluate',
   '/api/algorithms/evaluations','/api/optimizers','/api/policies',
   '/api/policy-searches','/api/experiment-requests']){
  const response=await request.get(endpoint);
  expect(response.status(),endpoint+' should be explicitly retired').toBe(501);
  expect(response.headers()['content-type']||'').toContain('application/json');
  expect((await response.json()).error).toBeTruthy();
 }
 await page.goto('/#/memory');
 await expect(page.locator('#root')).toBeVisible();
 expect(await page.locator('body').innerText()).not.toContain('Server response is not JSON');
});

test('model routes reject missing managed IDs as JSON errors, not SPA HTML',async({request})=>{
 for(const [method,uri] of [['GET','/api/model-library/jobs/not-a-job/status'],
   ['DELETE','/api/model-library/installed/not-a-model']]){
  const response=method==='GET'?await request.get(uri):await request.delete(uri);
  expect(response.ok()).toBeFalsy();
  expect(response.headers()['content-type']||'').toContain('application/json');
  expect((await response.json()).error).toBeTruthy();
 }
});


test('Room quick settings save through the Rust full-draft contract without resetting limits',
 async({page,request})=>{
 const roomDraft={
  name:'Sub4 room option persistence',topic:'Preserve conversation policy and budget',
  ownerName:'Owner',agentIds:['general'],routingMode:'discussion',
  rounds:3,maxTurns:7,lifetimeMaxResponses:4,tokenBudget:1200,
  contextBudgetTokens:1800,autoModels:true,toolsEnabled:true,autoLearning:true,
  models:[],reasoningEffort:''
 }
 const response=await request.post('/api/rooms',{data:roomDraft})
 expect(response.ok()).toBeTruthy()
 const room=await response.json() as {id:string}
 const sent:Array<Record<string,unknown>>=[]
 try{
  await page.route('**/api/rooms',route=>{
   if(route.request().method()==='POST')
    sent.push(route.request().postDataJSON() as Record<string,unknown>)
   return route.continue()
  })
  await page.goto('/#/rooms')
  const listEntry=page.locator('.room-list-item')
   .filter({hasText:'Sub4 room option persistence'})
  await expect(listEntry).toBeVisible()
  await listEntry.click()
  await page.getByRole('button',{name:'대화 설정'}).click()
  const settings=page.getByRole('dialog',{name:'대화 설정'})
  await expect(settings).toBeVisible()
  const tools=settings.locator('label').filter({hasText:'도구 사용'}).locator('input[type="checkbox"]')
  await expect(tools).toBeChecked()
  await tools.uncheck()
  await expect.poll(async()=>{
   const result=await request.get('/api/rooms/'+room.id)
   return (await result.json() as {toolsEnabled:boolean}).toolsEnabled
  }).toBe(false)
  expect(sent.length).toBeGreaterThan(0)
  expect(sent.at(-1)).toMatchObject({...roomDraft,id:room.id,toolsEnabled:false})
  let stored=await (await request.get('/api/rooms/'+room.id)).json()
  expect(stored.lifetimeMaxResponses).toBe(4)
  expect(stored.maxTurns).toBe(7)
  expect(stored.tokenBudget).toBe(1200)
  expect(stored.autoLearning).toBe(true)
  await expect(tools).not.toBeChecked()
  await tools.check()
  await expect.poll(async()=>{
   const result=await request.get('/api/rooms/'+room.id)
   return (await result.json() as {toolsEnabled:boolean}).toolsEnabled
  }).toBe(true)
  stored=await (await request.get('/api/rooms/'+room.id)).json()
  expect(stored.lifetimeMaxResponses).toBe(4)
 }finally{
  await request.delete('/api/rooms/'+room.id)
 }
})
