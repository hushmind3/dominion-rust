import test from 'node:test'
import assert from 'node:assert/strict'
import {createHash} from 'node:crypto'
import {mkdir,mkdtemp,writeFile,rm} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {join} from 'node:path'
import {readOfficialRun,compareOfficialRuns} from './swebench-official-compare.mjs'
const hash=s=>createHash('sha256').update(s).digest('hex')
const ids=['django__django-1','django__django-2']
async function fixture(root,name,outcomes){
    const path=join(root,'run-'+name),logs=join(root,'logs-'+name)
    await mkdir(path,{recursive:true})
    await mkdir(logs,{recursive:true})
    const manifest={suite:'SWE-bench',dataset:'SWE-bench/SWE-bench_Verified',
        sourceSha256:'d'.repeat(64),selectedInstances:ids,
        model:'provider-model',providerKind:'openai',providerUrl:'https://host.test/v1',
        limits:{tokens:10000,iterations:5,replans:2},
        timeoutSeconds:1200,ownerApprovedTools:['shell_exec','workspace_write'],
        sourceIsolation:'sealed-single-commit',sharedGitObjects:false}
    await writeFile(join(path,'manifest.json'),JSON.stringify(manifest))
    await writeFile(join(logs,'run.json'),JSON.stringify({
        dataset:'SWE-bench/SWE-bench_Verified',split:'test'}))
    const predictions=ids.map((instance_id,i)=>({
        instance_id,model_name_or_path:'dominion-rust/model',
        model_patch:'diff --git a/x b/x\n+fix-'+i+'\n'
    }))
    const receipts=predictions.map(row=>({
        instance_id:row.instance_id,repo:'django/django',baseCommit:'a'.repeat(40),
        patchBytes:Buffer.byteLength(row.model_patch),patchSha256:hash(row.model_patch),
        goalStatus:'completed',toolCalls:3,elapsedMs:5000,
        usage:{totalTokens:200,estimatedCalls:0}
    }))
    await writeFile(join(path,'predictions.jsonl'),
        predictions.map(JSON.stringify).join('\n')+'\n')
    await writeFile(join(path,'records.jsonl'),
        receipts.map(JSON.stringify).join('\n')+'\n')
    for(const [i,id]of ids.entries()){
        const folder=join(logs,'dominion-rust__model',id)
        await mkdir(folder,{recursive:true})
        await writeFile(join(folder,'report.json'),
            JSON.stringify({[id]:{resolved:outcomes[i]}}))
    }
    return {path,logs,predictions,receipts,manifest}
}
test('paired official verdicts include all cases and measured cost',async()=>{
    const root=await mkdtemp(join(tmpdir(),'dominion-compare-'))
    try{
        const l=await fixture(root,'left',[true,false])
        const r=await fixture(root,'right',[false,true])
        const [a,b]=await Promise.all([
            readOfficialRun(l.path,l.logs),readOfficialRun(r.path,r.logs)])
        const report=compareOfficialRuns(a,b)
        assert.equal(report.controlled,true)
        assert.deepEqual(report.paired,{bothSolved:0,leftOnlySolved:1,
            rightOnlySolved:1,neitherSolved:0,netLeftAdvantage:0})
        assert.equal(report.left.recordedTokens,400)
        assert.equal(report.left.nonEstimatedTokens,400)
        assert.equal(report.right.toolCalls,6)
        assert.equal(report.left.resolvedRate,0.5)
        assert.equal(report.taskOutcomes[0].leftReportSha256.length,64)
        b.manifest.model='different'
        assert.equal(compareOfficialRuns(a,b).controlled,false)
        b.manifest.model=a.manifest.model
        b.manifest.limits.tokens=1234
        assert.equal(compareOfficialRuns(a,b).controlled,false)
    }finally{await rm(root,{recursive:true,force:true})}
})
test('refuse fake grader outputs, missing receipts and tampered native patch',async()=>{
    const root=await mkdtemp(join(tmpdir(),'dominion-compare-error-'))
    try{
        const q=await fixture(root,'bad',[true,false])
        await writeFile(join(q.logs,'dominion-rust__model',ids[0],'report.json'),
            JSON.stringify({[ids[0]]:{resolved:'true'}}))
        await assert.rejects(readOfficialRun(q.path,q.logs),/resolved boolean/)
        const f=q.predictions.map((r,i)=>i===0?{...r,model_patch:'tampered'}:r)
        await writeFile(join(q.path,'predictions.jsonl'),f.map(JSON.stringify).join('\n')+'\n')
        await assert.rejects(readOfficialRun(q.path,q.logs),/patch hash mismatch/)
        const good=q.predictions.map(JSON.stringify).join('\n')+'\n'
        await writeFile(join(q.path,'predictions.jsonl'),good)
        await writeFile(join(q.logs,'dominion-rust__model',ids[0],'report.json'),
            JSON.stringify({[ids[0]]:{resolved:true}}))
        await rm(join(q.logs,'dominion-rust__model',ids[1],'report.json'))
        await assert.rejects(readOfficialRun(q.path,q.logs),/Missing official Docker report/)
    }finally{await rm(root,{recursive:true,force:true})}
})
