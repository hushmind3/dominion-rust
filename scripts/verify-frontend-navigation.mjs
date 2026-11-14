// Source-contract regression for the integrated navigation. This does not
// replace browser interaction tests or prove backend operations succeeded.
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import assert from 'node:assert/strict'

const read=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8')
const nav=read('web/src/components/WorkspaceShell.tsx')
const app=read('web/src/App.tsx')
const connections=read('web/src/pages/Connections.tsx')
const memory=read('web/src/pages/Memory.tsx')
const api=read('src/api/mod.rs')
const collections=read('src/api/collections.rs')

test('sidebar exposes 14 meaningful entries without a redundant Goal page',()=>{
 const navigation=nav.match(/export const navigation:[\s\S]*?\n\]/)?.[0]||''
 const ids=[...navigation.matchAll(/\{id:'([^']+)',label:'nav\.[^']+',icon:/g)].map(match=>match[1])
 assert.equal(ids.length,14)
 assert.equal(new Set(ids).size,15)
 assert.equal(ids.filter(id=>id==='connections').length,1)
 assert.ok(!ids.includes('goals'),'Goal is a chat command, not another sidebar product menu')
 for(const hidden of ['extensions','capabilities'])assert.equal(ids.includes(hidden),false)
 for(const required of ['memory','rsi','video','rooms','models','agents','developer','workflows'])
  assert.ok(ids.includes(required),'missing '+required)
})

test('connection hub composes the real MCP/skill and capability pages',()=>{
 for(const required of ["import {ExtensionsPage}","import {CapabilitiesPage}","<ExtensionsPage/>","<CapabilitiesPage",
 "채널 연결","MCP · 스킬","역량 마켓",'role="tablist"','role="tabpanel"',
 "'/api/channels'","'/api/oauth/connections'"])
  assert.ok(connections.includes(required),'missing connection binding '+required)
 for(const route of ['/api/channels','/api/mcp/catalog','/api/skills/import-repo','/api/capabilities'])
  assert.ok(api.includes(route),'missing existing backend route '+route)
})

test('historic deep links stay in the merged connection page',()=>{
 assert.ok(app.includes("legacy?'connections'"))
 assert.ok(app.includes("initialSection={connectionSection}"))
 assert.ok(connections.includes("'#/connections?section='+next"))
 assert.ok(!app.includes("{page==='extensions'&&<ExtensionsPage"))
 assert.ok(!app.includes("{page==='capabilities'&&<CapabilitiesPage"))
})

