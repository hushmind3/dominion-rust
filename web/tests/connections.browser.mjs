// Optional REAL Chromium interaction smoke. Run with:
//   npm ci --prefix web && npx --prefix web playwright install chromium
//   node web/tests/connections.browser.mjs
// The backend is deliberately mocked: this checks UI wiring only, not MCP
// server installation, OAuth handshakes or model/provider outcomes.
import {spawn} from 'node:child_process'
import {strict as assert} from 'node:assert'
import {setTimeout as sleep} from 'node:timers/promises'
import {fileURLToPath} from 'node:url'
import {chromium} from 'playwright'

const root=fileURLToPath(new URL('../../',import.meta.url))
const base='http://127.0.0.1:5173'
let server
let browser

async function waitForVite(){
 for(let attempt=0;attempt<80;attempt++){
  if(server.exitCode!==null)throw Error('Vite exited before becoming ready')
  try{
   const response=await fetch(base,{signal:AbortSignal.timeout(500)})
   if(response.ok)return
  }catch{}
  await sleep(250)
 }
 throw Error('Vite did not become available on port 5173')
}
function getPayload(path, memory){
 if(path==='/api/status')return {jobs:0}
 if(path==='/api/capabilities')return {items:[]}
 if(path==='/api/skill-catalog')return {items:[],total:0,updatedAt:''}
 if(path==='/api/memories')return memory
 return []
}
async function run(){
 server=spawn(process.platform==='win32'?'npm.cmd':'npm',['run','dev','--prefix','web'],{
  cwd:root,env:{...process.env,DOMINION_API_URL:'http://127.0.0.1:4317'},
  stdio:'ignore',
 })
 await waitForVite()
 browser=await chromium.launch({headless:true})
 const page=await browser.newPage()
 const dialogs=[]
 page.on('dialog',async dialog=>{dialogs.push(dialog.message());await dialog.dismiss()})
 await page.addInitScript(()=>{
  window.EventSource=class {onmessage=null; onerror=null; close(){}}
 })
 let memory=[{id:'manual',text:'Original memory',scope:'shared',source:'user',
  status:'active',retrievalEligible:true,createdAt:'2026-11-14T00:00:00Z'}]
 const edits=[]
 await page.route('**/api/**',async route=>{
  const request=route.request()
  const path=new URL(request.url()).pathname
  if(request.method()==='POST'&&path==='/api/memories/manual/edit'){
   const body=request.postDataJSON()
   edits.push(body)
   if(body.expectedText!==memory[0].text){
    await route.fulfill({status:409,contentType:'application/json',body:JSON.stringify({error:'Conflict'})})
    return
   }
   memory=[{...memory[0],text:body.text,verification:'manual_edit'}]
   await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(memory[0])})
   return
  }
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(getPayload(path,memory))})
 })
 await page.goto(base+'/#/connections')
 const tab=name=>page.getByRole('tab',{name,exact:true})
 await tab('MCP · 스킬').waitFor()
 assert.equal(await page.locator('nav button').filter({hasText:'연결 채널'}).count(),1)
 await tab('MCP · 스킬').click()
 assert.equal(await tab('MCP · 스킬').getAttribute('aria-selected'),'true')
 assert.ok(page.url().includes('section=extensions'))
 await tab('역량 마켓').click()
 assert.equal(await tab('역량 마켓').getAttribute('aria-selected'),'true')
 assert.ok(page.url().includes('section=capabilities'))
 await tab('채널 연결').click()
 assert.equal(await tab('채널 연결').getAttribute('aria-selected'),'true')

 await page.goto(base+'/#/extensions')
 await tab('MCP · 스킬').waitFor()
 assert.equal(await tab('MCP · 스킬').getAttribute('aria-selected'),'true')
 await page.goto(base+'/#/capabilities')
 await tab('역량 마켓').waitFor()
 assert.equal(await tab('역량 마켓').getAttribute('aria-selected'),'true')

 await page.goto(base+'/#/memory')
 const edit=page.getByRole('button',{name:'기억 수정'})
 await edit.waitFor()
 await edit.click()
 const textarea=page.getByRole('textbox',{name:'기억 수정'})
 await textarea.fill('Corrected memory')
 await page.getByRole('button',{name:'변경 저장'}).click()
 await page.getByText('Corrected memory').waitFor()
 assert.deepEqual(edits,[{text:'Corrected memory',expectedText:'Original memory'}])

 await page.goto(base+'/#/chat')
 await sleep(16_000) // more than the previous 15-second unsolicited-alert cadence
 assert.equal(await page.getByText('자기개선 한 번 해볼까요?').count(),0)
 assert.equal(dialogs.length,0)
 console.log('PASS: integrated tabs, old links, memory edits and quiet RSI (Chromium)')
}
try{await run()}finally{
 if(browser)await browser.close()
 if(server&&!server.killed)server.kill('SIGTERM')
}
