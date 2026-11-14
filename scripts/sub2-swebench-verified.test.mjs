import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
import {dirname,join} from 'node:path';
import {taskPublicView,prediction,summarize,verifyManifest,toolLedger,executionLedger,positiveBenchmarkLimit,attributedSessionIds,aggregateSessionUsage,importOfficialVerdicts,importOfficialRun} from './sub2-swebench-verified.mjs';
const issue={
 instance_id:'django__django-10000',
 repo:'django/django',
 base_commit:'a'.repeat(40),
 problem_statement:'A crash when using a valid form input.',
 patch:'REFERENCE PATCH MUST NEVER ENTER MODEL INPUT',
 test_patch:'HIDDEN TESTS MUST NEVER ENTER MODEL INPUT',
 FAIL_TO_PASS:'HIDDEN CASE IDS'
};
test('SWE-bench task projection never leaks official held-out patches',()=>{
 const projected=taskPublicView(issue);
 assert.deepEqual(Object.keys(projected),['instance_id','repo','base_commit','problem_statement']);
 assert.equal(JSON.stringify(projected).includes('HIDDEN'),false);
 assert.equal(JSON.stringify(projected).includes('REFERENCE'),false);
 assert.equal(JSON.stringify(projected).includes('FAIL_TO_PASS'),false);
});
test('prediction uses the actual generated Git diff, never self-reported success',()=>{
 const patch='diff --git a/a.py b/a.py\n--- a/a.py\n+++ b/a.py\n@@ -1 +1 @@\n-a\n+b\n';
 const pred=prediction(issue,patch,'DOMINION-rust');
 assert.equal(pred.model_patch,patch);
 assert.equal(pred.instance_id,issue.instance_id);
 assert.equal(pred.model_name_or_path,'DOMINION-rust');
});
test('task ids, duplicate instances, checkout revisions and path escapes are rejected',()=>{
 assert.throws(()=>taskPublicView({...issue,base_commit:'HEAD'}));
 assert.throws(()=>taskPublicView({...issue,repo:'../outside'}));
 assert.throws(()=>verifyManifest([issue,issue]));
});
test('success count stays null without the official Docker evaluator',()=>{
 const rows=[
  {patchBytes:30,goalStatus:'completed',usage:{totalTokens:200}},
  {patchBytes:0,goalStatus:'failed',usage:{totalTokens:600}},
  {patchBytes:20,goalStatus:'needs_review',usage:{totalTokens:200}}
 ];
 const result=summarize(rows);
 assert.equal(result.attempted,3);
 assert.equal(result.generatedPatches,2);
 assert.equal(result.nativeGoalCompleted,1);
 assert.equal(result.totalRecordedTokens,1000);
 assert.equal(result.tokensPerGeneratedPatch,500);
 assert.equal(result.officialResolved,null);
 assert.equal(result.officialResolveRate,null);
 const graded=rows.map((row,index)=>({...row,officialSolved:index===0}));
 const final=summarize(graded);
 assert.equal(final.officialResolved,1);
 assert.equal(final.officialResolveRate,1/3);
 assert.equal(final.tokensPerOfficialResolution,1000);
 assert.equal(final.actualBilledKRW,null);
 assert.equal(summarize([...rows,{patchBytes:0,usage:null}]).tokensPerGeneratedPatch,null);
});

