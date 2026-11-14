import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import path from 'node:path'

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const source=(p)=>readFileSync(path.join(root,p),'utf8')
const shell=source('web/src/components/WorkspaceShell.tsx')
const connections=source('web/src/pages/Connections.tsx')
const extensions=source('web/src/pages/Extensions.tsx')
const market=source('web/src/pages/Capabilities.tsx')
const app=source('web/src/App.tsx')
const rsi=source('web/src/pages/SelfImprovement.tsx')
const memory=source('web/src/pages/Memory.tsx')
const collections=source('src/api/collections.rs')
const router=source('src/api/mod.rs')
const notice=source('web/src/components/ImprovementNotice.tsx')
const goals=source('web/src/pages/AutonomousGoals.tsx')
const goalApi=source('src/api/goals.rs')
const goalJobs=source('src/engine/jobs.rs')
const store=source('src/db.rs')

test('one public connections sidebar item, retaining memory, RSI and champion goals',()=>{
 const nav=shell.match(/export const navigation:[\s\S]*?\n\]/)?.[0]
 assert.ok(nav,'navigation declaration must be reachable')
 assert.equal((nav.match(/id:'connections'/g)||[]).length,1)
 assert.doesNotMatch(nav,/id:'extensions'|id:'capabilities'/)
 for(const id of ['memory','rsi','goals'])assert.match(nav,new RegExp("id:'"+id+"'"))
 assert.match(app,/page==='connections'&&<ConnectionsPage\/>/)
 assert.match(app,/page==='goals'&&<AutonomousGoalsPage/)
})

test('connections contains functional tabs, not only renamed menu links',()=>{
 assert.match(connections,/section==='channels'/)
 assert.match(connections,/section==='extensions'/)
 assert.match(connections,/section==='capabilities'/)
 assert.match(connections,/setSection\('channels'\)/)
 assert.match(connections,/setSection\('extensions'\)/)
 assert.match(connections,/setSection\('capabilities'\)/)
 assert.match(connections,/role="tablist"/)
 assert.equal((connections.match(/role="tab" aria-selected=/g)||[]).length,3)
 assert.match(connections,/<ExtensionsPage\/>/)
 assert.match(connections,/<CapabilitiesPage navigate=/)
})

test('existing channel, OAuth, MCP, skills, plugin and capability endpoints remain wired',()=>{
 for(const endpoint of ['/api/channels/catalog','/api/channels','/api/oauth/connections'])
  assert.ok(connections.includes(endpoint),endpoint)
 for(const endpoint of ['/api/mcp','/api/skills','/api/mcp/discover','/api/skill-catalog','/api/skill-catalog/import'])
  assert.ok(extensions.includes(endpoint),endpoint)
 for(const endpoint of ['/api/capabilities','/api/channels/catalog'])
  assert.ok(market.includes(endpoint),endpoint)
 assert.match(extensions,/<SkillsPanel/)
})

test('no unsolicited self-improvement prompt, background poll or screen overlay',()=>{
 for(const [name,file] of [['app',app],['shell',shell],['connections',connections]]){
  assert.doesNotMatch(file,/\bImprovementNotice\b/,'no global RSI notice in '+name)
  assert.doesNotMatch(file,/\/api\/rsi\/signals/,'no implicit RSI signal polling in '+name)
 }
 assert.match(notice,/function ImprovementNotice\(\)\{return null\}/)
 assert.doesNotMatch(notice,/setInterval|setTimeout|\/api\/rsi\/signals|role="status"|role="alert"/)
 // Preserve manual issue creation and inline genuine error/status access.
 assert.match(notice,/openImprovementIssue/)
 assert.match(rsi,/<Notice message=\{error/)
 assert.match(rsi,/campaign\?\.error/)
 assert.match(app,/page==='rsi'&&<SelfImprovementPage\/>/)
})


test('manual memory edits use restricted atomic CAS endpoint',()=>{
 assert.match(memory,/memory\.source!=='user'/)
 assert.match(memory,/statusOf\(m\)==='active'/)
 assert.match(memory,/expectedText:memory\.text/)
 assert.match(memory,/expectedRevision:memory\.revision\?\?0/)
 assert.match(memory,/post\('\/api\/memories\/'\+encodeURIComponent\(memory\.id\)\+'\/edit'/)
 assert.doesNotMatch(memory,/\.\.\.stored|editedAt:new Date/)
 assert.match(collections,/\("memory", "edit"\)/)
 assert.match(collections,/state\.store\.edit_manual_memory/)
 assert.match(collections,/put_if_absent\("memory"/)
 assert.match(store,/pub fn edit_manual_memory/)
 assert.match(store,/record\["source"\] != "user"/)
 assert.match(store,/record\["text"\]\.as_str\(\) != Some\(expected\)/)
 assert.match(store,/record\["revision"\]\.as_u64\(\)\.unwrap_or\(0\) != expected_revision/)
 assert.match(store,/fields\.remove\(field\)/)
 assert.match(router,/get\(collections::one\)[\s\S]*?post\(collections::save\)/)
})

test('reviewed autonomous goal resumes from durable job checkpoint without replay',()=>{
 assert.match(goals,/\+'\/resume'/)
 assert.match(goals,/acknowledgeUnknownEffects:true/)
 assert.match(router,/\/api\/goals\/:id\/resume/)
 assert.match(goalApi,/pub async fn resume_goal/)
 assert.match(goalApi,/old_checkpoint\["inFlight"\]/)
 assert.match(goalApi,/enqueue_goal_resume/)
 assert.match(goalJobs,/pub fn enqueue_goal_resume/)
 assert.match(goalJobs,/put_if_absent\("job"/)
 assert.match(goalApi,/goal\.owner_reviewed_resume/)
 assert.match(goalApi,/goal\.enqueue_failed/)
})
