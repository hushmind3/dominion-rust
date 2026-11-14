#!/usr/bin/env node
/**
 * Actual SWE-bench Verified task driver for DOMINION's native Rust harness.
 *
 * Input: official dataset JSONL, owner-prepared local Git clones, a real Rust
 * binary, and a real provider configuration. Never feeds test_patch to the
 * model. Predictions are Git patches, not model-authored success claims.
 *
 * The official SWE-bench Docker evaluator is a separate process; running this
 * driver alone never asserts benchmark pass rates.
 */
import {readFile, appendFile, mkdir, mkdtemp, realpath, rm} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {join, resolve, dirname, sep} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {DominionRuntime} from '../sdk/js/node.js';

const execute = promisify(execFile);
const SHA = /^[a-f0-9]{40}$/i;
const REPO_PART = /^[a-zA-Z0-9_.-]+$/;
const PATCH_LIMIT = 8 * 1024 * 1024;
const TOOL_NAMES = [
  'workspace_list','workspace_read','workspace_read_range',
  'workspace_write','workspace_replace','workspace_replace_range',
  'code_search','file_list','file_read','file_write',
  'shell_exec','terminal_exec'
];

export function validateInstance(row) {
  if (!row || typeof row !== 'object' || Array.isArray(row)) {
    throw new TypeError('SWE-bench instance must be a JSON object');
  }
  const id = row.instance_id;
  const repo = row.repo;
  const commit = row.base_commit;
  const problem = row.problem_statement;
  if (typeof id !== 'string' || !/^[a-zA-Z0-9_.-]{1,180}$/.test(id)) {
    throw new Error('Invalid SWE-bench instance_id');
  }
  const parts = typeof repo === 'string' ? repo.split('/') : [];
  if (parts.length !== 2 || parts.some(p => !REPO_PART.test(p) || p === '.' || p === '..')) {
    throw new Error('Invalid SWE-bench repo: ' + String(repo));
  }
  if (typeof commit !== 'string' || !SHA.test(commit)) {
    throw new Error('Instance has no exact 40-character base_commit: ' + id);
  }
  if (typeof problem !== 'string' || !problem.trim() || problem.length > 250_000) {
    throw new Error('Instance problem_statement missing/too long: ' + id);
  }
  return {instance_id:id, repo, base_commit:commit.toLowerCase(), problem_statement:problem};
}

export function prediction(instance, model, patch) {
  if (typeof patch !== 'string' || Buffer.byteLength(patch) > PATCH_LIMIT) {
    throw new RangeError('Real Git patch exceeds 8 MiB limit');
  }
  return {
    instance_id: instance.instance_id,
    model_name_or_path: model,
    model_patch: patch
  };
}

/** Recoveries have to be OBSERVED in the same execution after the failure.
 * Successful unrelated tools cannot retroactively erase a failed action. */
