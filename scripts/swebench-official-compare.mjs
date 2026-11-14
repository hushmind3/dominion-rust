#!/usr/bin/env node
// Independently graded, identical-instance SWE-bench comparison.
// This code never scores model claims or missing Docker reports.
import {createHash} from 'node:crypto'
import {readFile} from 'node:fs/promises'
import {join,resolve} from 'node:path'
import {pathToFileURL} from 'node:url'
const hash=bytes=>createHash('sha256').update(bytes).digest('hex')
const requireValid=(condition,message)=>{if(!condition)throw Error(message)}
const validId=s=>typeof s==='string'&&s.length>0&&s.length<=180
    &&/^[A-Za-z0-9_.-]+$/.test(s)&&s!=='.'&&s!=='..'
const verified=s=>['verified','SWE-bench/SWE-bench_Verified',
    'princeton-nlp/SWE-bench_Verified'].includes(s)
const json=async p=>JSON.parse(await readFile(p,'utf8'))
const lines=s=>s.split(/\r?\n/).filter(t=>t.trim()).map(JSON.parse)
const identical=(a,b)=>JSON.stringify(a)===JSON.stringify(b)
function index(rows,kind){
    const result=new Map()
    for(const item of rows){
        const id=item?.instance_id
        requireValid(validId(id)&&!result.has(id),'Invalid or duplicate '+kind+' '+id)
        result.set(id,item)
    }
    return result
}
function modelPath(label){
    requireValid(typeof label==='string'&&label.length<=250
        &&label.split('/').every(validId),'Unsafe model label')
    return label.replaceAll('/','__')
}
export async function readOfficialRun(output,logs){
    const [manifest,recordsText,predictionsText,grader]=await Promise.all([
        json(join(output,'manifest.json')),readFile(join(output,'records.jsonl'),'utf8'),
        readFile(join(output,'predictions.jsonl'),'utf8'),json(join(logs,'run.json'))
    ])
    requireValid(manifest.suite==='SWE-bench'&&verified(manifest.dataset)
        &&verified(grader.dataset)&&grader.split==='test',
        'Official and native source must both be SWE-bench Verified test')
    requireValid(/^[a-f0-9]{64}$/i.test(manifest.sourceSha256||''),
        'Dataset source hash is missing')
    const ids=manifest.selectedInstances
    requireValid(Array.isArray(ids)&&ids.length>0
        &&ids.every(validId)&&new Set(ids).size===ids.length,'Invalid native task IDs')
    const records=index(lines(recordsText),'receipt')
    const predictions=index(lines(predictionsText),'prediction')
    requireValid(records.size===ids.length&&predictions.size===ids.length
        &&ids.every(id=>records.has(id)&&predictions.has(id)),
        'Native predictions, receipts and task list are different')
    const scored=new Map()
    for(const id of ids){
        const receipt=records.get(id),prediction=predictions.get(id)
        requireValid(typeof prediction.model_patch==='string'
            &&typeof prediction.model_name_or_path==='string'
            &&typeof receipt.repo==='string'
            &&/^[a-f0-9]{40}$/i.test(receipt.baseCommit||''),
            'Malformed source identity or prediction for '+id)
        const patch=prediction.model_patch
        requireValid(receipt.patchBytes===Buffer.byteLength(patch)
            &&receipt.patchSha256===hash(patch),
            'Native patch hash mismatch '+id)
        let grade={officialSolved:false,proofSha256:null,source:'empty_prediction'}
        if(patch.length>0){
            const path=join(logs,modelPath(prediction.model_name_or_path),id,'report.json')
            let data
            try{data=await readFile(path)}catch(error){
                if(error.code==='ENOENT')throw Error('Missing official Docker report '+id)
                throw error
            }
            let parsed
            try{parsed=JSON.parse(data.toString('utf8'))}catch{
                throw Error('Malformed official Docker report '+id)
            }
            requireValid(typeof parsed?.[id]?.resolved==='boolean',
                'Official Docker report has no resolved boolean '+id)
            grade={officialSolved:parsed[id].resolved,proofSha256:hash(data),
                source:'official_swebench_docker_report'}
        }
        scored.set(id,{...receipt,...grade})
    }
    return {manifest,grader,records:scored}
}
function summarize(records){
    const rows=[...records.values()],solved=rows.filter(r=>r.officialSolved).length
    const all=(name)=>rows.every(r=>Number.isSafeInteger(r[name])&&r[name]>=0)
        ?rows.reduce((s,r)=>s+r[name],0):null
    const tokens=rows.every(r=>Number.isSafeInteger(r.usage?.totalTokens)
        &&r.usage.totalTokens>=0)
        ?rows.reduce((s,r)=>s+r.usage.totalTokens,0):null
    return {attempted:rows.length,solved,resolvedRate:solved/rows.length,
        recordedTokens:tokens,
        nonEstimatedTokens:rows.every(r=>r.usage?.estimatedCalls===0)?tokens:null,
        toolCalls:all('toolCalls'),durationMs:all('elapsedMs'),
        tokensPerResolved:solved&&tokens!==null?tokens/solved:null,
        actualBilledKrw:null}
}
export function compareOfficialRuns(left,right){
    const ids=[...left.records.keys()].sort()
    requireValid(identical(ids,[...right.records.keys()].sort())
        &&left.manifest.sourceSha256===right.manifest.sourceSha256,
        'Task set or benchmark source differs; cannot cherry-pick denominators')
    for(const id of ids){
        const a=left.records.get(id),b=right.records.get(id)
        requireValid(a.repo===b.repo&&a.baseCommit===b.baseCommit,
            'Different historical Git source for '+id)
    }
    const a=left.manifest,b=right.manifest
    const sameModel=a.model===b.model&&a.providerKind===b.providerKind
        &&a.providerUrl===b.providerUrl
    const sameBudgets=identical(a.limits,b.limits)
        &&a.timeoutSeconds===b.timeoutSeconds
    const sameTools=Array.isArray(a.ownerApprovedTools)
        &&Array.isArray(b.ownerApprovedTools)
        &&identical([...a.ownerApprovedTools].sort(),[...b.ownerApprovedTools].sort())
    const sameIsolation=a.sourceIsolation===b.sourceIsolation
        &&a.sourceIsolation==='sealed-single-commit'
        &&a.sharedGitObjects===false&&b.sharedGitObjects===false
    let both=0,leftOnly=0,rightOnly=0,neither=0
    const paired=ids.map(id=>{
        const x=left.records.get(id),y=right.records.get(id)
        if(x.officialSolved&&y.officialSolved)both++
        else if(x.officialSolved)leftOnly++
        else if(y.officialSolved)rightOnly++
        else neither++
        return {instance_id:id,leftSolved:x.officialSolved,
            rightSolved:y.officialSolved,leftReportSha256:x.proofSha256,
            rightReportSha256:y.proofSha256}
    })
    return {dataset:'SWE-bench Verified',totalInstances:ids.length,
        sourceSha256:a.sourceSha256,
        evidence:'original_per_instance_Docker_report_json',
        controlled:sameModel&&sameBudgets&&sameTools&&sameIsolation,
        controls:{sameModel,sameBudgets,sameTools,sameIsolation},
        left:summarize(left.records),right:summarize(right.records),
        paired:{bothSolved:both,leftOnlySolved:leftOnly,rightOnlySolved:rightOnly,
            neitherSolved:neither,netLeftAdvantage:leftOnly-rightOnly},
        taskOutcomes:paired}
}
function options(argv){
    const values={}
    for(let i=0;i<argv.length;i+=2){
        requireValid(argv[i]?.startsWith('--')&&argv[i+1],'Expected --key value')
        values[argv[i].slice(2)]=argv[i+1]
    }
    for(const field of ['left','right','left-logs','right-logs'])
        requireValid(values[field],'Required --'+field)
    return values
}
if(process.argv[1]&&import.meta.url===pathToFileURL(resolve(process.argv[1])).href){
    try{
        const q=options(process.argv.slice(2))
        const [a,b]=await Promise.all([
            readOfficialRun(resolve(q.left),resolve(q['left-logs'])),
            readOfficialRun(resolve(q.right),resolve(q['right-logs']))
        ])
        console.log(JSON.stringify(compareOfficialRuns(a,b),null,2))
    }catch(error){console.error(error.message);process.exitCode=1}
}
