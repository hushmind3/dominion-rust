/**
 * Reproducible Sub2 native-only RSI completion test, ported from the
 * independently demonstrated Sub3 protocol.
 *
 * Never calls an external code model. The Ollama provider record below is
 * deliberately unreachable: a missing native source operator MUST fail,
 * never silently fall back to paid or undisclosed inference.
 *
 * Every accepted patch must be made and independently judged by DOMINION.
 */
import {mkdirSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {dirname,resolve} from 'node:path';

const apiBase=process.env.DOMINION_LIVE_BASE||'http://127.0.0.1:44327';
const output=resolve(process.env.DOMINION_LIVE_REPORT||'benchmark/sub2-native-proof-latest.json');
const timeoutMs=Number(process.env.DOMINION_LIVE_TIMEOUT_MS||3_060_000);
const frozenSource=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
const terminal=new Set(['completed','failed','cancelled','needs_review']);
const sleep=ms=>new Promise(done=>setTimeout(done,ms));
const receipt={
  schemaVersion:1,kind:'dominion.sub2.native_recursive_completion',
  sourceCommit:frozenSource,status:'starting',campaignId:null,
  acceptedGenerations:0,capabilityDemonstrated:false,modelCalls:0,
  history:[],error:null,startedAt:new Date().toISOString()
};
function persist(){
  mkdirSync(dirname(output),{recursive:true});
  writeFileSync(output,JSON.stringify(receipt,null,2)+'\n');
}
async function call(path,method='GET',body){
  const response=await fetch(apiBase+path,{
    method,headers:body===undefined?{}:{'content-type':'application/json'},
    body:body===undefined?undefined:JSON.stringify(body),
    signal:AbortSignal.timeout(45_000)
  });
  const text=await response.text();
  let json;try{json=JSON.parse(text)}catch{json={body:text.slice(0,1000)}}
  if(!response.ok)throw Error('HTTP '+response.status+' '+path+' '+JSON.stringify(json).slice(0,1600));
  return json;
}
async function main(){
  persist();
  let healthy=false;
  for(let i=0;i<90;i++){
    try{await call('/api/health');healthy=true;break;}catch{await sleep(1500)}
  }
  if(!healthy)throw Error('Real native DOMINION server did not start');
  const provider=await call('/api/providers','POST',{
    name:'Sub2 native-only source induction (provider intentionally offline)',
    kind:'ollama',local:true,enabled:true,toolMode:'native',
    baseUrl:'http://127.0.0.1:65534',model:'native-unused',
    contextTokens:16384
  });
  if(!provider.id)throw Error('Provider registration did not persist');
  // Reuse the exact objective of the previously independently
  // measured three-generation native source improvement sequence.
  const request="DOMINION 자기개선기가 실제로 더 나은 알고리즘을 만들고 기억 능력으로 전이하여 독립 검증된 3세대 연속 채택을 달성하세요. 첫 세대는 src/engine/room_state.rs의 저장된 주인 사실을 반복 검증하는 비용을 줄이는 실행 가능한 소스 변형을 찾으세요. 다음 세대에는 src/engine/rsi_evolve.rs의 자기개선 후보 선택을 위한 반복 통계 순회와 중복 영역 판별을 줄이는 새 계산 원리를 생성하세요. 후속 세대는 실제로 재빌드된 개선기가 다시 src/engine/room_state.rs의 동일 질문 반복 분석 또는 저장된 사실의 불변 특징 재계산을 최적화하세요. 선택은 각 세대가 기존 실행 결과와 실제 코드의 구조를 근거로 하며 다른 능력 영역으로의 전이는 동결된 독립 평가 개선과 무회귀가 입증될 때만 인정하세요. 모든 세대에서 소스 변경, 독립 A/B 측정, 채택 커밋, 새로운 개선기 바이너리를 실제로 기록하세요. 특정 소스 형태가 사라졌다면 더 나은 대안을 생성하고 검증하며 효과 없는 시도를 개선으로 보고하지 마세요.";
  const job=await call('/api/rsi/start','POST',{
    mode:'continuous',confirmation:'START_RSI_EVOLUTION',
    providerId:provider.id,model:'native-unused',selectionPolicy:'automatic',
    nativeInduction:true,stopAfterProof:true,request,focusFiles:[],
    maxAttempts:4,tokenBudget:120000,timeBudgetSeconds:3000
  });
  if(!job.id)throw Error('Native campaign did not start');
  receipt.campaignId=job.id;
  receipt.status='running';persist();
  const deadline=Date.now()+timeoutMs;
  while(Date.now()<deadline){
    const campaign=await call('/api/rsi/campaigns/'+encodeURIComponent(job.id));
    receipt.status=campaign.status||'running';
    receipt.acceptedGenerations=Number(campaign.acceptedGenerations||0);
    receipt.capabilityDemonstrated=campaign.capabilityDemonstrated===true;
    receipt.error=campaign.error||null;
    receipt.headCommit=campaign.headCommit||null;
    receipt.maxAcceptedStreak=campaign.maxAcceptedStreak||null;
    receipt.history=(campaign.history||[]).map(g=>({
      attempt:g.attempt,jobId:g.jobId,
      generatorCommit:g.generatorCommit,
      generatorBinarySha256:g.generatorBinarySha256,
      generatorModel:g.generatorModel,
      requestedModel:g.requestedModel||null,
      origin:g.nativeMutation?.origin||null,
      operator:g.nativeMutation?.operator||null,
      domain:g.domain,tokenUsage:g.tokenUsage||null,
      sourceDiffBytes:g.sourceDiffBytes,
      candidateId:g.candidateId||null,
      acceptedCommit:g.acceptedCommit||null,accepted:g.accepted===true,
      verdict:g.evaluation?.verdict||null,
      evidenceSha256:g.evaluation?.evidenceSha256||null,
      metricGains:g.evaluation?.metrics?.metricGains||0,
      metaHoldoutGains:g.evaluation?.metrics?.metaHoldoutGains||0,
      improvedHoldoutDomains:g.evaluation?.metrics?.improvedHoldoutDomains||[],
      improvedHoldouts:g.evaluation?.metrics?.improvedHoldouts||0,
      pairedCases:g.evaluation?.metrics?.pairedCases||0,
      regressions:g.evaluation?.metrics?.regressions??null,
    }));
    receipt.modelCalls=receipt.history.reduce((s,g)=>s+Number(g.tokenUsage?.calls||0),0);
    persist();
    if(terminal.has(receipt.status))break;
    await sleep(10_000);
  }
  if(!terminal.has(receipt.status))throw Error('Native campaign exceeded proof deadline');
  const chain=receipt.history.slice(0,3);
  const lineage=chain.length===3&&chain.every((g,i)=>
    g.accepted&&g.verdict==='improved'&&g.regressions===0
    &&g.pairedCases>=8&&g.improvedHoldouts>0&&g.sourceDiffBytes>0
    &&typeof g.candidateId==='string'
    &&typeof g.generatorBinarySha256==='string'
    &&/^[0-9a-f]{64}$/i.test(g.evidenceSha256||'')
    &&typeof g.acceptedCommit==='string'
    &&g.generatorModel?.startsWith('dominion-native:')
    &&(i===0||g.generatorCommit===chain[i-1].acceptedCommit)
    &&(i===0||g.generatorBinarySha256!==chain[i-1].generatorBinarySha256));
  receipt.nativeProof={
    lineage,zeroModelCalls:receipt.modelCalls===0,
    metaGain:chain.some(g=>g.metaHoldoutGains>0
      &&g.improvedHoldoutDomains.includes('improver')),
    successorTransfer:chain.some((g,i)=>g.metaHoldoutGains>0
      &&chain.slice(i+1).some(next=>
        next.improvedHoldoutDomains.some(domain=>
          domain!=='improver'&&!g.improvedHoldoutDomains.includes(domain))))
  };
  if(receipt.status!=='completed'||receipt.capabilityDemonstrated!==true
      ||receipt.acceptedGenerations<3||!Object.values(receipt.nativeProof).every(Boolean)){
    throw Error('Native RSI proof did not establish 3 independently measured adopted generations, genuine meta gain, successor transfer, zero model calls and distinct rebuilt binaries');
  }
  receipt.error=null;
  receipt.completedAt=new Date().toISOString();
  persist();
  console.log(JSON.stringify({status:receipt.status,capabilityDemonstrated:true,
    acceptedGenerations:receipt.acceptedGenerations,headCommit:receipt.headCommit,
    modelCalls:receipt.modelCalls,chain:receipt.history.map(g=>({
      attempt:g.attempt,operator:g.operator,domain:g.domain,
      generatorCommit:g.generatorCommit,acceptedCommit:g.acceptedCommit,
      metaHoldoutGains:g.metaHoldoutGains,improvedHoldouts:g.improvedHoldouts
    }))},null,2));
}
try{await main()}catch(error){
  receipt.error=String(error);
  if(!terminal.has(receipt.status))receipt.status='proof_failed';
  receipt.completedAt=new Date().toISOString();
  persist();console.error(error);process.exitCode=1;
}
