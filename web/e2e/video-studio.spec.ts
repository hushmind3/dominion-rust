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
 await expect(page.getByRole('heading',{name:'주제 하나로, 완성 영상까지.'})).toBeVisible()
 await expect(page.getByText('영상 모델 라이브러리')).toBeVisible()
 await expect(page.getByRole('group',{name:'영상 모델 계열 선택'})).toBeVisible()
 await expect(page.getByRole('group',{name:'영상 모델 양자화 선택'})).toBeVisible()
 await expect(page.getByRole('button',{name:/Q4_K_M/})).toBeVisible()
 await page.getByRole('button',{name:/Q5_K_M/}).click()
 await expect(page.getByRole('button',{name:/Q5_K_M/})).toHaveAttribute('aria-pressed','true')
 await expect(page.getByText(/모델 본체 3.81GB/)).toBeVisible()
 await page.getByRole('button',{name:/LTX-Video 2B/}).click()
 await expect(page.getByRole('button',{name:/LTX-Video 2B/})).toHaveAttribute('aria-pressed','true')
 await expect(page.getByRole('button',{name:/FP16/})).toBeVisible()
 await page.getByRole('button',{name:/HunyuanVideo 1.5/}).click()
 await expect(page.getByRole('button',{name:/HunyuanVideo 1.5/})).toHaveAttribute('aria-pressed','true')
 await page.getByRole('button',{name:/Wan 2.2 TI2V 5B/}).click()
 const modelPack=await request.get('/api/video/model-packs')
 expect(modelPack.ok()).toBeTruthy()
 const pack=await modelPack.json()
 expect(pack.id).toBe('wan22-ti2v-5b')
 expect(pack.kind).toBe('video')
 expect(pack.variants.map((v:{id:string})=>v.id)).toContain('q4_k_m')
 expect(pack.variants.map((v:{id:string})=>v.id)).toContain('fp16')
 expect(pack.families.map((f:{id:string})=>f.id)).toEqual([
  'wan22-ti2v-5b','ltx-video-2b','hunyuan-video-15'
 ])
 expect(pack.variants.map((v:{id:string})=>v.id)).toContain('ltx2b-fp16')
 expect(pack.variants.map((v:{id:string})=>v.id)).toContain('hunyuan15-fp16')
 expect(pack.variants.every((v:{modelBytes:number})=>v.modelBytes>1_000_000_000)).toBeTruthy()
 const invalidVariant=await request.post('/api/video/model-packs/install',{data:{variant:'text-llm'}})
 expect(invalidVariant.status()).toBe(400)
 expect(pack.estimatedBytes).toBeGreaterThan(10_000_000_000)
 const badPath=await request.post('/api/video/model-packs/install',{data:{modelsDir:'/does/not/exist/models'}})
 expect(badPath.status()).toBe(400)
 const productions=await request.get('/api/video/productions')
 expect(productions.ok()).toBeTruthy()
 expect(Array.isArray(await productions.json())).toBeTruthy()
 await expect(page.getByRole('button',{name:/자율 제작 시작/})).toBeDisabled()
 await page.getByRole('button',{name:/수동 제작/}).click()
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
 await expect(page.getByRole('textbox',{name:'Google OAuth Client Secret'})).toBeVisible()
})

test('video settings submit to Rust and invalid workflows are rejected visibly',async({page,request})=>{
 await page.goto('/#/video')
 await page.getByRole('button',{name:/엔진·연결/}).click()
 const comfy=page.getByRole('button',{name:/ComfyUI · AI 생성/})
 await comfy.click()
 await expect(comfy).toHaveAttribute('aria-pressed','true')
 await expect.poll(async()=>{
  const after=await request.get('/api/video/config')
  return (await after.json()).engine
 }).toBe('comfyui')

 const workflow=page.locator('input[type=file][accept*="json"]')
 await workflow.setInputFiles({
  name:'not-api-format.json',mimeType:'application/json',
  buffer:Buffer.from(JSON.stringify({nodes:[],links:[]}))
 })
 await expect(page.getByRole('alert')).toContainText('ComfyUI API graph contains invalid node IDs')

 // Restore the native engine so other tests start with a working fallback.
 await page.getByRole('button',{name:/로컬 FFmpeg/}).click()
 await expect.poll(async()=>{
  const native=await request.get('/api/video/config')
  return (await native.json()).engine
 }).toBe('native')
})

test('video creation and model selector fit mobile, tablet and desktop widths',async({page})=>{
 for(const width of [375,768,1440]){
  await page.setViewportSize({width,height:850})
  await page.goto('/#/video')
  await expect(page.getByRole('heading',{name:'주제 하나로, 완성 영상까지.'})).toBeVisible()
  await expect(page.getByRole('group',{name:'영상 모델 계열 선택'})).toBeVisible()
  const overflowing=await page.evaluate(()=>document.documentElement.scrollWidth>window.innerWidth+2)
  expect(overflowing,'Unexpected horizontal overflow at '+width+'px').toBeFalsy()
 }
})
