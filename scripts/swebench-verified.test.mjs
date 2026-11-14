import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, writeFile, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {capturePatch,validateInstance,prediction,metrics,parseJsonl} from './swebench-verified.mjs';

const fixture = {
  instance_id:'django__django-12345',
  repo:'django/django',
  base_commit:'0123456789abcdef0123456789abcdef01234567',
  problem_statement:'Fix the regression in the repository',
  test_patch:'SECRET_HIDDEN_TEST_DO_NOT_EXPOSE',
  hints_text:'not part of the fair default agent prompt'
};

function git(dir,...args){
  const result=spawnSync('git',['-C',dir,...args],{
    encoding:'utf8',env:{...process.env,GIT_TERMINAL_PROMPT:'0'}
  });
  assert.equal(result.status,0,result.stderr);
  return result.stdout.trim();
}

test('official instance excludes hidden test patch, hints and internal reference answer',()=>{
  const item=validateInstance(fixture);
  assert.deepEqual(Object.keys(item).sort(),[
    'base_commit','instance_id','problem_statement','repo'
  ]);
  assert.ok(!JSON.stringify(item).includes('SECRET_HIDDEN_TEST'));
  assert.throws(()=>validateInstance({...fixture,base_commit:'HEAD'}),/40-character/);
  assert.throws(()=>validateInstance({...fixture,repo:'../private'}),/Invalid SWE-bench repo/);
  assert.throws(()=>validateInstance({...fixture,repo:'django/../django'}),/Invalid SWE-bench repo/);
});

test('prediction is official schema with real patch and no claimed pass rate',()=>{
  const valid=validateInstance(fixture);
  const patch='diff --git a/a.py b/a.py\n+fixed\n';
  assert.deepEqual(prediction(valid,'dominion-rust/model',patch),{
    instance_id:fixture.instance_id,
    model_name_or_path:'dominion-rust/model',
    model_patch:patch,
  });
  assert.throws(()=>prediction(valid,'model',null),/Real Git patch/);
  const stat=metrics({
    id:'g',status:'needs_review',iterations:2,replans:1,
    receipt:{tokenUsage:{totalTokens:1420}}
  },[
    {toolCalls:4,modelCalls:3,tokens:800},
    {toolCalls:2,modelCalls:2,tokens:620}
  ],2600,patch);
  assert.equal(stat.executedToolCalls,6);
  assert.equal(stat.modelCalls,5);
  assert.equal(stat.totalTokens,1420);
  assert.equal(stat.patchBytes,Buffer.byteLength(patch));
  assert.equal(stat.officialSWEbenchVerdict,null);
  assert.equal(stat.goalVerifiedByNativeHarness,false);
});

test('dataset JSONL is parsed without accepting malformed lines',()=>{
  assert.deepEqual(parseJsonl('{"instance_id":"x"}\n\n{"instance_id":"y"}\n')
    .map(row=>row.instance_id),['x','y']);
  assert.throws(()=>parseJsonl('{"valid":true}\n{broken\n'),/line 2/);
});

test('Git patch captures real edits and untracked source files from pinned commit',async t=>{
  if(spawnSync('git',['--version']).status!==0){t.skip('git not installed');return;}
  const dir=await mkdtemp(join(tmpdir(),'dominion-swebench-test-'));
  try{
    git(dir,'init','-q');
    git(dir,'config','user.name','Dominion Test');
    git(dir,'config','user.email','test@localhost');
    await writeFile(join(dir,'existing.py'),'value = 1\n');
    git(dir,'add','--all');
    git(dir,'commit','-q','-m','initial');
    const sha=git(dir,'rev-parse','HEAD');
    assert.match(sha,/^[a-f0-9]{40}$/);
    await writeFile(join(dir,'existing.py'),'value = 2\n');
    await writeFile(join(dir,'added.py'),'new_value = 3\n');
    const patch=await capturePatch(dir,sha);
    assert.match(patch,/diff --git a\/existing\.py b\/existing\.py/);
    assert.match(patch,/diff --git a\/added\.py b\/added\.py/);
    assert.match(patch,/\+value = 2/);
    assert.match(patch,/\+new_value = 3/);
    assert.equal(await capturePatch(dir,sha),patch);
    assert.rejects(capturePatch(dir,'HEAD'),/Unpinned Git baseline/);
  }finally{
    await rm(dir,{recursive:true,force:true});
  }
});
