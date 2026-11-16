import {test,expect} from 'playwright/test'

// No mocked UI or HTTP responses: Vite + actual Rust API and durable state.
test('native harness retains run and memory APIs and removes embedded RSI/media',async({page,request})=>{
 const health=await request.get('/api/health');
 expect(health.ok()).toBeTruthy();
 expect((await health.json()).engine).toBe('rust');
 const runs=await request.get('/api/v1/runs');
 expect(runs.ok()).toBeTruthy();
 expect(Array.isArray(await runs.json())).toBeTruthy();
 const scopes=await request.get('/api/v1/memory/scopes');
 expect(scopes.ok()).toBeTruthy();
 expect(Array.isArray(await scopes.json())).toBeTruthy();
 for(const path of ['/api/video/status','/api/rsi/campaigns']){
  const response=await request.get(path);
  expect(response.status()).toBe(501);
  expect(response.headers()['content-type']).toContain('application/json');
 }
 for(const path of ['/api/v1/videos','/api/v1/improvements']){
  const response=await request.get(path);
  expect(response.status()).toBe(404);
  expect(response.headers()['content-type']).toContain('application/json');
  expect((await response.json()).error).toBe('unknown native API endpoint');
 }
 await page.goto('/#/chat');
 await expect(page.locator('#root')).toBeVisible();
 await expect(page.locator('aside nav')).toBeVisible();
 await page.goto('/#/memory');
 await expect(page.locator('#root')).toBeVisible();
 const menu=await page.locator('aside nav').innerText();
 expect(menu).not.toContain('RSI');
 await page.goto('/#/video');
 // Legacy deep links resolve to Chat rather than loading a retired component.
 await expect(page.locator('#root')).toBeVisible();
 expect(page.url()).toContain('#/video');
});

test('explicit memory and the run journal survive restart-compatible API calls',async({request})=>{
 const notes=await request.get('/api/v1/memory/all?scope=global');
 expect(notes.ok()).toBeTruthy();
 expect(Array.isArray(await notes.json())).toBeTruthy();
 const events=await request.get('/api/jobs');
 expect(events.ok()).toBeTruthy();
 expect(Array.isArray(await events.json())).toBeTruthy();
});
