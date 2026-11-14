import {test} from 'node:test'
import assert from 'node:assert/strict'
import {mkdtemp,mkdir,writeFile,readFile,rm} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {join} from 'node:path'
import {validateInstance,goalPayload,loadInstances,summarize,analyzeOperations,approvalPolicy} from './swebench-dominion.mjs'

const base='a'.repeat(40)
const row={instance_id:'django__django-1001',repo:'django/django',
    base_commit:base,problem_statement:'Fix request parsing.',
    patch:'GOLD_SECRET',test_patch:'HIDDEN_SECRET',FAIL_TO_PASS:'PRIVATE_TEST'}
test('public task payload excludes all gold patches and hidden test metadata',()=>{
    const parsed=validateInstance(row)
    assert.deepEqual(Object.keys(parsed),['id','repo','base','problem'])
    const body=goalPayload(parsed,'benchmark','model', {tokens:20000,iterations:7,replans:2})
    assert.match(body.goal,/Fix request parsing/)
    assert.equal(body.isolateWorktree,true)
    assert.equal(body.executor.providerId,'benchmark')
    assert.equal(body.tokenBudget,20000)
    assert.ok(!JSON.stringify(body).includes('GOLD_SECRET'))
    assert.ok(!JSON.stringify(body).includes('HIDDEN_SECRET'))
    assert.ok(!JSON.stringify(body).includes('PRIVATE_TEST'))
})
test('reject malicious paths, invalid task shape and accidental duplicate cases',async()=>{
    for(const mutation of [
        {instance_id:'../../secrets'},{instance_id:'..'},
        {repo:'../../private'},{repo:'ORG/../PRIVATE'},
        {base_commit:'HEAD'},{problem_statement:''}
    ]) assert.throws(()=>validateInstance({...row,...mutation}))
    const dir=await mkdtemp(join(tmpdir(),'dominion-swebench-test-'))
    try{
        const file=join(dir,'cases.jsonl')
        await writeFile(file,JSON.stringify(row)+'\n'+JSON.stringify(row)+'\n')
        await assert.rejects(loadInstances(file,null,10),/duplicate instance_id/)
        await assert.rejects(loadInstances(file,'missing',10),/no matching/)
        assert.equal((await loadInstances(file,null,1)).length,1)
    }finally{await rm(dir,{recursive:true,force:true})}
})
test('a terminal internal goal and a valid Git patch are never official pass claims',()=>{
    const results=[{goalStatus:'completed',patchBytes:100,usage:{totalTokens:200},toolCalls:3},
        {goalStatus:'needs_review',patchBytes:30,usage:null,toolCalls:null},
        {goalStatus:'failed',patchBytes:0,usage:{totalTokens:90},toolCalls:5}]
    assert.deepEqual(summarize(results),{
        attempted:3,goalCompleted:1,predictionPatches:2,
        estimatedOrReportedTokens:null,tokenMeasurementCoverage:2/3,
        measuredTokenSubtotal:290,
        toolCalls:null,toolMeasurementCoverage:0,
        observedToolFailures:null,observedToolRecoveryRate:null,
        actualBilledKrw:null,officialResolved:null,
        note:'Internal completion and patch existence are NOT SWE-bench correctness. Use the official Docker evaluator.'
    })
})

test('durable operation recovery requires later same-run success; unknown side effects stay distinct',()=>{
    const operations=[
        {id:'f',runId:'run1',opSeq:1,tool:'workspace_replace',status:'failed',
            resourceKey:'workspace:src/lib.rs'},
        {id:'unrelated',runId:'run1',opSeq:2,tool:'file_write',status:'completed',
            resourceKey:'file:other.txt'},
        {id:'repair',runId:'run1',opSeq:3,tool:'workspace_replace',status:'completed',
            resourceKey:'workspace:src/lib.rs',toolReceipt:{verified:true}},
        {id:'unknown',runId:'run2',opSeq:1,tool:'github_api',status:'interrupted',
            resourceKey:'github:issue',effectState:'effect_unknown'}
    ]
    const m=analyzeOperations(operations)
    assert.deepEqual(m,{total:4,failed:1,successful:2,unknownEffects:1,
        verifiedEffects:1,recoveredFailures:1,recoveryRate:1})
    assert.equal(analyzeOperations(operations.slice(0,2)).recoveryRate,0)
    assert.equal(analyzeOperations([]).recoveryRate,null)
    assert.throws(()=>analyzeOperations([operations[0],operations[0]]),/duplicate/)
})

test('owner selects actual native tool grants for unattended benchmark',()=>{
    const policy=approvalPolicy('workspace_write,workspace_replace_range,shell_exec')
    assert.deepEqual(policy,{autoApproveAll:false,autoApproveMcp:false,
        autoApprove:['workspace_write','workspace_replace_range','shell_exec']})
    assert.throws(()=>approvalPolicy(''),/Explicit/)
    assert.throws(()=>approvalPolicy('github_api'),/Unsupported/)
    assert.throws(()=>approvalPolicy('workspace_write,workspace_write'),/duplicate/)
    assert.throws(()=>approvalPolicy('file_write'),/Unsupported/)
})
