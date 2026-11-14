import test from "node:test";
import assert from "node:assert/strict";
import {mkdtempSync,mkdirSync,writeFileSync,symlinkSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {dirname,join} from "node:path";
import {oracle,summarize,verifiedGoal} from "./sub2-goal-eval.mjs";

test("independent file verifier grades real bytes, not a model claim",()=>{
  const root=mkdtempSync(join(tmpdir(),"dominion-goal-oracle-"));
  try{
    const path="eval-runs/task-a/result.txt";
    mkdirSync(dirname(join(root,path)),{recursive:true});
    writeFileSync(join(root,path),"EXPECTED\n");
    const check={type:"file_equals",path,content:"EXPECTED\n"};
    assert.equal(oracle(root,[check]).passed,true);
    assert.equal(oracle(root,[{...check,content:"EXPECTED"}]).passed,false);
    assert.equal(oracle(root,[{type:"file_contains",path,contains:"PECT"}]).passed,true);
    assert.equal(oracle(root,[{type:"file_absent",path}]).passed,false);
    assert.equal(oracle(root,[{type:"file_absent",path:"eval-runs/task-a/missing.txt"}]).passed,true);
    assert.equal(oracle(root,[]).passed,false);
    assert.equal(oracle(root,[{type:"file_exists",path:"../outside.txt"}]).passed,false);
    assert.equal(oracle(root,[{type:"file_exists",path:"/etc/passwd"}]).passed,false);
    symlinkSync(join(root,path),join(root,"eval-runs/task-a/link.txt"));
    assert.equal(oracle(root,[{type:"file_exists",path:"eval-runs/task-a/link.txt"}]).passed,false,
      "symlinked artifact must never count as independent proof");
  }finally{rmSync(root,{recursive:true,force:true})}
});

test("a stale, forged or no-action receipt cannot be a verified win",()=>{
  const good={jobId:"job-new",verifiedCompletion:true,actionEvidenceCount:2};
  const local={passed:true};
  assert.equal(verifiedGoal("completed",good,"job-new",local),true);
  assert.equal(verifiedGoal("needs_review",good,"job-new",local),false);
  assert.equal(verifiedGoal("completed",good,"job-old",local),false);
  assert.equal(verifiedGoal("completed",{...good,verifiedCompletion:false},"job-new",local),false);
  assert.equal(verifiedGoal("completed",{...good,actionEvidenceCount:0},"job-new",local),false);
  assert.equal(verifiedGoal("completed",good,"job-new",{passed:false}),false);
  assert.equal(verifiedGoal("completed",null,"job-new",local),false);
});

test("measured cost per verified result includes failed runs but refuses incomplete usage",()=>{
  const complete=[
    {verified:true,usage:{totalTokens:200,estimatedCalls:1}},
    {verified:false,usage:{totalTokens:700}},
    {verified:true,usage:{totalTokens:300}}
  ];
  const summary=summarize(complete);
  assert.equal(summary.passAt1,2/3);
  assert.equal(summary.recordedTotalTokens,1200);
  assert.equal(summary.recordedTokensPerVerifiedCompletion,600,
    "failed task tokens are part of total cost per solved task");
  assert.equal(summary.actualBilledKRW,null);
  const missing=summarize([...complete,{verified:false,usage:null}]);
  assert.equal(missing.missingUsageReports,1);
  assert.equal(missing.recordedTokensPerVerifiedCompletion,null,
    "incomplete usage cannot be presented as measured efficiency");
  assert.equal(summarize([]).passAt1,null);
});
