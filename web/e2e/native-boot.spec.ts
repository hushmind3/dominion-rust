import {test,expect} from 'playwright/test'

// No fake network or model responses: Rust, Vite proxy and React are all live.
test('main frontend boots against the native Rust API',async({page,request})=>{
 const health=await request.get('/api/health')
 expect(health.ok()).toBeTruthy()
 const info=await health.json()
 expect(info.engine).toBe('rust')
 expect(info.runtime).toBe('rust')

 await page.goto('/#/settings')
 await expect(page.locator('#root')).toBeVisible()
 await expect(page.locator('body')).not.toContainText('Server response is not JSON')

 const status=await request.get('/api/status')
 expect(status.ok()).toBeTruthy()
 const data=await status.json()
 expect(data.engine).toBe('rust')
 expect(typeof data.jobs).toBe('number')
})