export function measuredRecovery(operations) {
  const rows=(Array.isArray(operations)?operations:[])
    .filter(op=>op && typeof op==='object')
    .sort((a,b)=>String(a.runId||'').localeCompare(String(b.runId||''))
      || (Number(a.opSeq)||0)-(Number(b.opSeq)||0));
  const writeTools=new Set(['workspace_write','workspace_replace',
    'workspace_replace_range','file_write']);
  const commandTools=new Set(['shell_exec','terminal_exec']);
  const executionFailed=op=>{
    const result=op.result;
    if(!result || typeof result!=='object')return false;
    if(result.error!=null || result.isError===true || result.success===false)return true;
    if(result.exitCode!==undefined && result.exitCode!==null
      && (!Number.isSafeInteger(result.exitCode)||result.exitCode!==0))return true;
    if(writeTools.has(op.tool) && result.written!==true)return true;
    return false;
  };
  // Interrupted/outcome-unknown operations may have produced external effects.
  // Neither a failure nor a success is inferred from an uncertain operation.
  const uncertain=op=>op.outcomeUnknown===true
    || ['running','interrupted'].includes(op.status);
  const successful=op=>!uncertain(op)&&op.status==='completed'
    && op.result!=null && !executionFailed(op);
  const failed=op=>!uncertain(op)
    && (['failed','denied','error','needs_review'].includes(op.status)
      || (op.status==='completed'&&executionFailed(op)));
  const attempted=rows.filter(op=>successful(op)||failed(op));
  const failures=attempted.filter(failed);
  const sameTarget=(first,next)=>{
    const prior=first.arguments||{};
    const later=next.arguments||{};
    // A read after a failed write is investigation, not a recovered edit.
    if(writeTools.has(first.tool))return writeTools.has(next.tool)
      && typeof prior.path==='string' && prior.path.length>0
      && later.path===prior.path && next.result?.written===true;
    if(commandTools.has(first.tool))return next.tool===first.tool
      && typeof prior.command==='string' && prior.command.length>0
      && later.command===prior.command
      && (later.cwd??'')===(prior.cwd??'');
    const path=prior.path??prior.cwd;
    if(path!=null)return next.tool===first.tool
      && (later.path??later.cwd)===path;
    return next.tool===first.tool
      && JSON.stringify(later)===JSON.stringify(prior);
  };
  const recovered=failures.filter(error=>{
    const seq=error.opSeq;
    if(typeof error.runId!=='string'||!error.runId
      ||!Number.isSafeInteger(seq)||seq<0)return false;
    return rows.some(next=>next.runId===error.runId
      && Number.isSafeInteger(next.opSeq)&&next.opSeq>seq
      && successful(next)&&sameTarget(error,next));
  }).length;
  return {
    observedToolCalls:attempted.length,
    successfulToolCalls:attempted.filter(successful).length,
    failedToolCalls:failures.length,
    recoveredToolFailures:recovered,
    unknownToolOutcomes:rows.filter(uncertain).length,
    verifiedToolEffects:attempted.filter(op=>successful(op)
      && op.effectState==='verified' && op.toolReceipt?.verified===true).length,
    // No failure observations means no measured recovery rate, not 100%.
    toolFailureRecoveryRate:failures.length===0 ? null :
      recovered/failures.length,
  };
}

/** Owner-specified Korean-won price card; never mistaken for actual billing. */
export function estimatedKrw(usage,pricing){
  if(!pricing)return null;
  const input=Number(pricing.promptPerMillionKrw);
  const output=Number(pricing.completionPerMillionKrw);
  if(!Number.isFinite(input)||!Number.isFinite(output)||input<0||output<0){
    throw new Error('Pricing must specify nonnegative KRW per million tokens');
  }
  const prompt=Number(usage?.promptTokens)||0;
  const completion=Number(usage?.completionTokens)||0;
  if(prompt===0 && completion===0)return null;
  return Math.round((prompt*input+completion*output)/1_000_000);
}

export function metrics(goal, executions, elapsedMs, patch, operations=[],pricing=null) {
  const records = Array.isArray(executions) ? executions : [];
  const sum = field => records.reduce((n, r) => n + (Number(r?.[field]) || 0), 0);
  const tokens = goal?.receipt?.tokenUsage ?? {};
  const observed=measuredRecovery(operations);
  return {
    goalId: goal?.id ?? null,
    goalStatus: goal?.status ?? 'unavailable',
    goalVerifiedByNativeHarness: goal?.status === 'completed',
    // The native completion verdict is NOT the SWE-bench hidden-test verdict.
    officialSWEbenchVerdict: null,
    iterations: Number(goal?.iterations) || 0,
    replans: Number(goal?.replans) || 0,
    executedToolCalls: observed.observedToolCalls || sum('toolCalls'),
    ...observed,
    modelCalls: Number(tokens.calls) || sum('modelCalls'),
    promptTokens: Number(tokens.promptTokens)||0,
    completionTokens: Number(tokens.completionTokens)||0,
    totalTokens: Number(tokens.totalTokens) || sum('tokens'),
    estimatedTokenCalls: Number(tokens.estimatedCalls)||0,
    estimatedProviderCostKrw:estimatedKrw(tokens,pricing),
    actualProviderBillingKrw:null,
    cacheDiscountIncluded:false,
    patchBytes: Buffer.byteLength(patch),
    elapsedMs,
  };
}

export function parseJsonl(text) {
  return text.split(/\r?\n/).filter(line=>line.trim())
    .map((line,index)=>{
      try{return JSON.parse(line);}
      catch(error){throw new Error('Invalid benchmark JSONL line ' + (index+1) + ': ' + error.message);}
    });
}

