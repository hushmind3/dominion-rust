// Source-driven, model-free native RSI campaign on a fresh Sub1 worktree.
// Every accepted generation MUST be independently built and paired-tested by
// DOMINION. Workflow success alone is never counted as RSI proof.
import fs from 'node:fs';
const port=Number(process.env.DOMINION_PORT||43173);
const origin=`http://127.0.0.1:${port}`;
const output=process.env.SUB1_NATIVE_REPORT||'benchmark/sub1-native-proof-result.json';
const save=record=>{
  fs.mkdirSync('benchmark',{recursive:true});
  fs.writeFileSync(output,JSON.stringify({...record,recordedAt:new Date().toISOString()},null,2)+'\n');
};
async function call(path,body) {
  const response=await fetch(origin+path,{
    ...(body===undefined?{}:{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)}),
    signal:AbortSignal.timeout(20000)
  });
  const raw=await response.text();
  let json;
  try{json=JSON.parse(raw)}catch{json={raw:raw.slice(0,1500)}}
  if(!response.ok)throw Error(`${path} ${response.status}: ${JSON.stringify(json).slice(0,1000)}`);
  return json;
}
const report={phase:'configuring',sourceDir:process.env.DOMINION_SOURCE_WORKSPACE,
  requestedGenerator:'dominion-native',historicalBugRestored:false,modelCallsClaimed:0,
  campaignId:null,history:[],errors:[],complete:false};
try {
  if(!report.sourceDir)throw Error('DOMINION_SOURCE_WORKSPACE is required');
  await call('/api/providers',{id:'sub1-native-proof',name:'Sub1 Native Proof',
    kind:'openai',enabled:true,toolMode:'native',
    baseUrl:'http://127.0.0.1:59999/v1',model:'native-only'});
  await call('/api/dev/config',{sourceDir:report.sourceDir});
  const request={mode:'continuous',confirmation:'START_RSI_EVOLUTION',
    selectionPolicy:'automatic',providerId:'sub1-native-proof',model:'native-only',
    request:'memory',focusFiles:[],nativeInduction:true,expandAfterGain:true,
    stopAfterProof:true,maxAttempts:5,tokenBudget:240000,timeBudgetSeconds:6000};
  report.request=request;
  const job=await call('/api/rsi/start',request);
  report.campaignId=job.id;
  if(typeof job.id!=='string')throw Error('RSI campaign ID missing');
  report.phase='running';save(report);
  const deadline=Date.now()+104*60*1000;
  let last=null;
  while(Date.now()<deadline){
    await new Promise(resolve=>setTimeout(resolve,10000));
    last=await call('/api/jobs/'+encodeURIComponent(job.id));
    if(['completed','failed','cancelled','needs_review'].includes(last.status))break;
  }
  if(!last)throw Error('No recorded campaign status');
  const evolution=last.evolution||last.result?.evolution||{};
  report.phase='measured';
  report.status=last.status;
  report.stopReason=evolution.stopReason||null;
  // Preserve the true campaign failure, not just the summary counters.
  report.failureDetail={
    jobError:last.error||null,
    campaignError:evolution.error||null,
    campaignPhase:evolution.phase||null,
    activeGenerationJobId:evolution.activeGenerationJobId||null,
    nextGenerationPlan:evolution.nextGenerationPlan?.targetPlan||null,
    planObjectiveOverride:evolution.planObjectiveOverride||null,
    attempts:evolution.attempts||0
  };
  report.history=evolution.history||[];
  report.proof={
    maxVerifiedStreak:evolution.maxVerifiedStreak||0,
    capabilityDemonstrated:evolution.capabilityDemonstrated===true,
    metaTransferProven:evolution.metaTransferProven===true,
    acceptedGenerations:report.history.filter(g=>g.accepted===true).length,
    nativeAdoptions:report.history.filter(g=>g.accepted===true
      && g.candidateOrigin==='native_inductive_source_mutation').length,
    actualModelCalls:report.history.reduce((v,g)=>v+(g.tokenUsage?.calls||0),0),
    adoptedCommits:report.history.filter(g=>g.accepted===true)
      .map(g=>({generator:g.generatorCommit,adopted:g.acceptedCommit,
        origin:g.candidateOrigin,target:g.candidateTarget,
        pairedEvidenceSha256:g.evaluation?.evidenceSha256,
        verdict:g.evaluation?.verdict,
        metrics:g.evaluation?.metrics}))
  };
  const p=report.proof;
  report.complete=last.status==='completed' && p.capabilityDemonstrated
    && p.metaTransferProven && p.maxVerifiedStreak>=3 && p.nativeAdoptions>=3
    && p.actualModelCalls===0;
  save(report);
  console.log(JSON.stringify({phase:report.phase,campaignId:job.id,
    status:report.status,stopReason:report.stopReason,
    failureDetail:report.failureDetail,
    proof:report.proof,complete:report.complete}));
  if(!report.complete)throw Error('Independent native RSI proof incomplete');
}catch(error){
  report.errors.push(String(error));
  report.phase=report.complete?'complete':'incomplete';
  save(report);
  console.error('SUB1_NATIVE_PROOF_ERROR',String(error));
  process.exitCode=1;
}
