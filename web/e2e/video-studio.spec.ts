import {test,expect} from 'playwright/test'

/** Exercises the production Video page and its native Rust router; no page
 * route mocking, fake job JSON or simulated OAuth provider is used. */
test('video studio has an accessible creation to publishing workflow',async({page,request})=>{
 const health=await request.get('/api/video/status')
 expect(health.ok()).toBeTruthy()
 const payload=await health.json()
 expect(payload).toHaveProperty('config.engine')
 expect(payload).toHaveProperty('jobs')

 await page.goto('/#/video')
 await expect(page.getByRole('heading',{name:'아이디어에서 영상, 게시까지.'})).toBeVisible()
 await expect(page.getByRole('heading',{name:'새 영상 만들기'})).toBeVisible()
 await expect(page.getByRole('heading',{name:'결과 미리보기'})).toBeVisible()
 await expect(page.getByRole('button',{name:/영상 생성 시작|엔진 설정 후 생성 가능/})).toBeDisabled()

 const landscape=page.getByRole('button',{name:/가로 영상/})
 await landscape.click()
 await expect(landscape).toHaveAttribute('aria-pressed','true')

 await page.getByRole('button',{name:/작업 보관함/}).click()
 await expect(page.getByRole('heading',{name:'모든 제작 작업'})).toBeVisible()
 await expect(page.getByRole('group',{name:'작업 상태 필터'})).toBeVisible()
 await page.getByRole('button',{name:/확인 필요/}).click()
 await expect(page.getByRole('button',{name:/확인 필요/}).last()).toHaveAttribute('aria-pressed','true')

 await page.getByRole('button',{name:/YouTube 게시/}).click()
 await expect(page.getByRole('heading',{name:'YouTube에 게시'})).toBeVisible()
 await expect(page.getByRole('button',{name:'YouTube 업로드 시작'})).toBeDisabled()

 await page.getByRole('button',{name:/엔진·연결/}).click()
 await expect(page.getByRole('heading',{name:'영상 생성 엔진'})).toBeVisible()
 await expect(page.getByRole('heading',{name:'YouTube 계정 연결'})).toBeVisible()
 await expect(page.getByRole('textbox',{name:'Google OAuth Client Secret'})).toHaveCount(0)
})

test('video settings submit to Rust and invalid workflows are rejected visibly',async({page,request})=>{
 await page.goto('/#/video')
 await page.getByRole('button',{name:/엔진·연결/}).click()
 const comfy=page.getByRole('button',{name:/ComfyUI · AI 생성/})
 await comfy.click()
 await expect(comfy).toHaveAttribute('aria-pressed','true')
 const after=await request.get('/api/video/config')
 expect(after.ok()).toBeTruthy()
 expect((await after.json()).engine).toBe('comfyui')

 const workflow=page.locator('input[type=file][accept*="json"]')
 await workflow.setInputFiles({
  name:'not-api-format.json',mimeType:'application/json',
  buffer:Buffer.from(JSON.stringify({nodes:[],links:[]}))
 })
 await expect(page.getByRole('alert')).toContainText('workflow', {ignoreCase:true})

 // Restore the native engine so other tests start with a working fallback.
 await page.getByRole('button',{name:/로컬 FFmpeg/}).click()
 const native=await request.get('/api/video/config')
 expect((await native.json()).engine).toBe('native')
})