function options(args) {
  const out={limit:0,maxIterations:12,tokenBudget:120_000,timeoutMs:1_800_000,
    approveTools:false,resume:false,keep:false};
  const numeric={'--limit':'limit','--iterations':'maxIterations',
    '--tokens':'tokenBudget','--timeout-ms':'timeoutMs'};
  const paths={'--dataset':'dataset','--repos':'repos','--binary':'binary',
    '--provider':'provider','--output':'output','--metrics':'metricsPath',
    '--pricing':'pricingPath'};
  for(let i=0;i<args.length;i++){
    const key=args[i];
    if(key==='--approve-tools'){out.approveTools=true;continue;}
    if(key==='--resume'){out.resume=true;continue;}
    if(key==='--keep-workspaces'){out.keep=true;continue;}
    if(numeric[key] || paths[key]){
      const value=args[++i];
      if(!value)throw new Error('Missing value for '+key);
      if(numeric[key]){
        const number=Number(value);
        if(!Number.isSafeInteger(number)||number<0)throw new Error('Invalid '+key);
        out[numeric[key]]=number;
      } else out[paths[key]]=resolve(value);
      continue;
    }
    throw new Error('Unknown argument: '+key);
  }
  for(const key of ['dataset','repos','binary','provider','output']){
    if(!out[key])throw new Error('Missing --'+key);
  }
  if(!out.approveTools)throw new Error(
    'Explicit --approve-tools required: isolated Git worktrees do not confine host shell processes');
  if(!out.maxIterations || out.maxIterations>64 || !out.tokenBudget ||
    out.tokenBudget>2_000_000 || !out.timeoutMs)throw new Error('Invalid benchmark budgets');
  out.metricsPath ??= out.output + '.metrics.jsonl';
  if(out.metricsPath===out.output)throw new Error('Predictions and metrics must use separate paths');
  return out;
}

async function git(cwd, ...args) {
  const {stdout}=await execute('git',['-C',cwd,...args],{
    timeout:90_000, maxBuffer:PATCH_LIMIT+1,
    env:{...process.env,GIT_TERMINAL_PROMPT:'0'}
  });
  return stdout;
}

/** Capture actual tracked and newly created source files from a pinned clone. */
export async function capturePatch(checkout, baseCommit) {
  if(!SHA.test(baseCommit))throw new Error('Unpinned Git baseline');
  // Intent-to-add makes new source files visible in the binary-safe diff.
  await git(checkout,'add','--intent-to-add','--all');
  const patch=await git(checkout,'diff','--binary','--no-ext-diff','--no-color',baseCommit);
  if(Buffer.byteLength(patch)>PATCH_LIMIT)throw new RangeError('Git patch exceeded 8 MiB');
  return patch;
}

async function readCompleted(path) {
  try{
    const rows=parseJsonl(await readFile(path,'utf8'));
    const ids=new Set();
    for(const row of rows){
      if(typeof row.instance_id!=='string'||typeof row.model_patch!=='string') {
        throw new Error('Invalid resumed prediction file');
      }
      if(ids.has(row.instance_id))throw new Error('Duplicate predictions for '+row.instance_id);
      ids.add(row.instance_id);
    }
    return ids;
  }catch(error){
    if(error.code==='ENOENT')return new Set();
    throw error;
  }
}

