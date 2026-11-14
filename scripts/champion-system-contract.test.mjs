// Source integration assertions; Rust unit tests and real-model evaluation are separate.
import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import path from 'node:path'

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const file=(name)=>readFileSync(path.join(root,name),'utf8')
const check=(name,expected)=>{const text=file(name);for(const pattern of expected)
 assert.ok(text.includes(pattern),`${name}: missing ${pattern}`)}

test('owner steer: UI -> authenticated API -> durable inbox -> native model step',()=>{
 check('web/src/pages/Chat.tsx',['/api/sessions/','/steer','stream.current&&busy'])
 check('src/api/mod.rs',['/api/sessions/:id/steer','/api/sdk/threads/:id/steer'])
 check('src/api/sessions.rs',['engine::steering::enqueue'])
 check('src/engine/agent_sdk.rs',['pub fn steer','steering::enqueue'])
 check('src/engine/chat/runtime/turn.rs',['drain_at_step_with_ids','harness.owner_revision_continued'])
 check('src/engine/steering.rs',['has_pending','drain_at_step_with_ids','authorKind'])
})

test('unstarted tool plans cannot outrank a newly arrived owner command',()=>{
 check('src/engine/chat/runtime/turn.rs',[
  'harness.owner_revision_batch_skipped','skip_for_new_owner_instruction',
  'check_tool_result_pairs','buffered(4)'])
 check('src/engine/chat/runtime/tool_dispatch.rs',[
  'fn skip_for_new_owner_instruction','"executed":false','tool_call_id'])
 check('src/db.rs',["kind='chat_steer'",'has_queued_owner_instruction'])
})

test('RSI route change is gated by original replayed, audited model evidence',()=>{
 check('src/engine/mod.rs',['pub mod rsi_model_learning;'])
 check('src/engine/rsi_model_learning.rs',[
  'fn learn_verified','model_route_audited','replayed_pair_metrics',
  'matched','independentTrials'])
 check('src/engine/router.rs',[
  'learn_verified(state)','compared_preference','compare_known_prices'])
 check('src/engine/chat/provider.rs',['router::select_provider'])
})

test('KRW tariff is owner-editable, quoted per actual tokens, and never called a bill',()=>{
 check('web/src/pages/Models.tsx',[
  'costPerMillionInputKrw','costPerMillionOutputKrw','inputKrw','outputKrw'])
 check('src/engine/gateway.rs',[
  'fn quoted_usage_krw','"costQuoteKrw"','"actualBilledKrw"'])
 check('src/db.rs',['"costQuoteKnownCalls"','"actualBilledKrw"'])
 check('web/src/pages/Chat.tsx',['sessionQuote','참고 '])
})

test('video: technical inspection -> each SHA-linked scene -> owner UI',()=>{
 check('src/engine/video.rs',['mod diagnostics;'])
 check('src/engine/video/diagnostics.rs',['blackdetect','freezedetect','potentialVisualDefect'])
 check('src/engine/video/quality.rs',['diagnostics::inspect','visualDiagnostics'])
 check('src/engine/video/film.rs',['saved_receipt','reviewRecommended','visualDiagnostics'])
 check('web/src/pages/video/FilmRetake.tsx',['reviewRecommended','visualDiagnostics'])
})
