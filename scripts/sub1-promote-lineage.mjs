// Publish a source lineage only after the native self-improvement evaluator
// already proved a complete three-generation adoption chain. The workflow's
// GITHUB_TOKEN cannot force-push or erase a concurrently advanced branch.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';

const reportPath=process.env.SUB1_NATIVE_REPORT||'benchmark/sub1-native-proof-result.json';
const source=process.env.SUB1_SOURCE_WORKTREE;
const base=process.env.GITHUB_SHA;
const branch='dominion-sub1';
const shaOk=value=>typeof value==='string'&&/^[a-f0-9]{40}$/.test(value);
const fail=message=>{throw Error('SUB1_LINEAGE_NOT_PUBLISHED: '+message);};
function git(...args) {
  return execFileSync('git',['-C',source,...args],{
    encoding:'utf8',stdio:['ignore','pipe','inherit'],maxBuffer:4*1024*1024
  }).trim();
}
if(!source||!shaOk(base)||!fs.existsSync(source))fail('source worktree and exact event SHA required');
const report=JSON.parse(fs.readFileSync(reportPath,'utf8'));
const proof=report.proof||{};
const chain=proof.adoptedCommits||[];
if(report.complete!==true||report.status!=='completed'
  ||proof.capabilityDemonstrated!==true||proof.metaTransferProven!==true
  ||proof.maxVerifiedStreak<3||proof.nativeAdoptions<3
  ||proof.actualModelCalls!==0||chain.length<3) {
  fail('independent measured capability proof has not completed');
}
if(git('rev-parse','HEAD')!==base)fail('source worktree no longer matches original campaign');
let expectedParent=base;
for(const [i,row] of chain.entries()) {
  if(!shaOk(row.generator)||!shaOk(row.adopted)
    ||row.generator!==expectedParent||row.generator===row.adopted
    ||row.origin!=='native_inductive_source_mutation'
    ||row.verdict!=='improved'||row.metrics?.eligible!==true
    ||(row.metrics?.regressions??1)!==0
    ||(row.metrics?.metricRegressions??1)!==0
    ||(row.metrics?.improvedHoldouts??0)<1
    ||!shaOk(row.pairedEvidenceSha256)) {
    fail('generation '+(i+1)+' lacks independent clean adoption evidence');
  }
  if(git('rev-parse',row.adopted+'^')!==row.generator) {
    fail('generation '+(i+1)+' git parent is not the executed generator');
  }
  const paths=git('diff-tree','--no-commit-id','--name-only','-r',row.adopted)
    .split('\n').filter(Boolean);
  if(paths.length===0||paths.some(path=>!path.startsWith('src/'))) {
    fail('generation '+(i+1)+' lacks an isolated source-code delta');
  }
  expectedParent=row.adopted;
}
const firstImprover=chain.findIndex(row=>row.target==='improver');
if(firstImprover<0||firstImprover>=chain.length-1
  ||!chain.slice(firstImprover+1).some(row=>row.target!== 'improver')) {
  fail('measured meta-method change did not transfer to another domain');
}
const currentRemote=git('ls-remote','origin','refs/heads/'+branch).split(/\s+/)[0];
if(currentRemote!==base)fail('branch moved during experiment; never overwrite newer owner work');
git('checkout','--detach',expectedParent);
if(git('status','--porcelain'))fail('adopted tree is dirty');
// Full original tests on the ACTUAL final descendant, not the generator.
execFileSync('cargo',['test','--release','--all-targets','--locked'],{
  cwd:source,env:process.env,stdio:'inherit',timeout:20*60*1000
});
const afterTest=git('status','--porcelain');
if(afterTest)fail('final test changed source worktree: '+afterTest.slice(0,160));
execFileSync('git',['-C',source,'-c','user.name=Dominion RSI Proof',
  '-c','user.email=rsi-proof@users.noreply.github.com','commit','--allow-empty',
  '-m','chore(sub1): preserve verified native RSI lineage [rsi-lineage]'],{
    stdio:'inherit'
  });
const marker=git('rev-parse','HEAD');
if(git('rev-parse',marker+'^')!==expectedParent)fail('publication marker did not follow proven source tip');
// Git will reject any non-fast-forward race; never use --force.
git('push','origin',marker+':refs/heads/'+branch);
console.log(JSON.stringify({published:true,branch,base,acceptedGenerations:chain.length,
  acceptedTip:expectedParent,publicationCommit:marker,mode:'verified_fast_forward_only'}));
