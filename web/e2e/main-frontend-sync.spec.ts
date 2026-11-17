import {test,expect} from 'playwright/test'

// Exercise the latest main conversation shell against the real sub4 Rust API.
test('main chat/group switch preserves a mounted conversation and opens native settings',async({page})=>{
 await page.goto('/#/chat');
 await expect(page.locator('.page-chat')).toBeVisible();
 await expect(page.locator('.conversation-switch')).toBeVisible();
 const chat=page.locator('.page-chat');
 await page.locator('.conversation-switch button').last().click();
 await expect(page.locator('.page-rooms')).toBeVisible();
 await expect(chat).toBeHidden();
 // Both chat and rooms stay mounted: scope the click to the visible room view.
 await page.locator('.page-rooms .conversation-switch button').first().click();
 await expect(chat).toBeVisible();
 // The chat tree stays mounted across a room turn, preserving draft state.
 await expect(page.locator('.page-chat')).toHaveCount(1);

 await page.keyboard.press('Control+,');
 const settings=page.getByRole('dialog',{name:'설정'});
 await expect(settings).toBeVisible();
 await expect(settings.getByText('기존 메인 데이터 가져오기')).toBeVisible();
 await expect(settings.getByText('이전 데이터 열람')).toBeVisible();
 await page.keyboard.press('Escape');
 await expect(settings).toHaveCount(0);
 await expect(chat).toBeVisible();
});

test('new connection hub retains sub4 GitHub repository-grant control',async({page})=>{
 await page.goto('/#/connections');
 const dialog=page.getByRole('dialog',{name:'설정'});
 await expect(dialog).toBeVisible();
 await page.locator('.connection-tabs button').nth(1).click();
 await expect(page.getByRole('button',{name:'GitHub 작업 권한 연결'})).toBeVisible();
 await expect(page.getByRole('button',{name:'Google 문서 작업 권한 연결'})).toBeVisible();
});

test('Google OAuth app can be registered in the account UI without exposing its secret',async({page})=>{
 await page.goto('/#/connections');
 await page.locator('.connection-tabs button').nth(1).click();
 await expect(page.getByRole('button',{name:'Google 문서 작업 권한 연결'})).toBeVisible();
 await page.getByRole('button',{name:'OAuth 앱 설정'}).last().click();
 const modal=page.getByRole('dialog',{name:'OAuth 앱 설정'});
 await expect(modal).toBeVisible();
 await modal.locator('input').first().fill('e2e-google-client');
 await modal.locator('input').nth(1).fill('e2e-secret');
 await modal.getByRole('button',{name:'앱 설정 저장'}).click();
 await expect(modal).toHaveCount(0);
 await expect(page.getByRole('button',{name:'Google 문서 작업 권한 연결'})).toBeEnabled();
 const apps=await (await page.request.get('/api/oauth/providers')).json();
 expect(apps.find((p:{id:string;configured:boolean})=>p.id==='google')?.configured).toBe(true);
 const accounts=await (await page.request.get('/api/oauth/connections')).json();
 expect(JSON.stringify(accounts)).not.toContain('e2e-secret');
});
