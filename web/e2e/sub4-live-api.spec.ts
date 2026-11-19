import {test,expect} from 'playwright/test'

// Live Rust + Vite + Chromium; no route interception or mocked API payloads.
test('main model menu has live, typed Rust endpoints for models and installations',async({page,request})=>{
 const endpoints=[
  '/api/health','/api/status','/api/providers',
  '/api/model-library/installed','/api/model-library/jobs',
  '/api/model-library/runtime','/api/model-library/runtimes',
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

test('learning panels show native structured evidence instead of missing APIs',async({page,request})=>{
 for(const endpoint of ['/api/experience','/api/improvements','/api/transfer',
  '/api/algorithms','/api/algorithms/evaluate',
  '/api/algorithms/evaluations','/api/optimizers',
  '/api/policies','/api/policy-searches','/api/experiment-requests']){
  const response=await request.get(endpoint);
  expect(response.ok(),endpoint+' returned '+response.status()).toBeTruthy();
  expect(response.headers()['content-type']||'').toContain('application/json');
  await response.json();
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
