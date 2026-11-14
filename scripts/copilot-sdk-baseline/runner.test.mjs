import test from "node:test";
import assert from "node:assert/strict";
import {mkdtempSync,mkdirSync,writeFileSync,symlinkSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {dirname,join} from "node:path";
import {
  withinCase,permissionForCase,isLocalHost,acceptanceProof,
  summarizeSdk,validateBenchmarkRoot
} from "./runner.mjs";

test("SDK permission gateway allows only in-scope file read/write, never arbitrary tools",()=>{
  const tmp=mkdtempSync(join(tmpdir(),"sdk-permission-test-"));
  try{
    const workspace=join(tmp,"workspace");
    const target=join(workspace,"eval-runs","run","task");
    mkdirSync(target,{recursive:true});
    assert.equal(withinCase(workspace,target,"eval-runs/run/task/report.txt"),true);
    assert.equal(withinCase(workspace,target,join(target,"report.txt")),true);
    assert.equal(withinCase(workspace,target,"eval-runs/run/task/../outside.txt"),false);
    assert.equal(withinCase(workspace,target,"eval-runs/run/other/file.txt"),false);
    assert.equal(withinCase(workspace,target,"/etc/passwd"),false);
    assert.equal(withinCase(workspace,target,"eval-runs/run/task"),false);
    symlinkSync(tmp,join(target,"symlink"));
    assert.equal(withinCase(workspace,target,"eval-runs/run/task/symlink/escape.txt"),false);
    const allowed={kind:"write",fileName:"eval-runs/run/task/report.txt"};
    assert.equal(permissionForCase(workspace,target,allowed).kind,"approve-once");
    assert.equal(permissionForCase(workspace,target,{...allowed,managedApprovalRequired:true}).kind,"reject");
    assert.equal(permissionForCase(workspace,target,{kind:"read",fileName:"eval-runs/run/other/private"}).kind,"reject");
    assert.equal(permissionForCase(workspace,target,{kind:"shell",fullCommandText:"rm -rf /"}).kind,"reject");
    assert.equal(permissionForCase(workspace,target,{kind:"url",url:"https://example.com"}).kind,"reject");
    assert.equal(permissionForCase(workspace,target,{kind:"mcp",toolName:"side_effect"}).kind,"reject");
    assert.equal(permissionForCase(workspace,target,{kind:"write"}).kind,"reject");
    assert.equal(permissionForCase(workspace,target,{kind:"unknown",fileName:"eval-runs/run/task/report.txt"}).kind,"reject");
  }finally{rmSync(tmp,{recursive:true,force:true})}
});

test("SDK billing gate recognizes loopback URLs only",()=>{
  assert.equal(isLocalHost("http://127.0.0.1:11434/v1"),true);
  assert.equal(isLocalHost("http://localhost:1234/v1"),true);
  assert.equal(isLocalHost("https://example.com/v1"),false);
  assert.equal(isLocalHost("http://127.0.0.1.evil.com/v1"),false);
  assert.equal(isLocalHost("http://user:pass@localhost:11434/v1"),false);
  assert.equal(isLocalHost("file:///local"),false);
});

test("only independent artifact AND real token-usage event creates SDK success",()=>{
  const row={status:"finished",oracle:{passed:true},usage:{calls:1,totalTokens:234},error:null,limitReached:false};
  assert.equal(acceptanceProof(row),true);
  assert.equal(acceptanceProof({...row,usage:{calls:0,totalTokens:0}}),false);
  assert.equal(acceptanceProof({...row,oracle:{passed:false}}),false);
  assert.equal(acceptanceProof({...row,status:"no_reply"}),false);
  assert.equal(acceptanceProof({...row,limitReached:true}),false);
  const totals=summarizeSdk([
    {...row,verified:true}, {...row,verified:false,usage:{calls:1,totalTokens:500}}
  ]);
  assert.equal(totals.passAt1,0.5);
  assert.equal(totals.recordedTokensPerVerifiedCompletion,734,
    "failed model calls belong in the cost per verified task");
  assert.equal(totals.actualBilledKRW,null);
  assert.equal(summarizeSdk([...Array(1)].map(()=>({...row,verified:false,usage:null}))).missingUsageReports,1);
});

test("requires isolated disposable benchmark root and separate workspace/data",()=>{
  const root=mkdtempSync(join(tmpdir(),"copilot-sdk-root-"));
  try{
    mkdirSync(join(root,"workspace"));mkdirSync(join(root,"data"));
    assert.throws(()=>validateBenchmarkRoot(root),/sentinel/);
    writeFileSync(join(root,".dominion-benchmark-sentinel"),"benchmark");
    assert.equal(validateBenchmarkRoot(root).root,root);
    const link=join(root,"workspace-symlink");
    symlinkSync(join(root,"workspace"),link);
    rmSync(join(root,"workspace"),{recursive:true});
    symlinkSync(link,join(root,"workspace"));
    assert.throws(()=>validateBenchmarkRoot(root),/symlinks/);
  }finally{rmSync(root,{recursive:true,force:true})}
});
