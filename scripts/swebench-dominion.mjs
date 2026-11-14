#!/usr/bin/env node
// Real SWE-bench prediction producer for the native Dominion Rust engine.
// Uses a separate Git worktree, SQLite store and server per task. Gold patch,
// test_patch and test outcomes are NEVER sent to the autonomous goal.
import {createHash, randomBytes} from 'node:crypto'
import {execFile as execFileCb, spawn} from 'node:child_process'
import {createServer} from 'node:net'
import {promisify} from 'node:util'
import {readFile, writeFile, appendFile, mkdir, open, realpath} from 'node:fs/promises'
import {resolve, join, dirname, isAbsolute, relative} from 'node:path'
import {pathToFileURL} from 'node:url'
const execFile=promisify(execFileCb)
const terminal=new Set(['completed','failed','cancelled','needs_review'])
const wait=ms=>new Promise(done=>setTimeout(done,ms))
const sha=text=>createHash('sha256').update(text).digest('hex')
const clean=value=>typeof value==='string' && /^[a-zA-Z0-9_.-]+$/.test(value)
    && value!=='.' && value!=='..'
const gitSha=value=>typeof value==='string'&&/^[a-f0-9]{40}$/i.test(value)
const repoId=value=>typeof value==='string'&&/^[\w.-]+\/[\w.-]+$/.test(value)
    && value.split('/').every(clean)
const inside=(parent,target)=>{
    const diff=relative(resolve(parent),resolve(target))
    return diff!==''&&!diff.startsWith('..')&&!isAbsolute(diff)
}
export function validateInstance(row){
    if(!row||!clean(row.instance_id)||row.instance_id.length>180
       ||!repoId(row.repo)||!gitSha(row.base_commit)
       ||typeof row.problem_statement!=='string'||!row.problem_statement.trim()
       ||row.problem_statement.length>250_000){
        throw Error('invalid SWE-bench public instance fields')
    }
    // Construct an entirely new object: no gold patch, test patch, fail-to-pass
    // or evaluation metadata can accidentally travel into the model prompt.
    return {id:row.instance_id,repo:row.repo,base:row.base_commit,
        problem:row.problem_statement}
}
export function goalPayload(instance,providerId,model,limits){
    return {goal:'Resolve this repository issue from the checked-out base revision. '
        +'Investigate the source, implement the fix and run relevant tests. '
        +'Preserve existing unrelated behavior.\n\n'+instance.problem,
        successCriteria:[],isolateWorktree:true,
        executor:{providerId,model},planner:{providerId,model},
        reviewer:{providerId,model},
        tokenBudget:limits.tokens,maxIterations:limits.iterations,
        maxReplans:limits.replans,replanAfter:2}
}
/** Recovery is proven only by a later successful call in the SAME run
 * to the same resource. It is a tool-level metric, not an official solution. */
