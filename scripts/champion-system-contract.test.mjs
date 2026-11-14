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

test('public SDK packages hit native Dominion API and preserve owner steering',()=>{
 check('sdk/typescript/src/client.ts',[
  '/api/sdk/threads','streamThread','threads.steer','client.threads.history',
  'new DominionStreamInterrupted'])
 check('sdk/typescript/src/transport.ts',[
  'X-Dominion-Token','decodeSse','async sse','abort'])
 check('sdk/typescript/tests/sdk.test.mjs',[
  'native SDK steering','native SDK stream','/api/sdk/threads'])
 check('sdk/rust/src/client.rs',[
  'create_sdk_thread','stream_sdk_thread','steer_sdk_thread',
  'run_native_streamed','run_native'])
 check('sdk/rust/src/mcp.rs',['pub struct ToolRegistry','serve_stdio'])
 check('sdk/rust/tests/sdk.rs',[
  'native_rust_sdk_thread_lifecycle_steers','native_rust_sdk_reports_actual_sse_error_message'])
})

test('RSI reflective feedback reaches the compiled descendant and never replaces the independent judge',()=>{
 check('src/engine/rsi_evolve.rs',['mod failure_feedback'])
 check('src/engine/rsi/learning/failure_feedback.rs',[
  'fn reflective_feedback','pairedMeasured','pairedRevalidated',
  'classification','rootCauseProven'])
 check('src/engine/rsi/execution/guards.rs',[
  'failure_feedback::reflective_feedback','measuredCounterexamplesDelivered'])
 check('src/engine/rsi/execution/worker.rs',[
  'rsi_system::revalidated_history','fit_worker_prompt'])
 check('src/engine/rsi/execution/generation_receipt.rs',['workerFeedbackPlan'])
 check('src/engine/rsi/evaluation/candidate_selection.rs',['rsi_worktree::compare_at'])
})

test('real coding evaluation uses complete Git patches and official task-level grading',()=>{
 check('src/api/mod.rs',['/api/goals/:id/patch','export_goal_patch'])
 check('src/engine/goal_git.rs',[
  'complete_export_patch','GIT_INDEX_FILE','--binary',
  'isolated_git_worktree_complete_patch_v2',
  'exact_export_includes_new_files_without_staging_original_index'])
 check('scripts/benchmark/swebench_dominion.py',[
  'princeton-nlp/SWE-bench_Verified',
  'official_score','resolved_ids','base_commit',
  'swebench.harness.run_evaluation','validated_patch'])
 check('scripts/benchmark/test_swebench_dominion.py',[
  'test_official_score_requires_exact_cohort',
  'test_complete_new_file_patch_is_admissible'])
})

test('real autonomous goals enforce last successful test, source SHA progress and bounded token spending',()=>{
 check('src/api/goals.rs',['requireTests','tokenBudget','token_budget'])
 check('src/engine/goals.rs',[
  'goal_test_gaps','observed_artifact_changes',
  'materially_advanced','diagnosis::classify',
  'strict_goal_rejects_model_claims',
  'failureDiagnosis','goal_token_budget'])
 check('src/engine/goals/progress.rs',[
  'observed_artifact_changes','strict_coding_progress_requires_real_artifact_change'])
 check('src/engine/goals/diagnosis.rs',[
  'fn classify','tool_execution','coding_test','rootCauseProven'])
 check('src/engine/verification.rs',[
  'fn is_code_validation_command','fn goal_test_gaps',
  'later_failed_test_invalidates_older_success',
  'fresh_code_validation'])
 check('src/db.rs',['fn session_execution_operations','LIMIT 1001'])
 check('src/engine/goal_control.rs',[
  'fn remaining_goal_tokens','goal_token_budget_exhausted',
  'aggregate_goal_budget_reads_real_model_usage'])
 check('src/engine/chat/runtime/turn.rs',[
  'goal.token_budget_exhausted','_dominion_frame'])
 check('src/engine/chat/context.rs',[
  'crashed_owner_steer_uses_persisted_frame_id'])
 check('scripts/benchmark/swebench_dominion.py',[
  'requireTests','tokenBudget','--token-budget',
  'tokenBudgetOverrunObserved'])
 check('sdk/typescript/src/types.ts',['requireTests'])
 check('sdk/rust/src/types.rs',['require_tests'])
})
