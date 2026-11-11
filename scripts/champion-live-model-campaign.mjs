// Champion genuine-model experiment: no fake worker, no authored response fixtures.
// Every measured result comes from the standard DOMINION provider -> RSI job
// -> detached worktree -> frozen A/B pipeline. A 401/502 is recorded as
// provider_unavailable, never reported as autonomous improvement.
import fs from 'node:fs';
import http from 'node:http';

const mode=process.argv[2];
const upstream='https://text.pollinations.ai/openai';
const port=Number(process.env.DOMINION_PORT||4317);
const output=process.env.CHAMPION_LIVE_REPORT||'benchmark/champion-live-campaign-result.json';
const host=`http://127.0.0.1:${port}`;
const save=value=>{
  fs.mkdirSync(output.slice(0,output.lastIndexOf('/'))||'.',{recursive:true});
  fs.writeFileSync(output,JSON.stringify({...value,recordedAt:new Date().toISOString()},null,2)+'\n');
};
const content=(value,limit=800)=>JSON.stringify(value).slice(0,limit);
const response=async(url,options={})=>{
  const result=await fetch(url,{...options,signal:AbortSignal.timeout(100000)});
  const raw=await result.text();
  let json;try{json=JSON.parse(raw)}catch{json={raw:raw.slice(0,800)}}
  return {status:result.status,json};
};
const modelMessage={model:'openai-fast',messages:[{role:'user',content:'Reply only the token READY.'}],max_tokens:20,stream:false};
if(mode==='probe'){
  try{
    const result=await response(upstream,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(modelMessage)});
    const answer=result.json?.choices?.[0]?.message?.content;
    const reachable=result.status===200&&typeof answer==='string'&&answer.length>0;
    save({phase:reachable?'model_reachable':'model_unavailable',model:'openai-fast',provider:'legacy_public_endpoint',
      httpStatus:result.status,answer:typeof answer==='string'?answer.slice(0,120):null,
      error:reachable?null:content(result.json)});
    console.log(`CHAMPION_MODEL_REACHABLE=${reachable?'true':'false'}`);
  }catch(error){
    save({phase:'model_unavailable',model:'openai-fast',error:String(error)});
    console.log('CHAMPION_MODEL_REACHABLE=false');
  }
}else if(mode==='proxy'){
  const server=http.createServer(async(req,res)=>{
    if(!['/v1/chat/completions','/v1/models'].includes(req.url||'')){
      res.writeHead(404);res.end('not found');return;
    }
    try{
      const chunks=[];for await(const piece of req)chunks.push(piece);
      const upstreamResponse=await fetch(upstream,{method:'POST',headers:{'content-type':'application/json'},
        body:Buffer.concat(chunks),signal:AbortSignal.timeout(240000)});
      const raw=await upstreamResponse.text();
      res.writeHead(upstreamResponse.status,{'content-type':'application/json'});
      res.end(raw);
    }catch(error){
      res.writeHead(502,{'content-type':'application/json'});
      res.end(JSON.stringify({error:String(error)}));
    }
  });
  server.listen(5666,'127.0.0.1',()=>console.log('CHAMPION_PROXY_READY'));
}else if(mode==='run'){
  const call=async(path,body)=>{
    const result=await response(host+path,body===undefined?{}:{
      method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)
    });
    if(result.status>=400)throw Error(`${path} HTTP ${result.status}: ${content(result.json)}`);
    return result.json;
  };
  const trace={phase:'starting',provider:'legacy_public_endpoint',model:'openai-fast',
    sourceDir:process.env.DOMINION_SOURCE_WORKSPACE,submittedRequest:null,
    jobId:null,generationEvidence:[],errors:[]};
  try{
    await call('/api/providers',{id:'champion-free-proxy',name:'Champion Free Model',
      kind:'openai',enabled:true,toolMode:'native',baseUrl:'http://127.0.0.1:5666/v1',
      model:'openai-fast'});
    await call('/api/dev/config',{sourceDir:process.env.DOMINION_SOURCE_WORKSPACE});
    const request={
      mode:'continuous',confirmation:'START_RSI_EVOLUTION',
      selectionPolicy:'automatic',providerId:'champion-free-proxy',model:'openai-fast',
      request:'Fix a reproducible source decomposition bug in src/engine/decompose.rs. The Korean suffix -고 is split only if the preceding character occurs in a short hard-coded action-ending list. Consequently two real actions such as 파일을 만들고 결과를 기록해 and 파일을 지우고 다시 만들어 are not decomposed into separately executable steps. After reading the actual action_conjunction_at implementation, call workspace_replace with the exact observed oldText and newText to support commonly used action endings for 만들고, 지우고, 고치고 without breaking nouns such as 사고/최고. Preserve token coverage, original action text, existing markers and numbered list handling; verify what was really changed. Do not end at search or description alone, and if replace rejects an oldText, read the exact current block and retry. The native DOMINION evaluator will independently measure candidate vs baseline in frozen worktrees.',
      focusFiles:['src/engine/decompose.rs'],preferredEditTool:'workspace_replace',maxAttempts:3,
      tokenBudget:90000,timeBudgetSeconds:2100
    };
    trace.submittedRequest={...request,request:request.request.slice(0,320)};
    const queued=await call('/api/rsi/start',request);
    trace.jobId=queued.id;
    if(typeof trace.jobId!=='string'||!trace.jobId)throw Error(`RSI did not return a job id: ${content(queued)}`);
    trace.phase='running';save(trace);
    const deadline=Date.now()+42*60*1000;
    let last=null;
    while(Date.now()<deadline){
      await new Promise(resolve=>setTimeout(resolve,10000));
      last=await call(`/api/jobs/${encodeURIComponent(trace.jobId)}`);
      if(['completed','failed','cancelled','needs_review'].includes(last.status))break;
    }
    if(!last)throw Error('No status after launch');
    const history=last.evolution?.history||last.result?.evolution?.history||[];
    trace.generationEvidence=history.map(g=>({
      attempt:g.attempt,jobId:g.jobId,model:g.generatorModel,
      tokenUsage:g.tokenUsage,candidateId:g.candidateId,accepted:g.accepted,
      acceptedCommit:g.acceptedCommit,verdict:g.evaluation?.verdict,
      improvedHoldouts:g.evaluation?.metrics?.improvedHoldouts,
      generatorCommit:g.generatorCommit,domain:g.domain,searchStrategy:g.searchStrategy,
      error:g.error,toolsUsed:g.toolsUsed,newFindings:g.newFindings,
      observedSourceEdits:g.observedSourceEdits||0,
      editOperations:(g.editOperations||[]).map(op=>({
        tool:op.tool,path:op.path,status:op.status,written:op.written,
        staleVersion:op.staleVersion,error:op.error
      })),
      toolRejections:g.toolRejections||[],
      methodPolicyRevision:g.decisionInput?.reuseDecision?.policyRevision||null,
      selectedMethodIds:g.decisionInput?.reuseDecision?.methodIds||[],
      improvementActionObservation:g.improvementActionObservation,
      focusFiles:g.focusFiles,knowledgeId:g.knowledgeId,
      evaluation:g.evaluation?.metrics
    }));
    trace.phase=['completed','failed','cancelled','needs_review'].includes(last.status)?
      'campaign_finished':'campaign_timeout';
    trace.status=last.status;trace.result=last.result||null;
    trace.stopReason=last.evolution?.stopReason||last.result?.evolution?.stopReason||null;
    const editTools=new Set(['workspace_replace','workspace_replace_range','workspace_write']);
    const countCalls=(generation,names)=>generation.toolsUsed
      .filter(tool=>names.has(tool.tool)).reduce((sum,tool)=>sum+(tool.calls||0),0);
    const sourceEditCalls=trace.generationEvidence.reduce((sum,g)=>
      sum+countCalls(g,editTools),0);
    const verifiedSourceEdits=trace.generationEvidence.reduce((sum,g)=>
      sum+(g.observedSourceEdits||0),0);
    const editedGenerations=trace.generationEvidence.filter(g=>
      (g.observedSourceEdits||0)>0).length;
    const candidateCount=trace.generationEvidence.filter(g=>g.candidateId).length;
    const accepted=trace.generationEvidence.filter(g=>g.accepted===true
      && typeof g.acceptedCommit==='string' && g.acceptedCommit.length>0);
    trace.summary={
      modelCalls:trace.generationEvidence.reduce((sum,g)=>sum+(g.tokenUsage?.calls||0),0),
      sourceEditCalls,verifiedSourceEdits,editedGenerations,
      methodPolicyRevisions:[...new Set(trace.generationEvidence
        .map(g=>g.methodPolicyRevision).filter(Boolean))],
      candidateCount,acceptedGenerations:accepted.length,
      acceptedCommits:accepted.map(g=>g.acceptedCommit),
      readOnlyGenerations:trace.generationEvidence.filter(g=>!g.candidateId
        && countCalls(g,new Set(['code_search','workspace_read_range']))>0).length,
      independentlyVerifiedImprovement:accepted.some(g=>g.verdict==='improved'),
      note:'CI completed does not imply an accepted model-generated RSI improvement'
    };
    trace.actualCompletion=trace.summary.independentlyVerifiedImprovement;
    if(trace.phase==='campaign_timeout')trace.errors.push('campaign did not finish before runner deadline');
    save(trace);
    console.log(JSON.stringify({phase:trace.phase,status:trace.status,jobId:trace.jobId,
      generationEvidence:trace.generationEvidence,stopReason:trace.stopReason}));
  }catch(error){
    trace.phase='campaign_error';trace.errors.push(String(error));save(trace);
    console.log(JSON.stringify({phase:trace.phase,error:String(error)}));
    process.exitCode=1;
  }
}else {
  console.error('Usage: node scripts/champion-live-model-campaign.mjs probe|proxy|run');
  process.exitCode=2;
}
