#!/usr/bin/env node
// Cross-platform, coordinator-owned paired experiment. Git worktrees isolate
// source changes; they are not an OS sandbox for executing candidate code.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath,pathToFileURL} from 'node:url';

const [base,patchArg,planArg,reportArg,...extra]=process.argv.slice(2);
if(extra.length||![base,patchArg,planArg,reportArg].every(Boolean))throw Error('Usage: node rsi-lab.mjs BASE_SHA candidate.patch cases.json report.json');
if(!/^[a-f0-9]{40}$/i.test(base))throw Error('Expected full Git commit SHA');
const run=(args,cwd=process.cwd())=>{
 const result=spawnSync('git',args,{cwd,encoding:'utf8',timeout:30000,maxBuffer:8*1024*1024,windowsHide:true});
 if(result.error||result.status!==0)throw Error(result.error?.message||result.stderr||'Git command failed');
 return result.stdout.trim();
};
const root=run(['rev-parse','--show-toplevel']);
const patch=fs.realpathSync(patchArg),planFile=fs.realpathSync(planArg),report=path.resolve(reportArg);
const plan=JSON.parse(fs.readFileSync(planFile,'utf8'));
if(!Array.isArray(plan.cases)||plan.cases.length<2||plan.cases.length>128)throw Error('2..128 cases required');
if(!/^[A-Za-z0-9_.-]{1,120}$/.test(plan.oracleVersion||''))throw Error('oracleVersion required');
const seen=new Set();
for(const c of plan.cases){
 if(!/^[A-Za-z0-9_.-]{1,120}$/.test(c.id||'')||! /^[A-Za-z0-9_.-]{1,80}$/.test(c.domain||'')||! /^[A-Za-z0-9_:]{1,240}$/.test(c.filter||'')||seen.has(c.id))throw Error('Invalid or duplicate case id/domain/exact filter');
 seen.add(c.id);
}
run(['cat-file','-e',base+'^{commit}'],root);
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'dominion-rsi-'));
const created=[];
try{
 for(const arm of ['baseline','candidate']){
  const dir=path.join(temp,arm);run(['worktree','add','--quiet','--detach',dir,base],root);created.push(dir);
 }
 if(plan.fixturePatch){
  const fixture=path.resolve(path.dirname(planFile),plan.fixturePatch);
  for(const dir of created){run(['apply','--check',fixture],dir);run(['apply',fixture],dir);}
 }
 const candidate=path.join(temp,'candidate');
 run(['apply','--check',patch],candidate);run(['apply',patch],candidate);
 Object.assign(process.env,{RSI_TEMP:temp,RSI_PLAN:planFile,RSI_PATCH:patch,RSI_BASE_SHA:base,RSI_REPORT:report});
 await import(pathToFileURL(path.join(path.dirname(fileURLToPath(import.meta.url)),'rsi-measure.mjs')).href);
}finally{
 for(const dir of created.reverse()){
  try{run(['worktree','remove','--force',dir],root);}catch{/* preserve the primary experiment failure */}
 }
 fs.rmSync(temp,{recursive:true,force:true});
}
