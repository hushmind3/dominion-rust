import test from 'node:test';
import assert from 'node:assert/strict';
import {compareOfficialRuns} from './sub2-swebench-compare.mjs';

const SHA='a'.repeat(40);
const source='external_SWE_bench_docker_per_instance_reports';
function row(id,solved,tokens,estimated=0){
  return {
    instance_id:id,repo:'django/django',base_commit:SHA,
    patchBytes:24,officialSolved:solved,
    officialGradeSource:'official_swebench_docker_report',
    usage:{calls:2,totalTokens:tokens,estimatedCalls:estimated},
    toolLedger:{total:4},
  };
}
function report(rows){
  return {
    dataset:'princeton-nlp/SWE-bench_Verified',
    runId:'dominion-swe-123456-abcd1234',
    model:'DOMINION-Rust-model',
    instanceIds:rows.map(row=>row.instance_id),
    reproducibility:{binarySha256:'a'.repeat(64),
      maxIterations:16,maxExecutorTokens:80000,timeoutMinutes:20,
      provider:{model:'model',kind:'openai',baseUrl:'http://127.0.0.1:1234'}},
    officialGrading:{source,fullyGraded:true},
    receipts:rows,
  };
}
test('same-model, same-budget official cases get paired outcome comparison',()=>{
  const left=report([row('django__django-1',true,120),
    row('django__django-2',false,380)]);
  const right=report([row('django__django-2',true,250),
    row('django__django-1',false,150)]);
  const compared=compareOfficialRuns(left,right);
  assert.equal(compared.attempted,2);
  assert.equal(compared.harnessComparisonControlled,true);
  assert.deepEqual(compared.paired,{
    bothSolved:0,leftOnlySolved:1,
    rightOnlySolved:1,neitherSolved:0,netLeftAdvantage:0});
  assert.equal(compared.left.resolvedRate,0.5);
  assert.equal(compared.right.resolvedRate,0.5);
  assert.equal(compared.left.recordedTokens,500);
  assert.equal(compared.right.independentlyCountedTokens,400);
  assert.equal(compared.left.toolCalls,8);
});
test('different model or budget cannot masquerade as isolated harness win',()=>{
  const first=report([row('django__django-1',true,100)]);
  const second=report([row('django__django-1',false,100)]);
  second.reproducibility.provider.model='model-2';
  assert.equal(compareOfficialRuns(first,second).harnessComparisonControlled,false);
  second.reproducibility.provider.model='model';
  second.reproducibility.maxExecutorTokens=120000;
  assert.equal(compareOfficialRuns(first,second).harnessComparisonControlled,false);
  second.reproducibility.maxExecutorTokens=80000;
  second.reproducibility.provider.model='';
  assert.equal(compareOfficialRuns(first,second).harnessComparisonControlled,false);
});
test('no cherry-picking, fake proof, unknown grade, cross-base or missing usage evidence',()=>{
  const first=report([row('django__django-1',true,150),
    row('django__django-2',false,200)]);
  const second=report([row('django__django-1',false,180),
    row('django__django-2',false,250)]);
  assert.throws(()=>compareOfficialRuns(first,report([row('django__django-1',true,150)])));
  const poisoned=structuredClone(second);
  poisoned.receipts[0].officialGradeSource='model_success_claim';
  assert.throws(()=>compareOfficialRuns(first,poisoned));
  poisoned.receipts[0].officialGradeSource='official_swebench_docker_report';
  poisoned.receipts[1].officialSolved=null;
  assert.throws(()=>compareOfficialRuns(first,poisoned));
  poisoned.receipts[1].officialSolved=false;
  poisoned.receipts[0].base_commit='b'.repeat(40);
  assert.throws(()=>compareOfficialRuns(first,poisoned));
  const imperfect=structuredClone(second);
  imperfect.receipts[1].usage.estimatedCalls=2;
  imperfect.receipts[0].toolLedger=null;
  const compared=compareOfficialRuns(first,imperfect);
  assert.equal(compared.right.recordedTokens,430);
  assert.equal(compared.right.independentlyCountedTokens,null);
  assert.equal(compared.right.toolCalls,null);
});
