import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const runner=fileURLToPath(new URL('./rsi-lab.mjs',import.meta.url));
test('portable runner applies exact patches in isolated worktrees and cleans up paths with spaces',()=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'rsi portable 한글 '));
 const git=(...args)=>{const p=spawnSync('git',args,{cwd:root,encoding:'utf8'});assert.equal(p.status,0,p.stderr);return p.stdout.trim()};
 try{
  git('init','-q');git('config','core.autocrlf','false');
  fs.writeFileSync(path.join(root,'subject.txt'),'before\n');git('add','.');
  git('-c','user.name=Fixture','-c','user.email=fixture@localhost','commit','-qm','baseline');
  const base=git('rev-parse','HEAD');
  const fixtures=path.join(root,'evaluation files');fs.mkdirSync(fixtures);
  fs.copyFileSync(runner,path.join(fixtures,'rsi-lab.mjs'));
  // A fixture grader inspects actual trees; this is not an RSI performance claim.
  fs.writeFileSync(path.join(fixtures,'rsi-measure.mjs'),`import fs from 'node:fs';import path from 'node:path';fs.writeFileSync(process.env.RSI_REPORT,JSON.stringify({baseline:fs.readFileSync(path.join(process.env.RSI_TEMP,'baseline','subject.txt'),'utf8'),candidate:fs.readFileSync(path.join(process.env.RSI_TEMP,'candidate','subject.txt'),'utf8'),temp:process.env.RSI_TEMP}));`);
  fs.writeFileSync(path.join(fixtures,'candidate.patch'),'diff --git a/subject.txt b/subject.txt\n--- a/subject.txt\n+++ b/subject.txt\n@@ -1 +1 @@\n-before\n+after\n');
  fs.writeFileSync(path.join(fixtures,'plan.json'),JSON.stringify({oracleVersion:'portable-test',cases:[{id:'a',domain:'tools',filter:'fixture::a'},{id:'b',domain:'memory',filter:'fixture::b'}]}));
  const report=path.join(fixtures,'report.json');
  const result=spawnSync(process.execPath,[path.join(fixtures,'rsi-lab.mjs'),base,path.join(fixtures,'candidate.patch'),path.join(fixtures,'plan.json'),report],{cwd:root,encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const measured=JSON.parse(fs.readFileSync(report));
  assert.equal(measured.baseline,'before\n');assert.equal(measured.candidate,'after\n');
  assert.equal(fs.readFileSync(path.join(root,'subject.txt'),'utf8'),'before\n');
  assert.equal(fs.existsSync(measured.temp),false);assert.equal(git('worktree','list','--porcelain').split('worktree ').length-1,1);
 }finally{fs.rmSync(root,{recursive:true,force:true})}
});