export function analyzeOperations(operations){
    if(!Array.isArray(operations))throw Error('operation ledger is missing')
    const seen=new Set()
    const rows=operations.map(op=>{
        if(!op||typeof op.id!=='string'||seen.has(op.id)
           ||!Number.isSafeInteger(op.opSeq))throw Error('invalid or duplicate tool receipt')
        seen.add(op.id)
        return op
    }).sort((a,b)=>String(a.runId).localeCompare(String(b.runId))
        ||a.opSeq-b.opSeq)
    const unknown=op=>op.outcomeUnknown===true||op.effectState==='effect_unknown'
       ||op.result?.outcomeUnknown===true
    const failed=op=>['failed','denied','error','needs_review'].includes(op.status)
       ||Boolean(op.result?.error)
    const succeeded=op=>op.status==='completed'&&!failed(op)&&!unknown(op)
    const failures=rows.filter(failed)
    const recovered=failures.filter(error=>rows.some(next=>
        next.runId===error.runId&&next.opSeq>error.opSeq
        &&succeeded(next)
        &&(error.resourceKey&&next.resourceKey===error.resourceKey
            ||!error.resourceKey&&next.tool===error.tool
                &&JSON.stringify(next.arguments?.path||next.arguments?.cwd||null)
                    ===JSON.stringify(error.arguments?.path||error.arguments?.cwd||null))
    )).length
    return {total:rows.length,failed:failures.length,
        successful:rows.filter(succeeded).length,
        unknownEffects:rows.filter(unknown).length,
        verifiedEffects:rows.filter(op=>op.toolReceipt?.verified===true).length,
        recoveredFailures:recovered,
        recoveryRate:failures.length?recovered/failures.length:null}
}
export function summarize(records){
    const completed=records.filter(x=>x.goalStatus==='completed').length
    const usage=records.filter(x=>Number.isSafeInteger(x.usage?.totalTokens)
        &&x.usage.totalTokens>=0)
    const reported=usage.reduce((sum,x)=>sum+x.usage.totalTokens,0)
    const tools=records.filter(x=>Number.isSafeInteger(x.toolLedger?.total))
    const totalTools=tools.reduce((sum,x)=>sum+x.toolLedger.total,0)
    const failures=tools.reduce((sum,x)=>sum+x.toolLedger.failed,0)
    const recovered=tools.reduce((sum,x)=>sum+x.toolLedger.recoveredFailures,0)
    return {attempted:records.length,goalCompleted:completed,
        predictionPatches:records.filter(x=>x.patchBytes>0).length,
        estimatedOrReportedTokens:usage.length===records.length?reported:null,
        tokenMeasurementCoverage:records.length?usage.length/records.length:null,
        measuredTokenSubtotal:usage.length?reported:null,
        toolCalls:tools.length===records.length?totalTools:null,
        toolMeasurementCoverage:records.length?tools.length/records.length:null,
        observedToolFailures:tools.length===records.length?failures:null,
        observedToolRecoveryRate:tools.length===records.length&&failures>0
            ?recovered/failures:null,
        actualBilledKrw:null,
        officialResolved:null,
        note:'Internal completion and patch existence are NOT SWE-bench correctness. Use the official Docker evaluator.'}
}
function parseArgs(argv){
    const [action,...flags]=argv
    const options={}
    for(let i=0;i<flags.length;i++){
        const flag=flags[i]
        if(!flag.startsWith('--')||flag.length<3||i+1>=flags.length)throw Error('Expected --key value')
        options[flag.slice(2)]=flags[++i]
    }
    return {action,options}
}
function boundedInt(value,name,low,high,defaultValue){
    if(value===undefined)return defaultValue
    const n=Number(value)
    if(!Number.isSafeInteger(n)||n<low||n>high)throw Error('Invalid '+name)
    return n
}
export async function loadInstances(file,filter,max){
    const content=await readFile(file,'utf8')
    const items=[]
    const seen=new Set()
    for(const line of content.split(/\r?\n/)){
        if(!line.trim())continue
        const instance=validateInstance(JSON.parse(line))
        if(filter&&filter!==instance.id)continue
        if(seen.has(instance.id))throw Error('duplicate instance_id '+instance.id)
        seen.add(instance.id)
        items.push(instance)
        if(items.length>=max)break
    }
    if(items.length===0)throw Error('no matching benchmark cases')
    return items
}
async function command(binary,args,opts={}){
    return execFile(binary,args,{timeout:opts.timeout||90000,
        maxBuffer:32*1024*1024,cwd:opts.cwd,env:opts.env})
}
async function freePort(){
    return new Promise((done,fail)=>{
        const server=createServer()
        server.once('error',fail)
        server.listen(0,'127.0.0.1',()=>{
            const port=server.address().port
            server.close(err=>err?fail(err):done(port))
        })
    })
}
async function request(base,path,method='GET',body,token,timeout=15000){
    const headers={'accept':'application/json'}
    if(body!==undefined)headers['content-type']='application/json'
    if(token)headers['x-dominion-token']=token
    const response=await fetch(base+path,{method,headers,
        body:body===undefined?undefined:JSON.stringify(body),
        signal:AbortSignal.timeout(timeout)})
    const raw=await response.text()
    let data
    try{data=JSON.parse(raw)}catch{throw Error('non-JSON server response '+response.status)}
    if(!response.ok)throw Error('HTTP '+response.status+': '+String(data.error||raw).slice(0,500))
    return data
}
async function started(server,base,expected,source,data){
    const deadline=Date.now()+30000
    while(Date.now()<deadline){
        if(server.exitCode!==null)throw Error('Dominion exited before readiness')
        try{
            const health=await request(base,'/api/health')
            if(health.runtime!=='rust')throw Error('not native Rust engine')
            const identity=await request(base,'/api/benchmark/identity')
            if(identity.benchmark!==true||identity.instanceId!==expected
                ||resolve(identity.workspace)!==resolve(source)
                ||resolve(identity.dataRoot)!==resolve(data)
                ||identity.pid!==server.pid){
                throw Error('benchmark identity mismatch (unrelated server or wrong workspace)')
            }
            return identity
        }catch(error){
            if(String(error.message).includes('identity mismatch')||String(error.message).includes('not native'))throw error
            await wait(200)
        }
    }
    throw Error('Dominion readiness timeout')
}
async function terminate(server){
    if(!server||server.exitCode!==null)return
    const done=new Promise(resolveDone=>server.once('exit',resolveDone))
    try{
        if(process.platform!=='win32'&&server.pid)process.kill(-server.pid,'SIGTERM')
        else server.kill('SIGTERM')
    }catch{server.kill('SIGTERM')}
    await Promise.race([done,wait(2000)])
    if(server.exitCode===null){
        try{
            if(process.platform!=='win32'&&server.pid)process.kill(-server.pid,'SIGKILL')
            else server.kill('SIGKILL')
        }catch{server.kill('SIGKILL')}
        await Promise.race([done,wait(1000)])
    }
}
async function findMirror(root,repo){
    const pieces=repo.split('/')
    for(const candidate of [join(root,pieces[0]+'__'+pieces[1]+'.git'),
        join(root,pieces[0],pieces[1]),join(root,pieces[0]+'__'+pieces[1])]){
        try{
            const location=await realpath(candidate)
            if(!inside(root,location))continue
            await command('git',['-C',location,'rev-parse','--git-dir'])
            return location
        }catch{}
    }
    throw Error('local Git mirror missing for '+repo)
}
async function capturePatch(workspace,base){
    const status=(await command('git',['-C',workspace,'status','--porcelain','--untracked-files=all'])).stdout
    // Intent-to-add exposes newly created files in an ordinary Git diff,
    // without staging their content or altering the checked-out source files.
    await command('git',['-C',workspace,'-c','core.hooksPath=/dev/null',
        'add','-N','-A','--','.'])
    const patch=(await command('git',['-C',workspace,'diff','--binary',
        '--no-ext-diff','--no-color',base])).stdout
    return {patch,status}
}
async function toolsUsed(base,session){
    if(!session)return null
    try{
        const runs=await request(base,'/api/executions')
        if(!Array.isArray(runs))return null
        const operations=[]
        for(const run of runs.filter(r=>r.sessionId===session)){
            const ops=await request(base,'/api/executions/'+encodeURIComponent(run.id)+'/operations')
            if(!Array.isArray(ops))return null
            operations.push(...ops)
        }
        return analyzeOperations(operations)
    }catch{return null}
}
export async function runCase(instance,options){
    const folder=join(options.output,'instances',instance.id)
    const source=join(folder,'source'),data=join(folder,'data')
    await mkdir(folder,{recursive:true});await mkdir(data,{recursive:true})
    const mirror=await findMirror(options.reposRoot,instance.repo)
    const begin=Date.now()
    const token=randomBytes(32).toString('hex')
    const port=await freePort()
    const base='http://127.0.0.1:'+port
    let server,log,worktree=false,goal=null,patch='',patchStatus=''
    let error=null,identity=null,tools=null
    try{
        await command('git',['-C',mirror,'worktree','add','--detach',source,instance.base])
        worktree=true
        log=await open(join(folder,'server.log'),'w',0o600)
        server=spawn(options.binary,[],{cwd:source,detached:process.platform!=='win32',
            env:{...process.env,DOMINION_BENCHMARK:'1',
                DOMINION_ENGINE_INSTANCE:instance.id,
                DOMINION_ADMIN_TOKEN:token,DOMINION_PORT:String(port),
                DOMINION_DATA:data,DOMINION_WORKSPACE:source,
                DOMINION_WEB:join(folder,'web-none')},
            stdio:['ignore',log.fd,log.fd]})
        identity=await started(server,base,instance.id,source,data)
        const provider={id:'benchmark',name:'Benchmark provider',
            baseUrl:options.providerUrl,model:options.model,
            kind:options.providerKind,enabled:true,
            ...(options.apiKey?{apiKey:options.apiKey}:{})}
        await request(base,'/api/providers','POST',provider,token)
        goal=await request(base,'/api/goals','POST',
            goalPayload(instance,'benchmark',options.model,options.limits),token)
        const deadline=Date.now()+options.timeoutSeconds*1000
        while(Date.now()<deadline){
            goal=await request(base,'/api/goals/'+encodeURIComponent(goal.id))
            if(terminal.has(goal.status))break
            if(server.exitCode!==null)throw Error('runtime stopped while goal was running')
            await wait(900)
        }
        if(!terminal.has(goal.status)){
            try{await request(base,'/api/goals/'+encodeURIComponent(goal.id)+'/cancel','POST',{},token)}
            catch{}
            error='goal timeout'
        }
        goal=await request(base,'/api/goals/'+encodeURIComponent(goal.id))
        tools=await toolsUsed(base,goal.sessionId)
        // Do not conflate internal goal status with externally graded patch.
        const path=goal.gitWorktree?.path
        if(path){
            const real=await realpath(path)
            if(!inside(data,real))throw Error('goal worktree escaped its isolated data root')
            const result=await capturePatch(real,instance.base)
            patch=result.patch;patchStatus=result.status
        }
    }catch(cause){error=String(cause?.message||cause).slice(0,1200)}
    finally{
        await terminate(server)
        if(log)await log.close()
        if(worktree){
            try{await command('git',['-C',mirror,'worktree','remove','--force',source])}
            catch{/* keep the worktree for manual inspection if Git refuses */}
        }
    }
    if(patch.length>2_000_000){error='patch exceeds benchmark output limit';patch=''}
    const record={instance_id:instance.id,repo:instance.repo,baseCommit:instance.base,
        goalId:goal?.id||null,goalStatus:goal?.status||'infrastructure_error',
        internalVerified:goal?.metrics?.verifiedCompleted===true,
        usage:goal?.metrics?.usage||null,toolLedger:tools,
        toolCalls:tools?.total??null,
        elapsedMs:Date.now()-begin,patchBytes:Buffer.byteLength(patch),
        patchSha256:sha(patch),gitStatus:patchStatus.slice(0,4000),
        runtimeCommit:identity?.commit||null,
        failure:error,officialResolved:null}
    return {prediction:{instance_id:instance.id,
        model_name_or_path:options.modelLabel,model_patch:patch},record}
}
async function run(options){
    for(const option of ['instances','repos-root','binary','output','provider-url','model']){
        if(!options[option])throw Error('--'+option+' is required')
    }
    const root=resolve(options['repos-root'])
    const binary=resolve(options.binary),output=resolve(options.output)
    const cases=await loadInstances(resolve(options.instances),options['instance-id'],
        boundedInt(options['max-instances'],'max-instances',1,10000,10000))
    const keyName=options['api-key-env']
    if(keyName&&!(keyName in process.env))throw Error('API key environment variable is not set')
    const url=new URL(options['provider-url'])
    if(!['https:','http:'].includes(url.protocol)||url.username||url.password)
        throw Error('provider URL must be http(s) and contain no embedded credentials')
    const opts={reposRoot:root,binary,output,providerUrl:options['provider-url'],
        providerKind:options['provider-kind']||'openai',model:options.model,
        apiKey:keyName?process.env[keyName]:null,
        modelLabel:options['model-label']||'dominion-rust/'+options.model,
        limits:{tokens:boundedInt(options['token-budget'],'token-budget',1,2_000_000,150_000),
            iterations:boundedInt(options.iterations,'iterations',1,64,16),
            replans:boundedInt(options.replans,'replans',0,16,6)},
        timeoutSeconds:boundedInt(options['timeout-seconds'],'timeout-seconds',15,28800,1200)}
    if(!['openai','anthropic','ollama'].includes(opts.providerKind))throw Error('invalid provider kind')
    await mkdir(output,{recursive:true})
    const dataset=await readFile(resolve(options.instances))
    const runManifest={suite:'SWE-bench',sourceFile:resolve(options.instances),
        sourceSha256:sha(dataset),selectedInstances:cases.map(c=>c.id),
        model:opts.model,providerKind:opts.providerKind,
        providerUrl:opts.providerUrl,apiKeyEnvironment:keyName||null,
        timeoutSeconds:opts.timeoutSeconds,limits:opts.limits,
        worktreeIsolation:true,runtimeBinary:binary,
        officialEvaluation:false,createdAt:new Date().toISOString()}
    await writeFile(join(output,'manifest.json'),JSON.stringify(runManifest,null,2))
    const predictions=join(output,'predictions.jsonl')
    const records=join(output,'records.jsonl')
    await writeFile(predictions,'');await writeFile(records,'')
    const observations=[]
    for(const instance of cases){
        const result=await runCase(instance,opts)
        await appendFile(predictions,JSON.stringify(result.prediction)+'\n')
        await appendFile(records,JSON.stringify(result.record)+'\n')
        observations.push(result.record)
        console.log(instance.id+': goal='+result.record.goalStatus+
            ' patchBytes='+result.record.patchBytes+
            (result.record.failure?' error='+result.record.failure:''))
    }
    const summary=summarize(observations)
    await writeFile(join(output,'summary.json'),JSON.stringify(summary,null,2))
    console.log(JSON.stringify(summary,null,2))
    console.log('Official correctness: NOT GRADED. Run: swebench eval verified -p '+
        predictions+' --run-id <id> -j 4')
}
async function evaluate(options){
    if(!options.predictions||!options['run-id'])throw Error('need --predictions and --run-id')
    if(!clean(options['run-id']))throw Error('unsafe --run-id')
    const dataset=options.dataset||'verified'
    if(!/^[a-zA-Z0-9_.\/-]+$/.test(dataset))throw Error('invalid dataset')
    const workers=boundedInt(options.workers,'workers',1,128,4)
    const args=['eval',dataset,'-p',resolve(options.predictions),
        '--run-id',options['run-id'],'-j',String(workers)]
    const child=spawn('swebench',args,{stdio:'inherit'})
    const code=await new Promise((done,reject)=>{
        child.once('error',reject);child.once('exit',value=>done(value??1))
    })
    if(code!==0)throw Error('official SWE-bench evaluator failed with exit code '+code)
}
async function main(){
    const {action,options}=parseArgs(process.argv.slice(2))
    if(action==='run')await run(options)
    else if(action==='evaluate')await evaluate(options)
    else throw Error('usage: node scripts/swebench-dominion.mjs run --instances FILE --repos-root PATH --binary PATH --output DIR --provider-url URL --model MODEL [--api-key-env ENV] [--max-instances N] or evaluate --predictions FILE --run-id ID')
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
    main().catch(error=>{console.error(error.message);process.exitCode=1})
}
