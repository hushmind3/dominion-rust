import {test,expect} from 'playwright/test'

// No provider or external market feed required: proves the real money UI and
// both HTTP adapters use the native Rust persistent corpus.
test('Money discovery is mounted in the existing main UI and shares Rust APIs',async({page,request})=>{
 const response=await request.get('/api/money/stats');
 expect(response.ok()).toBeTruthy();
 const stats=await response.json();
 expect(typeof stats.signals).toBe('number');
 expect(typeof stats.candidates).toBe('number');
 expect(typeof stats.metrics).toBe('number');
 const native=await request.get('/api/v1/money/find');
 expect(native.ok()).toBeTruthy();
 expect(Array.isArray((await native.json()).drafts)).toBeTruthy();

 await page.goto('/#/money');
 await expect(page.getByRole('heading',{name:'돈벌이 찾기'})).toBeVisible();
 await expect(page.getByRole('button',{name:'돈벌이 찾기',exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'공개 자료 갱신'})).toBeVisible();
 await expect(page.getByText('GitHub·Hugging Face')).toHaveCount(0);
 const policy=await request.post('/api/money/automation',{data:{enabled:false}});
 expect(policy.ok()).toBeTruthy();
 expect((await policy.json()).enabled).toBe(false);
});
