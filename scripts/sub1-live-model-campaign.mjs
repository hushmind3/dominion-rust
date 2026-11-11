// Sub1 genuine-model experiment: no fake worker, no authored response fixtures.
// Every measured result comes from the standard DOMINION provider -> RSI job
// -> detached worktree -> frozen A/B pipeline. A 401/502 is recorded as
// provider_unavailable, never reported as autonomous improvement.
import fs from 'node:fs';
import http from 'node:http';

const mode=process.argv[2];
const upstream='https://text.pollinations.ai/openai';
const port=Number(process.env.DOMINION_PORT||4317);
const output=process.env.SUB1_LIVE_REPORT||'benchmark/sub1-live-campaign-result.json';
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
    console.log(`SUB1_MODEL_REACHABLE=${reachable?'true':'false'}`);
  }catch(error){
    save({phase:'model_unavailable',model:'openai-fast',error:String(error)});
    console.log('SUB1_MODEL_REACHABLE=false');
  }
}else if(mode==='proxy'){
  const server=http.createServer(async(req,res)=>{
    if(!['/v1/chat/completions','/v1/models'].includes(req.url||'')){
      res.writeHead(404);res.end('not found');return;
    }
    try{
      const chunks=[];for await(const piece of req)chunks.push(piece);
      const rawBody=Buffer.concat(chunks);
      // Only the tool menu, not secrets or full source, is persisted. This
      // distinguishes "required was never sent" from "model ignored required".
      try {
        const payload=JSON.parse(rawBody.toString('utf8'));
        const names=(payload.tools||[]).map(t=>t.function?.name||t.name||'unknown');
        const record={at:new Date().toISOString(),model:payload.model,
          toolChoice:payload.tool_choice||null,reasoningEffort:payload.reasoning_effort||null,
          maxOutputTokens:payload.max_tokens||null,tools:names,
          messages:(payload.messages||[]).map(m=>({role:m.role,
            chars:typeof m.content==='string'?m.content.length:0}))};
        fs.mkdirSync('benchmark',{recursive:true});
        fs.appendFileSync('benchmark/sub1-proxy-tool-choice.jsonl',JSON.stringify(record)+'\n');
      } catch(error) {
        console.error('SUB1_PROXY_TRACE_ERROR',String(error));
      }
      const upstreamResponse=await fetch(upstream,{method:'POST',headers:{'content-type':'application/json'},
        body:rawBody,signal:AbortSignal.timeout(240000)});
      const raw=await upstreamResponse.text();
      res.writeHead(upstreamResponse.status,{'content-type':'application/json'});
      res.end(raw);
    }catch(error){
      res.writeHead(502,{'content-type':'application/json'});
      res.end(JSON.stringify({error:String(error)}));
    }
  });
  server.listen(5666,'127.0.0.1',()=>console.log('SUB1_PROXY_READY'));
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
    await call('/api/providers',{id:'sub1-free-proxy',name:'Sub1 Free Model',
      kind:'openai',enabled:true,toolMode:'native',baseUrl:'http://127.0.0.1:5666/v1',
      model:'openai-fast'});
    await call('/api/dev/config',{sourceDir:process.env.DOMINION_SOURCE_WORKSPACE});
    const request={
      mode:'continuous',confirmation:'START_RSI_EVOLUTION',
      selectionPolicy:'automatic',providerId:'sub1-free-proxy',model:'openai-fast',
      thinking:false,reasoningEffort:'low',
      request:'Improve the real Rust task decomposition function split_markers() in src/engine/decompose.rs. Frozen evaluator tests Korean ordered actions: 설계를 읽고 결과를 저장한 다음 검증해 (3 actions), 코드를 수정하고 나서 테스트를 실행해 (2 actions), 자료를 검색하고 내용을 요약해 (2 actions); and negative noun cases 사고 예방책을 검토해 and 최고 품질을 유지해 (1 action each). The original source only recognizes standalone words like 그리고 / 뒤 and misses 다음 / 나서 and the Korean verb connective -고. This is a genuine source edit, not an explanation: read the exact Rust source range, then change a SMALL, meaningful portion of split_markers() with workspace_replace_range or workspace_replace. First obtain an independently compiling, quality-improving patch; subsequent generations should extend the incomplete behavior using the actual independent holdout feedback. Preserve original task texts, numbered lists, and noun negatives; never edit the frozen oracle. If compilation fails, use exact rustc diagnostics and reread the current mutated source before the next patch. An unchanged read, analysis or test alone is not success',
      focusFiles:['src/engine/decompose.rs'],
      maxAttempts:Number(process.env.SUB1_LIVE_MAX_ATTEMPTS||3),
      tokenBudget:Number(process.env.SUB1_LIVE_TOKEN_BUDGET||90000),
      timeBudgetSeconds:Number(process.env.SUB1_LIVE_TIME_BUDGET||2700)
    };
    trace.submittedRequest={...request,request:request.request.slice(0,320)};
    const queued=await call('/api/rsi/start',request);
    trace.jobId=queued.id;
    if(typeof trace.jobId!=='string'||!trace.jobId)throw Error(`RSI did not return a job id: ${content(queued)}`);
    trace.phase='running';save(trace);
    const deadline=Date.now()+Number(process.env.SUB1_LIVE_DEADLINE_MS||42*60*1000);
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
      writeReceipts:g.writeReceipts||[],actionTrace:g.actionTrace||[],
      methodActions:g.methodActions||[],executionMethod:g.executionMethod||null,
      attemptedCandidateId:g.attemptedCandidateId||null,
      candidateCapture:g.candidateCapture||null,
      improvementDirective:g.improvementDirective||null,
      focusFiles:g.focusFiles,knowledgeId:g.knowledgeId,
      evaluation:g.evaluation?.metrics
    }));
    // Preserve actual model-authored diffs, independent build evidence and
    // holdout reports, not a fabricated success count.
    trace.sourceCandidates=[];
    const candidateIds=[...new Set(trace.generationEvidence.flatMap(g=>
      [g.candidateId,g.attemptedCandidateId].filter(id=>typeof id==='string')))];
    for(const id of candidateIds){
      try{
        const candidate=await call(`/api/rsi/${encodeURIComponent(id)}`);
        trace.sourceCandidates.push({
          id,baseCommit:candidate.baseCommit,target:candidate.target,
          patch:candidate.patch,patchSha256:candidate.patchSha256,
          paths:candidate.paths,status:candidate.status,
          evaluation:candidate.evaluation||null
        });
      }catch(error){trace.errors.push(`candidate ${id}: ${String(error)}`);}
    }
    trace.phase=['completed','failed','cancelled','needs_review'].includes(last.status)?
      'campaign_finished':'campaign_timeout';
    trace.lineage={
      acceptedStreak:last.evolution?.acceptedStreak??last.result?.evolution?.acceptedStreak??0,
      maxAcceptedStreak:last.evolution?.maxAcceptedStreak??last.result?.evolution?.maxAcceptedStreak??0,
      maxVerifiedStreak:last.evolution?.maxVerifiedStreak??last.result?.evolution?.maxVerifiedStreak??0,
      capabilityDemonstrated:last.evolution?.capabilityDemonstrated===true
        ||last.result?.evolution?.capabilityDemonstrated===true,
      metaTransferProven:last.evolution?.metaTransferProven===true
        ||last.result?.evolution?.metaTransferProven===true,
      generatorTransitions:trace.generationEvidence.slice(1).map((g,i)=>({
        generation:g.attempt,parentAcceptedCommit:trace.generationEvidence[i].acceptedCommit,
        generatorCommit:g.generatorCommit,
        inheritedParent:g.generatorCommit===trace.generationEvidence[i].acceptedCommit
          && typeof g.generatorCommit==='string',
        independentlyImproved:g.accepted===true && g.verdict==='improved'
      }))
    };
    trace.status=last.status;trace.result=last.result||null;
    trace.stopReason=last.evolution?.stopReason||last.result?.evolution?.stopReason||null;
    const editTools=new Set(['workspace_replace','workspace_replace_range','workspace_write']);
    const countCalls=(generation,names)=>generation.toolsUsed
      .filter(tool=>names.has(tool.tool)).reduce((sum,tool)=>sum+(tool.calls||0),0);
    const sourceEditCalls=trace.generationEvidence.reduce((sum,g)=>
      sum+countCalls(g,editTools),0);
    const candidateCount=trace.generationEvidence.filter(g=>g.candidateId||g.attemptedCandidateId).length;
    const sourceReceipts=trace.generationEvidence.flatMap(g=>g.writeReceipts||[])
      .filter(receipt=>editTools.has(receipt.tool));
    const learnedProgramExecutions=trace.generationEvidence.reduce((total,g)=>
      total+(g.methodActions||[]).filter(row=>row.inducedProgramDecision).length,0);
    const accepted=trace.generationEvidence.filter(g=>g.accepted===true
      && typeof g.acceptedCommit==='string' && g.acceptedCommit.length>0);
    trace.summary={
      modelCalls:trace.generationEvidence.reduce((sum,g)=>sum+(g.tokenUsage?.calls||0),0),
      sourceEditCalls,candidateCount,
      verifiedSourceWrites:sourceReceipts.filter(receipt=>receipt.written===true).length,
      rejectedSourceWrites:sourceReceipts.filter(receipt=>receipt.written!==true).length,
      learnedProgramExecutions,
      acceptedGenerations:accepted.length,
      acceptedCommits:accepted.map(g=>g.acceptedCommit),
      readOnlyGenerations:trace.generationEvidence.filter(g=>!g.candidateId
        && countCalls(g,new Set(['code_search','workspace_read_range']))>0).length,
      independentlyVerifiedImprovement:accepted.some(g=>g.verdict==='improved'),
      note:'CI completed does not imply an accepted model-generated RSI improvement'
    };
    trace.actualCompletion=trace.summary.independentlyVerifiedImprovement;
    if(trace.summary.verifiedSourceWrites===0) {
      trace.noEditModelResponses=[];
      for(const generation of trace.generationEvidence) {
        if(!generation.jobId) continue;
        try {
          const worker=await call(`/api/jobs/${encodeURIComponent(generation.jobId)}`);
          const sessionId=worker.sessionId||worker.result?.sessionId;
          if(!sessionId) {
            trace.errors.push(`no sessionId for generation ${generation.jobId}`);
            continue;
          }
          const messages=await call(`/api/sessions/${encodeURIComponent(sessionId)}/messages`);
          const rows=Array.isArray(messages)?messages:[];
          trace.noEditModelResponses.push({jobId:generation.jobId,sessionId,
            assistantMessages:rows.filter(row=>row.role==='assistant').slice(-8).map(row=>({
              content:String(row.content||'').slice(-2600),
              reasoning:String(row.reasoning||'').slice(-1600),
              toolCalls:(row.tool_calls||row.toolCalls||[]).map(t=>({
                name:t.function?.name||t.name||null,
                arguments:String(t.function?.arguments||t.arguments||'').slice(0,900)
              }))
            }))});
        } catch(error) {trace.errors.push(`model transcript: ${String(error)}`);}
      }
    }
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
  console.error('Usage: node scripts/sub1-live-model-campaign.mjs probe|proxy|run');
  process.exitCode=2;
}
