// Actual free-model/DOMINION end-to-end measurement. No synthetic tool receipts,
// fabricated model replies, or manual patch injection.
import http from 'node:http';
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';

const mode=process.argv[2];
const upstream='https://text.pollinations.ai/openai';
const localModel=process.env.SUB4_MODEL_KIND==='ollama';
const selectedModel=localModel?(process.env.SUB4_MODEL_NAME||'qwen3:4b-instruct'):'openai-fast';
const ollamaOrigin=process.env.SUB4_OLLAMA_URL||'http://127.0.0.1:11434';
const port=Number(process.env.DOMINION_PORT||43179);
const host=`http://127.0.0.1:${port}`;
const reportPath=process.env.SUB4_LIVE_REPORT||'benchmark/sub4-live-rsi-result.json';
const commit=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
let receipt={schema:'sub4.real-model.rsi.v1',branch:'dominion-sub4',
  sourceCommit:commit,model:selectedModel,providerKind:localModel?'ollama':'openai',
  modelContextTokens:localModel?16384:null,
  status:'initializing',
  actualModelCalls:0,sourceWriteCalls:0,modelMethodCalls:0,
  acceptedGenerations:0,measuredImprovement:false,transferEvidence:[],
  generations:[],errors:[]};
function persist() {
  fs.mkdirSync(reportPath.slice(0,reportPath.lastIndexOf('/'))||'.',{recursive:true});
  fs.writeFileSync(reportPath,JSON.stringify({...receipt,recordedAt:new Date().toISOString()},null,2)+'\n');
}
async function post(url,body,timeout=180000) {
  const res=await fetch(url,{method:'POST',headers:{'content-type':'application/json'},
    body:JSON.stringify(body),signal:AbortSignal.timeout(timeout)});
  const data=await res.text();
  let result;try{result=JSON.parse(data)}catch{result={raw:data.slice(0,900)}}
  if(!res.ok)throw Error(`HTTP ${res.status}: ${JSON.stringify(result).slice(0,900)}`);
  return result;
}
async function get(path) {
  const res=await fetch(host+path,{signal:AbortSignal.timeout(15000)});
  const raw=await res.text();
  if(!res.ok)throw Error(`${path} HTTP ${res.status}: ${raw.slice(0,300)}`);
  return JSON.parse(raw);
}
const sleep=(ms)=>new Promise(resolve=>setTimeout(resolve,ms));
const terminal=new Set(['completed','cancelled','failed','needs_review']);
function summarize(g) {
  const ops=Array.isArray(g.toolsUsed)?g.toolsUsed:[];
  const count=name=>ops.filter(o=>o.tool===name).reduce((v,o)=>v+Number(o.calls||0),0);
  const metrics=g.evaluation?.metrics||{};
  const paired=g.accepted===true && !!g.acceptedCommit &&
    metrics.eligible===true && Number(metrics.improvedHoldouts||0)>0 &&
    Number(metrics.regressions||0)===0;
  return {attempt:g.attempt,jobId:g.jobId,
    tokenUsage:g.tokenUsage,actualModelCalls:Number(g.tokenUsage?.calls||0),
    toolsUsed:ops,sourceWriteCalls:ops.filter(o=>['workspace_replace','workspace_replace_range','workspace_write'].includes(o.tool)).reduce((n,o)=>n+Number(o.actualWrites||0),0),
    modelMethodCalls:count('rsi_method_propose'),
    focusFiles:g.focusFiles,domain:g.domain,domainSelection:g.domainSelection,
    candidateId:g.candidateId||null,accepted:g.accepted===true,
    acceptedCommit:g.acceptedCommit||null,holdoutEligible:metrics.eligible===true,
    improvedHoldouts:metrics.improvedHoldouts??null,regressions:metrics.regressions??null,
    measuredGain:paired,methodExecutions:g.methodExecutions||[],
    error:g.error||null,stopReason:g.stopReason||null};
}
if(mode==='probe'){
  try {
    let reply;
    if(localModel){
      const result=await post(ollamaOrigin+'/api/chat',{
        model:selectedModel,messages:[{role:'user',content:'Reply exactly READY.'}],
        stream:false,options:{num_ctx:16384,num_predict:64}},180000);
      reply=result?.message?.content;
    }else{
      const result=await post(upstream,{model:selectedModel,
        messages:[{role:'user',content:'Reply exactly READY.'}],max_tokens:24,stream:false},35000);
      reply=result?.choices?.[0]?.message?.content;
    }
    console.log(`SUB4_FREE_MODEL=${typeof reply==='string'&&reply.length?'available':'unavailable'}`);
    receipt.status='probed';receipt.probeReply=typeof reply==='string'?reply.slice(0,80):null;
  }catch(error){receipt.status='provider_unavailable';receipt.errors.push(String(error));
    console.log('SUB4_FREE_MODEL=unavailable')}
  persist();
}else if(mode==='proxy'){
  const server=http.createServer(async(req,res)=>{
    if(!['/v1/chat/completions','/v1/models'].includes(req.url||'')){
      res.writeHead(404);res.end('not found');return;
    }
    try{
      const data=[];for await(const chunk of req)data.push(chunk);
      const upstreamResult=await fetch(upstream,{method:'POST',headers:{'content-type':'application/json'},
        body:Buffer.concat(data),signal:AbortSignal.timeout(180000)});
      res.writeHead(upstreamResult.status,{'content-type':upstreamResult.headers.get('content-type')||'application/json'});
      res.end(Buffer.from(await upstreamResult.arrayBuffer()));
    }catch(error){
      res.writeHead(502,{'content-type':'application/json'});
      res.end(JSON.stringify({error:String(error)}));
    }
  });
  server.listen(5667,'127.0.0.1',()=>console.log('SUB4_PROXY_READY'));
}else if(mode==='run'){
  try{
    let healthy=false;
    for(let i=0;i<90;i++){
      try{await get('/api/health');healthy=true;break}catch{await sleep(1200)}
    }
    if(!healthy)throw Error('native DOMINION engine did not start');
    const provider=await post(host+'/api/providers',{
      id:'sub4-live-model',name:localModel?'Sub4 real local model':'Sub4 real public model',
      kind:localModel?'ollama':'openai',
      enabled:true,toolMode:'native',
      ...(localModel?{contextTokens:16384,maxOutputTokens:1536}:{}),
      baseUrl:localModel?ollamaOrigin:'http://127.0.0.1:5667/v1',model:selectedModel
    });
    receipt.providerId=provider.id||'sub4-live-model';
    await post(host+'/api/dev/config',{sourceDir:process.env.DOMINION_SOURCE_WORKSPACE});
    const goal='Fix one concrete user-visible planning error in src/engine/decompose.rs. split_markers currently misses Korean ordered actions such as 설계를 읽고 결과를 저장한 다음 검증해. Implement grammar-aware splitting of action suffix -고 and connectors 다음/나서, avoiding false noun splits (사고, 최고), while preserving meaningful step text. Inspect the exact source once using workspace_read_range, then CALL workspace_replace_range with observed startLine, endLine, sourceSha256 as expectedSha256, and improved newText. Do not send oldText or continue searching after the source is observed. Do not just discuss a solution. Then let DOMINION evaluate frozen held-out cases.';
    const started=await post(host+'/api/rsi/start',{
      confirmation:'START_RSI_EVOLUTION',mode:'continuous',
      providerId:receipt.providerId,model:selectedModel,
      selectionPolicy:'automatic',request:goal,
      focusFiles:['src/engine/decompose.rs'],
      maxAttempts:2,tokenBudget:78000,
      timeBudgetSeconds:Number(process.env.SUB4_CAMPAIGN_SECONDS||1250)
    });
    if(typeof started.id!=='string'||!started.id)throw Error('campaign start returned no id');
    receipt.campaignId=started.id;receipt.status='running';persist();
    const deadline=Date.now()+Number(process.env.SUB4_RUN_DEADLINE_MS||21*60*1000);
    let last;
    while(Date.now()<deadline){
      await sleep(10000);
      last=await get('/api/jobs/'+encodeURIComponent(receipt.campaignId));
      if(terminal.has(last.status))break;
    }
    if(!last)throw Error('campaign status never available');
    const history=last.evolution?.history||last.result?.evolution?.history||[];
    receipt.generations=history.map(summarize);
    receipt.actualModelCalls=receipt.generations.reduce((n,g)=>n+g.actualModelCalls,0);
    receipt.sourceWriteCalls=receipt.generations.reduce((n,g)=>n+g.sourceWriteCalls,0);
    receipt.modelMethodCalls=receipt.generations.reduce((n,g)=>n+g.modelMethodCalls,0);
    receipt.acceptedGenerations=receipt.generations.filter(g=>g.accepted).length;
    receipt.measuredImprovement=receipt.generations.some(g=>g.measuredGain);
    receipt.transferEvidence=receipt.generations.flatMap(g=>g.methodExecutions)
      .filter(r=>r.crossDomain===true);
    receipt.status=last.status;
    receipt.reason=last.evolution?.stopReason||last.result?.evolution?.stopReason||null;
    if(!terminal.has(last.status))receipt.errors.push('campaign deadline without terminal result');
    persist();
    console.log(JSON.stringify({status:receipt.status,campaignId:receipt.campaignId,
      calls:receipt.actualModelCalls,writes:receipt.sourceWriteCalls,
      proposedMethods:receipt.modelMethodCalls,accepted:receipt.acceptedGenerations,
      improved:receipt.measuredImprovement,reason:receipt.reason,
      generations:receipt.generations.map(g=>({attempt:g.attempt,writes:g.sourceWriteCalls,
        tools:g.toolsUsed,accepted:g.accepted,error:g.error}))},null,2));
  }catch(error){
    receipt.status='experiment_error';receipt.errors.push(String(error));persist();
    console.error(error);process.exitCode=1;
  }
}else{
  console.error('usage: sub4-live-rsi.mjs probe|proxy|run');process.exitCode=2;
}