test('RSI suggestions never mount as a global unsolicited popup',()=>{
 const entry=read('web/src/main.tsx')
 assert.ok(!/\bImprovementNotice\b|\bopenImprovementIssue\b/.test(entry+app))
 assert.ok(app.includes("{page==='rsi'&&<SelfImprovementPage/>}"))
 assert.ok(!/window\.Notification\s*\(|new\s+Notification\s*\(/.test(entry+app))
})

test('manual memory edits have a verified server path, not a decorative control',()=>{
 assert.ok(memory.includes("+'/edit'"))
 assert.ok(memory.includes("m.source==='user'"))
 assert.ok(memory.includes('expectedText'))
 assert.ok(collections.includes('("memory", "edit")'))
 assert.ok(collections.includes('StatusCode::CONFLICT'))
 assert.ok(collections.includes('StatusCode::FORBIDDEN'))
})

test('slash-goal engine persists while workflow hosts the optional review console',()=>{
 const workflows=read('web/src/pages/Workflows.tsx')
 const goalConsole=read('web/src/pages/workflows/GoalConsole.tsx')
 const goalsAPI=read('src/api/goals.rs')
 const runner=read('src/engine/goals.rs')
 assert.ok(!app.includes("page==='goals'"))
 assert.ok(workflows.includes('<GoalConsole openSession={openSession}/>'))
 assert.ok(goalConsole.includes("+'/cancel'"))
 assert.ok(goalConsole.includes("+'/resume'"))
 assert.ok(goalConsole.includes('acknowledgeUnknownEffects:true'))
 assert.ok(goalConsole.includes('tokenBudget'))
 for(const route of ['/api/goals/:id/cancel','/api/goals/:id/resume'])
  assert.ok(api.includes(route),'missing '+route)
 assert.ok(goalsAPI.includes('transaction_if_status'))
 assert.ok(runner.includes('resumeCheckpoint'))
 assert.ok(runner.includes('expected_status'))
})

test('memory edits are one atomic store transaction including provenance guard',()=>{
 const db=read('src/db.rs')
 assert.ok(collections.includes('edit_manual_memory(&id, expected, text)'))
 assert.ok(db.includes('pub fn edit_manual_memory'))
 assert.ok(db.includes('TransactionBehavior::Immediate'))
 assert.ok(db.includes('record["source"] != "user"'))
 assert.ok(db.includes('record["status"]'))
 assert.ok(db.includes('"memory.manual_edit"')||db.includes("'memory.manual_edit'"))
})

test('paid LTX video and partial retake use server-priced, digest-bound consent',()=>{
 const video=read('web/src/pages/Video.tsx')
 const retake=read('web/src/pages/video/FilmRetake.tsx')
 const engine=read('src/engine/video.rs')
 const film=read('src/engine/video/film.rs')
 const pricing=read('src/engine/video/pricing.rs')
 const routes=read('src/api/mod.rs')
 assert.ok(routes.includes('/api/video/quote'))
 assert.ok(video.includes('/api/video/quote'))
 assert.ok(video.includes('approvedPaidQuote:true'))
 assert.ok(video.includes('maxUsdCents:paidQuote?.totalUsdCents'))
 assert.ok(retake.includes('retakeQuote.totalUsdCents'))
 assert.ok(engine.includes('pricing::approved(body,&quote)'))
 assert.ok(engine.includes('film::retake_quote'))
 assert.ok(film.includes('receiptVerified'))
 assert.ok(pricing.includes('quoteDigest'))
 assert.ok(pricing.includes('https://docs.ltx.io/pricing'))
})

test('goal replans use real source-edit receipts and acceptance improvement',()=>{
 const engine=read('src/engine/goals.rs')
 const progress=read('src/engine/goals/progress.rs')
 const verify=read('src/engine/verification.rs')
 assert.ok(engine.includes('progress::measure('))
 assert.ok(engine.includes('turn_progress.advanced()'))
 assert.ok(engine.includes('stalled.saturating_add(1)'))
 assert.ok(progress.includes('verification::is_action_evidence'))
 assert.ok(progress.includes('cleared_checks'))
 assert.ok(verify.includes('"workspace_replace_function" => true'))
})

test('issue 9: agent -> installed skill -> workflow -> persisted deliverable has live endpoints',()=>{
 const routes=read('src/api/mod.rs')
 const api=read('src/api/workflows.rs')
 const engine=read('src/engine/workflows.rs')
 const context=read('src/engine/chat/context.rs')
 const agents=read('web/src/pages/Agents.tsx')
 const models=read('web/src/pages/Models.tsx')
 const workflow=read('web/src/pages/Workflows.tsx')
 const developer=read('web/src/pages/Developer.tsx')
 const home=read('web/src/pages/Home.tsx')
 for(const path of ['/api/workflows','/api/workflows/:id/run',
   '/api/workflows/deliverables/:id','/api/skills']){
  assert.ok(routes.includes(path),path+' needs an actual API route')
 }
 assert.ok(api.includes('jobs::enqueue_once'))
 assert.ok(api.includes('runnable(&state,&workflow)'))
 assert.ok(engine.includes('run_quiet_outcome'))
 assert.ok(engine.includes('workflow_deliverable'))
 assert.ok(engine.includes('check_acceptance('))
 assert.ok(context.includes('preferredSkillIds'))
 assert.ok(agents.includes('preferredSkillIds'))
 assert.ok(agents.includes('#/workflows?agent='))
 assert.ok(models.includes('/api/agents/bulk-model'))
 assert.ok(workflow.includes('DeliverableDialog'))
 assert.ok(workflow.includes('requestId'))
 assert.ok(developer.includes('/api/workflows/'))
 assert.ok(home.includes('readyAgents'))
})

test('official Qwen-Agent SDK is an actual opt-in job, not a repository link',()=>{
 const routes=read('src/api/jobs.rs')
 const dispatch=read('src/engine/jobs.rs')
 const bridge=read('src/engine/sdk_local.rs')
 const sdk=read('scripts/agent_sdk/qwen_agent_runner.py')
 const developer=read('web/src/pages/Developer.tsx')
 for(const marker of ['"sdk_local"','sdk_local::authorize','enqueue_once'])
  assert.ok(routes.includes(marker),'missing SDK admission '+marker)
 assert.ok(dispatch.includes('"sdk_local" => crate::engine::sdk_local::run'))
 assert.ok(bridge.includes('approvedLocalWrite'))
 assert.ok(bridge.includes('rust_disk_sha256'))
 assert.ok(bridge.includes('check_acceptance('))
 assert.ok(sdk.includes('from qwen_agent.agents import Assistant'))
 assert.ok(sdk.includes('@register_tool("dominion_write")'))
 assert.ok(developer.includes('kind:\'sdk_local\''))
 assert.ok(developer.includes('setSdkApproved'))
 assert.ok(!nav.includes("{id:'goals'"),'Goal remains a chat command, not a separate product menu')
})
