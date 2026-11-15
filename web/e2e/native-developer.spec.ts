import {test,expect} from 'playwright/test'

// No fake network or model responses: Rust, Vite proxy and React are all live.
test('main frontend boots against the native Rust API',async({page,request})=>{
 const health=await request.get('/api/health')
 expect(health.ok()).toBeTruthy()
 const info=await health.json()
 expect(info.engine).toBe('rust')
 expect(info.runtime).toBe('rust')

 await page.goto('/#/developer')
 await expect(page.locator('#root')).toBeVisible()
 await expect(page.locator('#root')).toContainText('Git')
 await expect(page.locator('body')).not.toContainText('Server response is not JSON')

 const status=await request.get('/api/dev/status')
 expect(status.ok()).toBeTruthy()
 const data=await status.json()
 expect(data).toHaveProperty('config.sourceDir')
 expect(data).toHaveProperty('preview.status')
 expect(typeof data.git).toBe('boolean')
})

test('Developer job polling is served by Rust and not a SPA fallback',async({request})=>{
 const job=await request.get('/api/dev/jobs/00000000-0000-4000-8000-000000000000')
 expect(job.status()).toBe(404)
 expect(job.headers()['content-type']).toContain('application/json')
 expect(await job.json()).toEqual({error:'developer job not found'})
})
