// This evaluator runs outside both variants. The plan and oracle are frozen
// before candidate code runs; candidate stdout is never itself a verdict.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
const root=process.env.RSI_TEMP;
const plan=JSON.parse(fs.readFileSync(process.env.RSI_PLAN,'utf8'));
const hash=data=>crypto.createHash('sha256').update(data).digest('hex');
const logs=[];
// Candidate build scripts and test binaries must not inherit arbitrary
// production API keys/tokens from the outer coordinator environment.
// This is environment hygiene, not a process/filesystem/OS sandbox.
const allowedEnvironment=[
 'PATH','HOME','USER','LOGNAME','LANG','LC_ALL','TMPDIR','TMP','TEMP',
 'CARGO_HOME','RUSTUP_HOME','RUSTUP_TOOLCHAIN','CARGO_BUILD_TARGET',
 'SDKROOT','MACOSX_DEPLOYMENT_TARGET','CC','CXX','AR','LD',
 'PKG_CONFIG_PATH','PKG_CONFIG_LIBDIR','PKG_CONFIG_SYSROOT_DIR',
 'CMAKE_PREFIX_PATH','CI','RSI_TEMP','RSI_PLAN','RSI_BASE_SHA',
 'RSI_REPORT','RSI_PATCH','RSI_BUILD_CACHE'
];
const childEnvironment=Object.fromEntries(allowedEnvironment
 .filter(key=>typeof process.env[key]==='string')
 .map(key=>[key,process.env[key]]));
const run=(exe,args,cwd)=>{
 const result=spawnSync(exe,args,{cwd,encoding:'utf8',timeout:360000,maxBuffer:8*1024*1024,
  env:{...childEnvironment,CARGO_TARGET_DIR:path.join(process.env.RSI_BUILD_CACHE||path.join(root,'build'),path.basename(cwd))}});
 const text=(result.stdout||'')+'\n'+(result.stderr||'');
 logs.push(text.slice(-80000));
 return {ok:result.status===0&&!result.error,text,stdout:result.stdout||''};
};
const median=values=>{const sorted=[...values].sort((a,b)=>a-b),mid=Math.floor(sorted.length/2);return sorted.length%2?sorted[mid]:(sorted[mid-1]+sorted[mid])/2};
const oracle=plan.oracleFixture?fs.readFileSync(plan.oracleFixture):null;
if(oracle&&hash(oracle)!==plan.oracleSha256)throw Error('Frozen oracle hash mismatch');
const verifyOracle=()=>{if(oracle&&hash(fs.readFileSync(plan.oracleFixture))!==plan.oracleSha256)throw Error('Oracle was modified during execution');};
// Fresh targets keep both compiled variants alive for interleaved AB/BA runs.
const variants={};
for(const label of ['baseline','candidate']){
 const dir=path.join(root,label);
 if(oracle){
  fs.appendFileSync(path.join(dir,'src/main.rs'),`\n#[cfg(test)]\n#[path=${JSON.stringify(plan.oracleFixture)}]\nmod rsi_frozen_oracle;\n`);
 }
 const built=run('cargo',['test','--release','--all-targets','--no-run','--message-format=json'],dir);
 const binaries=built.stdout.split('\n').flatMap(line=>{
  try{const v=JSON.parse(line);return v.reason==='compiler-artifact'&&v.profile?.test&&v.executable?[v.executable]:[]}catch{return []}
 });
 const tests=new Map();
 if(built.ok)for(const binary of new Set(binaries)){
  const listed=run(binary,['--list','--format','terse'],dir);
  if(!listed.ok)throw Error('Cannot enumerate tests');
  for(const line of listed.stdout.split('\n'))if(line.endsWith(': test')){
   const name=line.slice(0,-6);tests.set(name,[...(tests.get(name)||[]),binary]);
  }
 }
 variants[label]={dir,ok:built.ok&&binaries.length>0,tests};
}
const cases=[];
for(const spec of plan.cases){
 const metrics=spec.metrics||[];
 const repetitions=metrics.length?6:1;
 const arms={baseline:{pass:true,values:[]},candidate:{pass:true,values:[]}};
 for(let sample=0;sample<repetitions;sample++){
  // Balance ordering so thermal/cache drift cannot always favor candidate.
  for(const label of sample%2?['candidate','baseline']:['baseline','candidate']){
   verifyOracle();
   const variant=variants[label], arm=arms[label];
   const binaries=variant.tests.get(spec.filter)||[];
   if(!variant.ok||binaries.length!==1){arm.pass=false;continue}
   const result=run(binaries[0],[spec.filter,'--exact','--nocapture','--test-threads=1'],variant.dir);
   const passed=result.ok&&/test result: ok\. 1 passed; 0 failed/.test(result.text);
   arm.pass&&=passed;
   if(metrics.length&&passed){
    const entries=result.stdout.split('\n').filter(line=>line.includes('DOMINION_RSI_METRIC '));
    if(entries.length!==1){arm.pass=false;continue}
    let values;try{values=JSON.parse(entries[0].split('DOMINION_RSI_METRIC ')[1])}catch{arm.pass=false;continue}
    if(metrics.some(metric=>!Number.isFinite(values[metric.key])||values[metric.key]<0)){arm.pass=false;continue}
    arm.values.push(values);
   }
   verifyOracle();
  }
 }
 const measured={};
 if(arms.baseline.pass&&arms.candidate.pass)for(const metric of metrics){
  measured[metric.key]={baseline:median(arms.baseline.values.map(v=>v[metric.key])),
   candidate:median(arms.candidate.values.map(v=>v[metric.key])),samples:repetitions};
 }
 cases.push({id:spec.id,baselinePass:arms.baseline.pass,candidatePass:arms.candidate.pass,metrics:measured});
}
verifyOracle();
const evidence=`baseCommit=${process.env.RSI_BASE_SHA}\noracleSha256=${plan.oracleSha256||''}\n${logs.join('\n')}\n${JSON.stringify(cases)}\n`;
fs.mkdirSync(path.dirname(process.env.RSI_REPORT),{recursive:true});
fs.writeFileSync(process.env.RSI_REPORT+'.evidence.txt',evidence);
const report={baseCommit:process.env.RSI_BASE_SHA,patchSha256:hash(fs.readFileSync(process.env.RSI_PATCH)),
 oracleVersion:plan.oracleVersion,oracleSha256:plan.oracleSha256||null,runner:'balanced-frozen-rsi-lab-v2',
 reportSha256:hash(evidence),baselineBuildOk:variants.baseline.ok,candidateBuildOk:variants.candidate.ok,cases};
fs.writeFileSync(process.env.RSI_REPORT,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({report:process.env.RSI_REPORT,pairedCases:cases.length}));