test('tool calls use persisted Rust operation IDs; failed and unknown calls count',()=>{
  const rows=[
    {id:'op-1',status:'completed',toolReceipt:{verified:true},effectState:'verified'},
    {id:'op-2',status:'failed',result:{outcomeUnknown:true},effectState:'effect_unknown'},
    {id:'op-3',status:'completed',toolReceipt:{verified:false},effectState:'applied'},
  ];
  const summary=toolLedger(rows);
  assert.deepEqual(summary,{
    total:3,completed:2,failed:1,unknown:1,verified:1,blocked:0,recovered:false,
  });
  assert.deepEqual(executionLedger([{operations:rows.slice(0,2)},{operations:rows.slice(2)}]),summary);
  assert.throws(()=>toolLedger([...rows,rows[0]]),'reusing one operation ID is not a new tool call');
  assert.throws(()=>executionLedger([{operations:null}]),'missing ledger cannot be zero calls');
});
test('only official grader can certify recovery and tool cost per solved task',()=>{
  const trial=[
    {usage:{totalTokens:100},toolLedger:{total:3,failed:1,unknown:0},officialSolved:true},
    {usage:{totalTokens:300},toolLedger:{total:7,failed:2,unknown:1},officialSolved:false},
    {usage:{totalTokens:200},toolLedger:{total:2,failed:0,unknown:0},officialSolved:true},
  ];
  const report=summarize(trial);
  assert.equal(report.toolCalls,12);
  assert.equal(report.toolErrors,3);
  assert.equal(report.unknownEffects,1);
  assert.equal(report.toolCallsPerOfficialResolution,6);
  assert.equal(report.officialRecoveryRate,0.5);
  assert.equal(summarize(trial.map(row=>({...row,officialSolved:null}))).officialRecoveryRate,null);
  assert.equal(summarize([...trial,{usage:null,toolLedger:null}]).toolCalls,null);
});

test('real benchmark budgets are exact bounded integers, never silently clamped',()=>{
  assert.equal(positiveBenchmarkLimit('80000','tokens',5000,2000000),80000);
  assert.equal(positiveBenchmarkLimit('64','iterations',1,64),64);
  for(const invalid of [0,-1,2.5,'abc',null,Infinity,Number.MAX_SAFE_INTEGER+2]){
    assert.throws(()=>positiveBenchmarkLimit(invalid,'iterations',1,64),
      'invalid budget '+String(invalid)+' must stop a paid comparison');
  }
  assert.throws(()=>positiveBenchmarkLimit('2000001','tokens',5000,2000000));
  assert.throws(()=>positiveBenchmarkLimit('4999','tokens',5000,2000000));
});

test('replan recovery is counted only when independent grading and native evidence exist',()=>{
  const rows=[
    {patchBytes:10,officialSolved:true,usage:{totalTokens:100},
      toolLedger:{total:4,failed:2},recoveryEvidence:{replanCount:1}},
    {patchBytes:10,officialSolved:false,usage:{totalTokens:300},
      toolLedger:{total:9,failed:1},recoveryEvidence:{replanCount:2}},
    {patchBytes:10,officialSolved:true,usage:{totalTokens:200},
      toolLedger:{total:1,failed:0},recoveryEvidence:{replanCount:0}},
  ];
  const measured=summarize(rows);
  assert.equal(measured.observedReplans,3);
  assert.equal(measured.officialResolvedAfterReplanRate,0.5);
  assert.equal(measured.actualBilledKRW,null);
  const ungraded=summarize(rows.map(row=>({...row,officialSolved:null})));
  assert.equal(ungraded.officialResolvedAfterReplanRate,null);
  const missing=summarize([...rows,{patchBytes:0,usage:null,toolLedger:null,
    recoveryEvidence:null,officialSolved:false}]);
  assert.equal(missing.observedReplans,null);
  assert.equal(missing.officialResolvedAfterReplanRate,null);
});

