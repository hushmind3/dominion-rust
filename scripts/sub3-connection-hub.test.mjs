// Source-contract regressions. Does not replace a real browser/API smoke run.
// Run: node --test scripts/sub3-connection-hub.test.mjs
import {readFileSync,existsSync} from 'node:fs'
import {join} from 'node:path'
import {fileURLToPath} from 'node:url'
import {test} from 'node:test'
import assert from 'node:assert/strict'

const root=join(fileURLToPath(new URL('..',import.meta.url)))
const source=path=>readFileSync(join(root,path),'utf8')
const shell=source('web/src/components/WorkspaceShell.tsx')
const app=source('web/src/App.tsx')
const hub=source('web/src/pages/Connections.tsx')
const chat=source('web/src/pages/Chat.tsx')

test('one sidebar entry owns all three connection capabilities',()=>{
 const navigation=shell.split('export const navigation:')[1]?.split('const items=')[0]||''
 assert.equal((navigation.match(/id:'[^']+'/g)||[]).length,15,'all 14 baseline entries plus autonomous goals remain')
 assert.equal((navigation.match(/id:'connections'/g)||[]).length,1)
 assert.doesNotMatch(navigation,/id:'extensions'|id:'capabilities'/)
 assert.match(navigation,/id:'memory'/)
 assert.match(hub,/채널 연결/)
 assert.match(hub,/MCP · 스킬/)
 assert.match(hub,/역량 마켓/)
 assert.match(hub,/function ChannelConnectionsPanel\(/)
})

test('legacy deep links reach the correct connection tab',()=>{
 assert.match(app,/const legacy=name==='extensions'\|\|name==='capabilities'/)
 assert.match(app,/page:\(legacy\?'connections'/)
 assert.match(app,/connectionSection=/)
 assert.match(app,/<ConnectionsPage initialSection=\{connectionSection\}/)
 assert.match(hub,/useEffect\(\(\)=>setSection\(initialSection\),\[initialSection\]\)/)
 assert.match(hub,/#\/connections.*section=/)
})

test('hidden extension pages load on demand without dropping installation APIs',()=>{
 assert.match(hub,/lazy\(\(\)=>import\('\.\/Extensions'\)/)
 assert.match(hub,/lazy\(\(\)=>import\('\.\/Capabilities'\)/)
 const extensions=source('web/src/pages/Extensions.tsx')
 const capabilities=source('web/src/pages/Capabilities.tsx')
 for(const endpoint of ['/api/mcp','/api/skills','/api/skill-catalog','/api/mcp/catalog'])
  assert.ok(extensions.includes(endpoint),endpoint)
 for(const endpoint of ['/api/capabilities','/api/channels/catalog'])
  assert.ok(capabilities.includes(endpoint),endpoint)
 assert.ok(hub.includes('/api/channels/catalog'))
 assert.ok(hub.includes('/api/oauth/connections'))
})

test('autonomous RSI alert and 15-second polling are removed, explicit repair stays',()=>{
 assert.doesNotMatch(app,/ImprovementNotice/)
 assert.equal(existsSync(join(root,'web/src/components/ImprovementNotice.tsx')),false)
 const manual=source('web/src/components/ImprovementIssue.ts')
 assert.doesNotMatch(manual,/setInterval|rsi\/signals|Notification|toast/)
 assert.match(manual,/\/api\/rsi\/issues/)
 assert.match(chat,/import \{openImprovementIssue\} from ["']\.\.\/components\/ImprovementIssue["']/)
 assert.match(chat,/자기개선으로 수정/)
})

test('memory still appears as a first-class connected product feature',()=>{
 assert.match(app,/<MemoryPage\/>/)
 const page=source('web/src/pages/Memory.tsx')
 assert.match(page,/\/api\/memories/)
 assert.match(page,/LearningRecords/)
 assert.match(page,/GrowthDashboard/)
})

test('autonomous goals are discoverable and durable resume requires owner review',()=>{
 assert.match(shell,/id:'goals',label:'nav.goals'/)
 assert.match(app,/page==='goals'&&<GoalsPage\/>/)
 const goals=source('web/src/pages/Goals.tsx')
 const workbench=source('web/src/components/GoalWorkbench.tsx')
 const api=source('src/api/goals.rs')
 const routes=source('src/api/mod.rs')
 assert.match(goals,/GoalWorkbench/)
 assert.match(workbench,/acknowledgeUnknownEffects:true/)
 assert.match(api,/pub async fn resume_goal/)
 assert.match(api,/goal.owner_reviewed_resume/)
 assert.match(routes,/\/api\/goals\/:id\/resume/)
})

test('owner memory editing enforces transaction, provenance and compare-and-swap',()=>{
 const api=source('src/api/collections.rs')
 const db=source('src/db.rs')
 const memory=source('web/src/pages/Memory.tsx')
 assert.match(api,/state.store.edit_owner_memory/)
 assert.match(api,/body.get\("id"\).is_some\(\)/)
 assert.match(db,/pub fn edit_owner_memory/)
 assert.match(db,/current\["source"\] != "user"/)
 assert.match(db,/current\["status"\] != "active"/)
 assert.match(db,/INSERT INTO audit\(id,event,detail\)/)
 assert.match(memory,/expectedText:editing.expectedText/)
 assert.match(memory,/m.source==='user'/)
})


test('LTX V2 film path preserves Omni and ComfyUI while guarding paid calls',()=>{
 const engine=source('src/engine/video.rs')
 const api=source('src/api/video.rs')
 const routes=source('src/api/mod.rs')
 const ltx=source('src/engine/video/ltx.rs')
 const film=source('src/engine/video/film.rs')
 const pricing=source('src/engine/video/pricing.rs')
 const ui=source('web/src/pages/Video.tsx')
 assert.match(engine,/mod omni;/)
 assert.match(engine,/mod cinematic;/)
 assert.match(engine,/mod ltx;/)
 assert.match(engine,/mod film;/)
 assert.match(engine,/mod pricing;/)
 assert.match(engine,/confirmedPaidCallCount/)
 assert.match(engine,/approvedPaidQuote/)
 assert.match(engine,/maxEstimatedKrw/)
 assert.match(engine,/pricing::quote/)
 assert.match(api,/pub async fn price_quote/)
 assert.match(routes,/\/api\/video\/quote/)
 assert.match(routes,/\/api\/video\/jobs\/:id\/retake/)
 assert.match(ltx,/https:\/\/api\.ltx\.io\/v2\/text-to-video/)
 assert.match(ltx,/video:ltx-api/)
 assert.match(ltx,/video.paid_submit_attempt/)
 assert.match(film,/file_sha/)
 assert.match(film,/persist_receipt/)
 assert.match(film,/reuse_other_shots/)
 assert.match(pricing,/FX_AS_OF/)
 assert.match(ui,/FilmRetake/)
 assert.match(ui,/approvedPaidQuote:true/)
})

test('paid retake uses SHA, exact paid call count and frozen KRW ceiling',()=>{
 const engine=source('src/engine/video.rs')
 const film=source('src/engine/video/film.rs')
 const retake=source('web/src/pages/video/FilmRetake.tsx')
 assert.match(engine,/film::retake_input/)
 assert.match(engine,/newPaidDurations/)
 assert.match(engine,/quote_durations/)
 assert.match(film,/expectedSourceSha256/)
 assert.match(film,/receiptVerified/)
 assert.match(retake,/expectedSha256:shot.outputSha256/)
 assert.match(retake,/confirmedPaidCallCount:shot.expectedPaidCalls/)
 assert.match(retake,/maxEstimatedKrw:shot.estimatedRetakeKrw/)
})
