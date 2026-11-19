// Regression guard for the user-visible stop path.
// Native cancellation and model-SSE preemption are exercised by Rust HTTP tests.
import {test} from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
const root=fileURLToPath(new URL('../',import.meta.url))
const chat=readFileSync(root+'src/pages/Chat.tsx','utf8')
const ko=readFileSync(root+'src/i18n/ko.ts','utf8')
const en=readFileSync(root+'src/i18n/en.ts','utf8')

test('busy chat has a keyboard-accessible real stop control instead of a disabled spinner',()=>{
 assert.match(chat,/busy\?<Button[^>]*type="button"[^>]*onClick=\{\(\)=>void stopChat\(\)\}/)
 assert.match(chat,/aria-label=\{stopping\?t\('chat\.stopping'\):t\('chat\.stop'\)\}/)
 assert.match(chat,/<Square size=\{18\} fill="currentColor"\/>/)
 assert.doesNotMatch(chat,/disabled=\{busy\|\|Boolean\(selectedSession/)
})

test('STOP propagates to server before local abort and ignores late SSE events',()=>{
 const stop=chat.slice(chat.indexOf(' const stopChat=async()=>{'),chat.indexOf(' const executeCommand=async('))
 assert.ok(stop.indexOf("+'/cancel'")>=0)
 assert.ok(stop.indexOf('if(!outcome.stopped)')<stop.indexOf('controller?.abort()'))
 assert.match(chat,/if\(controller\.signal\.aborted\|\|stoppingRef\.current\|\|stream\.current!==controller\)return;/)
 assert.match(chat,/if\(name==='stop'\)\{await stopChat\(\);return;\}/)
 assert.match(chat,/refresh\('\/api\/sessions','\/api\/approvals'/)
})

test('both translations explain cancellation acknowledgement and pending shutdown',()=>{
 for(const pack of [ko,en]){
  for(const key of ['chat.stop','chat.stopping','chat.stopped','chat.stopPending']){
   assert.ok(pack.includes("'"+key+"':"),"missing "+key)
  }
 }
})