test('delegated child-agent work counts in the same real goal and unrelated sessions do not',()=>{
 const sessions=[
   {id:'root',goalId:'goal-a'},
   {id:'child-a',parentSessionId:'root'},
   {id:'child-b',parentSessionId:'child-a'},
   {id:'unrelated',goalId:'goal-other'},
   {id:'unrelated-child',parentSessionId:'unrelated'},
 ];
 const ids=attributedSessionIds('root',sessions);
 assert.deepEqual([...ids].sort(),['child-a','child-b','root']);
 assert.equal(ids.has('unrelated-child'),false);
 const usage=aggregateSessionUsage([
   {calls:2,promptTokens:120,completionTokens:30,totalTokens:150,estimatedCalls:0},
   {calls:1,promptTokens:70,completionTokens:30,totalTokens:100,estimatedCalls:1},
   {calls:3,promptTokens:300,completionTokens:40,totalTokens:340,estimatedCalls:0},
 ]);
 assert.deepEqual(usage,{
   calls:6,promptTokens:490,completionTokens:100,totalTokens:590,estimatedCalls:1,
 });
 assert.throws(()=>attributedSessionIds('root',sessions.slice(1)));
 assert.throws(()=>attributedSessionIds('root',[...sessions,sessions[0]]));
 assert.throws(()=>aggregateSessionUsage([{calls:0,totalTokens:12}]));
});

test('independently imported Docker verdicts require unchanged native predictions',()=>{
 const root=mkdtempSync(join(tmpdir(),'dominion-swe-grader-'));
 try{
  const model='DOMINION-Rust-local-test';
  const runId='dominion-swe-123456-abcd1234';
  const patch='diff --git a/example.py b/example.py\n--- a/example.py\n+++ b/example.py\n@@ -1 +1 @@\n-old\n+new\n';
  const digest=createHash('sha256').update(patch).digest('hex');
  const rows=[
   {instance_id:'django__django-10000',patchSha256:digest,
     patchBytes:Buffer.byteLength(patch),goalStatus:'completed',usage:{totalTokens:100}},
   {instance_id:'sympy__sympy-2000',patchSha256:null,patchBytes:0,
     goalStatus:'failed',usage:{totalTokens:50}},
  ];
  const preds=[
   {instance_id:rows[0].instance_id,model_patch:patch,model_name_or_path:model},
   {instance_id:rows[1].instance_id,model_patch:'',model_name_or_path:model},
  ];
  const logs=join(root,'logs','evaluation',runId);
  const evidence=join(logs,model,rows[0].instance_id,'report.json');
  mkdirSync(dirname(evidence),{recursive:true});
  writeFileSync(evidence,JSON.stringify({[rows[0].instance_id]:{resolved:true}}));
  const trial=rows.map(row=>({...row}));
  importOfficialVerdicts(trial,preds,model,logs);
  assert.equal(trial[0].officialSolved,true);
  assert.equal(trial[1].officialSolved,false);
  assert.equal(summarize(trial).officialResolveRate,0.5);
  assert.throws(()=>importOfficialVerdicts(rows,
    [{...preds[0],model_patch:patch.replace('new','bad')},preds[1]],
    model,logs),'same-sized tampered diffs cannot inherit earlier Docker results');
  const reportPath=join(root,'report.json');
  writeFileSync(reportPath,JSON.stringify({
   runId,dataset:'princeton-nlp/SWE-bench_Verified',
   model,receipts:rows,summary:null
  }));
  writeFileSync(join(root,'predictions.jsonl'),
    preds.map(pred=>JSON.stringify(pred)).join('\n')+'\n');
  const imported=importOfficialRun(reportPath,logs);
  assert.equal(imported.summary.officialResolved,1);
  assert.equal(imported.officialGrading.fullyGraded,true);
  const saved=JSON.parse(readFileSync(reportPath,'utf8'));
  assert.equal(saved.receipts[0].officialGradeSource,'official_swebench_docker_report');
  assert.equal(saved.receipts[1].officialGradeSource,'empty_prediction_not_evaluated');
  rmSync(evidence);
  const partial=importOfficialRun(reportPath,logs);
  assert.equal(partial.officialGrading.fullyGraded,false);
  assert.equal(partial.summary.officialResolveRate,null,
    'no result must never become an official unresolved task by guesswork');
  assert.equal(partial.summary.officialResolved,null);
 }finally{rmSync(root,{recursive:true,force:true})}
});
