import test from 'node:test';
import assert from 'node:assert/strict';
import {taskPublicView,prediction,summarize,verifyManifest} from './sub2-swebench-verified.mjs';
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
test('no task, duplicate instances and invalid checkout commits are accepted',()=>{
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
 assert.equal(summarize([...rows,{patchBytes:0,usage:null}]).tokensPerGeneratedPatch,null);
});
