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

test('sidebar exposes 14 unique top-level items with a single connection entry',()=>{
 const navigation=nav.match(/export const navigation:[\s\S]*?\n\]/)?.[0]||''
 const ids=[...navigation.matchAll(/\{id:'([^']+)',label:'nav\.[^']+',icon:/g)].map(match=>match[1])
 assert.equal(ids.length,14)
 assert.equal(new Set(ids).size,14)
 assert.equal(ids.filter(id=>id==='connections').length,1)
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
