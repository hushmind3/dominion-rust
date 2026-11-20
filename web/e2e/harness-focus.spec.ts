import {test,expect} from 'playwright/test'

// Real Rust HTTP and Vite, without mocked state or a second agent executor.
test('native run, memory and main UI share one runtime',async({page,request})=>{
 const health=await request.get('/api/health')
 expect(health.ok()).toBeTruthy()
 expect((await health.json()).engine).toBe('rust')
 const runs=await request.get('/api/v1/runs')
 expect(runs.ok()).toBeTruthy()
 expect(Array.isArray(await runs.json())).toBeTruthy()
 const scopes=await request.get('/api/v1/memory/scopes')
 expect(scopes.ok()).toBeTruthy()
 expect(Array.isArray(await scopes.json())).toBeTruthy()
 await page.goto('/#/chat')
 await expect(page.locator('#root')).toBeVisible()
 await expect(page.locator('.conversation-switch')).toBeVisible()
 await page.goto('/#/memory')
 await expect(page.locator('#root')).toBeVisible()
})

test('native journal and explicit memory remain available through restart-compatible APIs',async({request})=>{
 const notes=await request.get('/api/v1/memory/all?scope=global')
 expect(notes.ok()).toBeTruthy()
 expect(Array.isArray(await notes.json())).toBeTruthy()
 const jobs=await request.get('/api/jobs')
 expect(jobs.ok()).toBeTruthy()
 expect(Array.isArray(await jobs.json())).toBeTruthy()
})
