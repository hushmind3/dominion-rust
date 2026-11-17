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

test('account connection uses server-owned GitHub and Google OAuth apps, never user keys',async({page})=>{
 await page.goto('/#/connections');
 await page.locator('.connection-tabs button').nth(1).click();
 const github=page.getByRole('button',{name:'GitHub로 로그인'});
 const google=page.getByRole('button',{name:'Google로 로그인'});
 await expect(github).toBeVisible();
 await expect(google).toBeVisible();
 await expect(github).toBeEnabled();
 await expect(google).toBeEnabled();
 await expect(page.getByText('Client ID')).toHaveCount(0);
 await expect(page.getByText('Client Secret')).toHaveCount(0);
 await expect(page.getByRole('button',{name:'OAuth 앱 설정'})).toHaveCount(0);

 const apps=await (await page.request.get('/api/oauth/providers')).json();
 const googleApp=apps.find((p:{id:string})=>p.id==='google');
 const githubApp=apps.find((p:{id:string})=>p.id==='github');
 expect(googleApp.configured).toBe(true);
 expect(googleApp.managed).toBe(true);
 expect(githubApp.configured).toBe(true);
 expect(githubApp.managed).toBe(true);
 expect(JSON.stringify(apps)).not.toContain('e2e-server-secret-never-expose');

 const googleUrl=new URL((await (await page.request.get('/api/oauth/google/start?workspace=true')).json()).url);
 expect(googleUrl.hostname).toBe('accounts.google.com');
 expect(googleUrl.searchParams.get('client_id')).toBe('dominion-e2e-google-app');
 expect(googleUrl.searchParams.get('scope')).toContain('https://www.googleapis.com/auth/drive');
 expect(googleUrl.searchParams.get('code_challenge')).toBeTruthy();
 expect(googleUrl.toString()).not.toContain('e2e-server-secret');

 const githubUrl=new URL((await (await page.request.get('/api/oauth/github/start?repository=true')).json()).url);
 expect(githubUrl.hostname).toBe('github.com');
 expect(githubUrl.searchParams.get('client_id')).toBe('dominion-e2e-github-app');
 expect(githubUrl.searchParams.get('scope')).toContain('repo');
});

test('GitHub login falls back to official CLI device flow with no app credentials',async({page})=>{
 await page.route('**/api/oauth/providers',route=>route.fulfill({
  status:200,contentType:'application/json',
  body:JSON.stringify([
   {id:'github',configured:false,managed:false,cliAvailable:true},
   {id:'google',configured:false,managed:false,cliAvailable:false}
  ])
 }));
 await page.route('**/api/oauth/github/cli/import',route=>route.fulfill({
  status:400,contentType:'application/json',body:JSON.stringify({error:'not signed in'})
 }));
 await page.route('**/api/oauth/github/cli/login',route=>route.fulfill({
  status:200,contentType:'application/json',
  body:JSON.stringify({phase:'waiting',userCode:'ABCD-1234',startedAt:100,
   message:'GitHub device confirmation'})
 }));
 await page.route('**/api/oauth/github/cli/login/status',route=>route.fulfill({
  status:200,contentType:'application/json',
  body:JSON.stringify({phase:'waiting',userCode:'ABCD-1234',startedAt:100,
   message:'GitHub device confirmation'})
 }));
 await page.route('**/api/oauth/github/cli/login/cancel',route=>route.fulfill({
  status:200,contentType:'application/json',body:JSON.stringify({ok:true})
 }));
 await page.goto('/#/connections');
 await page.locator('.connection-tabs button').nth(1).click();
 const button=page.getByRole('button',{name:'GitHub로 로그인'});
 await expect(button).toBeEnabled();
 await button.click();
 await expect(page.getByText('ABCD-1234')).toBeVisible();
 await expect(page.getByRole('link',{name:'GitHub 인증 페이지 열기'}))
  .toHaveAttribute('href','https://github.com/login/device');
 await page.getByRole('button',{name:'인증 취소'}).click();
 await expect(page.getByText('ABCD-1234')).toHaveCount(0);
});