async function runCase(row,cfg,provider,repoRoot) {
  const start=Date.now();
  const workspaceRoot=await mkdtemp(join(tmpdir(),'dominion-swebench-'));
  const localRepo=await realpath(join(repoRoot,...row.repo.split('/')));
  if(!localRepo.startsWith(repoRoot+sep))throw new Error('Git clone escapes --repos');
  const checkout=join(workspaceRoot,'repo');
  let worktree=false, runtime=null, record=null, patch='',executions=[],operations=[],failure=null;
  try{
    await git(localRepo,'cat-file','-e',row.base_commit+'^{commit}');
    await git(localRepo,'worktree','add','--detach',checkout,row.base_commit);
    worktree=true;
    runtime=await DominionRuntime.start({
      binaryPath:cfg.binary,workspace:checkout,dataDir:join(workspaceRoot,'data'),
      port:0,startupTimeoutMs:30_000
    });
    const client=runtime.client;
    const saved=await client.request('POST','api/providers',provider);
    if(!saved?.id)throw new Error('Rust engine did not register the benchmark provider');
    await client.request('POST','api/policy',{
      autoApproveAll:false,autoApproveMcp:false,autoApprove:TOOL_NAMES
    });
    const goalText='Implement the following software issue by editing the repository. '
      +'Use actual files and tools, and check results. Do not invent a patch or claim '
      +'hidden-test success. The initial checkout is pinned to the official base commit.\n\n'
      +row.problem_statement;
    const id=saved.id;
    record=await client.goals.run({
      goal:goalText,
      executor:{providerId:id,model:provider.model},
      planner:{providerId:id,model:provider.model},
      reviewer:{providerId:id,model:provider.model},
      isolateWorktree:false,publishPr:false,
      maxIterations:cfg.maxIterations,maxReplans:4,replanAfter:2,
      tokenBudget:cfg.tokenBudget,successCriteria:[],
    },{timeoutMs:cfg.timeoutMs,pollIntervalMs:1_000});
    executions=await client.request('GET','api/executions?limit=1000');
  }catch(error){
    failure=String(error?.message||error).slice(0,1500);
  }finally{
    if(runtime){
      // Collect the native dispatcher evidence even after a goal fails. The
      // operation ledger is the authority for recoveries, not a model claim.
      try{
        if(!executions.length)executions=await runtime.client.request(
          'GET','api/executions?limit=1000');
        for(const run of executions){
          if(typeof run?.id!=='string')continue;
          const rows=await runtime.client.request('GET',
            'api/executions/'+encodeURIComponent(run.id)+'/operations');
          if(Array.isArray(rows))operations.push(...rows);
        }
      }catch(error){
        failure ??= 'Operation evidence unavailable: '+String(error.message).slice(0,350);
      }
    }
    if(runtime)await runtime.close().catch(()=>{});
    if(worktree){
      try{
        // Intent-to-add includes new untracked source files in the exact diff.
        // The checkout is disposable and never touches the owner's worktree.
        patch=await capturePatch(checkout,row.base_commit);
      }catch(error){failure ??= 'Patch capture failed: '+String(error.message).slice(0,600);}
      await git(localRepo,'worktree','remove','--force',checkout).catch(()=>{});
    }
    if(!cfg.keep)await rm(workspaceRoot,{recursive:true,force:true});
  }
  const stats=metrics(record,executions,Date.now()-start,patch,operations,cfg.pricing);
  stats.instanceId=row.instance_id;
  stats.failure=failure;
  stats.sourceRepo=row.repo;
  stats.baseCommit=row.base_commit;
  return {prediction:prediction(row,'dominion-rust/'+provider.model,patch),stats};
}

export async function main(args=process.argv.slice(2)) {
  const cfg=options(args);
  const repoRoot=await realpath(cfg.repos);
  const provider=JSON.parse(await readFile(cfg.provider,'utf8'));
  if(!provider?.baseUrl || !provider?.model)throw new Error('Provider JSON requires baseUrl and model');
  cfg.pricing=cfg.pricingPath?JSON.parse(await readFile(cfg.pricingPath,'utf8')):null;
  if(cfg.pricing)estimatedKrw({promptTokens:1,completionTokens:1},cfg.pricing);
  if(!provider.apiKey && process.env.DOMINION_BENCH_API_KEY){
    provider.apiKey=process.env.DOMINION_BENCH_API_KEY;
  }
  const instances=parseJsonl(await readFile(cfg.dataset,'utf8')).map(validateInstance);
  const seen=new Set();
  for(const row of instances){
    if(seen.has(row.instance_id))throw new Error('Duplicate instance_id '+row.instance_id);
    seen.add(row.instance_id);
  }
  const completed=await readCompleted(cfg.output);
  if(completed.size && !cfg.resume)throw new Error('Output already exists; explicitly pass --resume');
  await mkdir(dirname(cfg.output),{recursive:true});
  await mkdir(dirname(cfg.metricsPath),{recursive:true});
  const selected=cfg.limit ? instances.slice(0,cfg.limit) : instances;
  for(const row of selected){
    if(completed.has(row.instance_id))continue;
    const {prediction:result,stats}=await runCase(row,cfg,provider,repoRoot);
    await appendFile(cfg.output,JSON.stringify(result)+'\n');
    await appendFile(cfg.metricsPath,JSON.stringify(stats)+'\n');
    process.stdout.write(JSON.stringify({
      instance_id:row.instance_id,goalStatus:stats.goalStatus,
      patchBytes:stats.patchBytes,executedToolCalls:stats.executedToolCalls,
      totalTokens:stats.totalTokens,officialSWEbenchVerdict:null,
      failure:stats.failure
    })+'\n');
  }
}

if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  main().catch(error=>{console.error('SWE-bench driver:',error.message);process.exitCode=1;});
}
