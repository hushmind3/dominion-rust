#!/usr/bin/env node
/**
 * Independent A/B evidence collector for DOMINION-native vs GitHub Copilot SDK.
 * Requires TWO running DOMINION instances with different workspaces.
 * Never calls a paid provider without --approve-cloud.
 * Does not declare success without an independently known expected file hash.
 */
import {randomUUID,createHash} from 'node:crypto'
import {readFile,realpath,writeFile} from 'node:fs/promises'
import path from 'node:path'

function argsFrom(argv){
 const result={}
 for(let i=0;i<argv.length;i++){
  if(!argv[i].startsWith('--'))throw Error('Unexpected argument '+argv[i])
  const [name,embedded]=argv[i].slice(2).split('=',2)
  if(['approve-cloud','approve-tools'].includes(name)){result[name]=true;continue}
  result[name]=embedded??argv[++i]
 }
 return result
}
const args=argsFrom(process.argv.slice(2))
const required=['native-url','sdk-url','native-workflow','sdk-workflow','native-dir',
 'sdk-dir','task','oracle-file','oracle-sha256']
for(const name of required){
 if(!args[name])throw Error('Missing --'+name+'; no run launched')
}
if(!/^[0-9a-f]{64}$/i.test(args['oracle-sha256']))
 throw Error('Oracle SHA-256 must contain exactly 64 hex characters')
if(!['native','sdk'].includes(args.first||'native'))
 throw Error('--first must be native or sdk')
const nativeRoot=await realpath(args['native-dir'])
const sdkRoot=await realpath(args['sdk-dir'])
if(nativeRoot===sdkRoot)throw Error('Native and SDK workspaces must differ')
const maxMinutes=Math.min(60,Math.max(1,Number(args['max-minutes']||15)))
const baseURL={native:args['native-url'],sdk:args['sdk-url']}
const flowID={native:args['native-workflow'],sdk:args['sdk-workflow']}
const folders={native:nativeRoot,sdk:sdkRoot}
const tokens={native:process.env.DOMINION_NATIVE_TOKEN||process.env.DOMINION_ADMIN_TOKEN,
 sdk:process.env.DOMINION_SDK_TOKEN||process.env.DOMINION_ADMIN_TOKEN}
for(const key of ['native','sdk']){
 const u=new URL(baseURL[key])
 if(!['http:','https:'].includes(u.protocol))throw Error('HTTP(S) URL required')
}
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms))
async function api(which,method,route,data){
 const headers={'Accept':'application/json'}
 if(data!==undefined)headers['Content-Type']='application/json'
 if(tokens[which])headers['Authorization']='Bearer '+tokens[which]
 const response=await fetch(new URL(route,baseURL[which]),{
  method,headers,body:data===undefined?undefined:JSON.stringify(data),
  signal:AbortSignal.timeout(15_000)
 })
 const body=await response.json()
 if(!response.ok)throw Error(which+' API '+response.status+': '+String(body.error||'request rejected').slice(0,160))
 return body
}
async function definition(which){
 const all=await api(which,'GET','/api/workflows')
 const flow=all.find(item=>item.id===flowID[which])
 if(!flow)throw Error(which+' workflow not found')
 if(!Array.isArray(flow.steps)||flow.steps.length!==1)
  throw Error(which+' workflow must have exactly one task step for controlled A/B')
 const sdk=flow.steps[0].harness==='copilot-sdk'
 if(sdk!==(which==='sdk'))throw Error(which+' flow has wrong harness engine')
 return flow.steps[0]
}
const nativeStep=await definition('native')
const sdkStep=await definition('sdk')
const declaredModelMatch=Boolean(nativeStep.model&&nativeStep.model===sdkStep.model)
const expectedHash=args['oracle-sha256'].toLowerCase()
async function oracle(which){
 const root=folders[which]
 const target=path.resolve(root,args['oracle-file'])
 if(!target.startsWith(root+path.sep))return {pass:false,reason:'unsafe oracle path'}
 try{
  const actual=await realpath(target)
  if(!actual.startsWith(root+path.sep))return {pass:false,reason:'symlink escape'}
  const data=await readFile(actual)
  if(data.length>8*1024*1024)return {pass:false,reason:'oracle file too large'}
  const hash=createHash('sha256').update(data).digest('hex')
  return {pass:hash===expectedHash,actualSha256:hash,expectedSha256:expectedHash}
 }catch(e){return {pass:false,reason:'output missing or unreadable'}}
}
async function run(which){
 const requestId=randomUUID()
 const extra=which==='sdk'?{
  sdkCostApproved:Boolean(args['approve-cloud']),
  sdkToolsApproved:Boolean(args['approve-tools'])
 }:{}
 const started=performance.now()
 const deadline=Date.now()+maxMinutes*60_000
 const job=await api(which,'POST','/api/workflows/'+encodeURIComponent(flowID[which])+'/run',{
  input:args.task,requestId,...extra
 })
 let latest
 while(Date.now()<deadline){
  await delay(1_250)
  latest=await api(which,'GET','/api/jobs/'+encodeURIComponent(job.id))
  if(['completed','failed','cancelled','needs_review'].includes(latest.status))break
 }
 const status=latest?.status||'not_observed'
 const result=latest?.result||{}
 const outputs=Array.isArray(result.outputs)?result.outputs:[]
 const output=outputs.at(-1)||{}
 const usage=output.usage||{}
 const verified=await oracle(which)
 return {
  harness:which==='sdk'?'github-copilot-sdk':'dominion-rust',
  jobId:job.id,status,elapsedMs:Math.round(performance.now()-started),
  declaredModel:which==='native'?nativeStep.model:sdkStep.model,
  oracle:verified,success:status==='completed'&&verified.pass,
  tokens:{
   measured:usage.measured??null,input:usage.promptTokens??null,
   output:usage.completionTokens??null,
   total:usage.totalTokens??null,cacheRead:usage.cacheReadTokens??null,
   cacheWrite:usage.cacheWriteTokens??null
  },
  toolEvents:output.harnessProvenance?.toolEventCounts??null,
  dominionVerifiedArtifacts:Array.isArray(output.artifacts)?output.artifacts.length:0,
  actualBilledKrw:null,
  error:status==='completed'?null:String(latest?.error||status).slice(0,180)
 }
}
const ordered=(args.first||'native')==='native'?['native','sdk']:['sdk','native']
const results={}
for(const which of ordered)results[which]=await run(which)
const report={
 schema:'dominion-agent-sdk-ab/v1',
 generatedAt:new Date().toISOString(),
 task:args.task,
 declaredModelMatch,
 hardwareAndWeightParityVerified:false,
 identicalModelWeightsNotAssumed:true,
 oracle:{relativePath:args['oracle-file'],expectedSha256:expectedHash},
 native:results.native,sdk:results.sdk,
 bothPassed:results.native.success&&results.sdk.success,
 statement:'Model runs were initiated through HTTP API; no superiority claim if weights, settings, or hardware differ. SDK tool events are not independent file-operation receipts.',
}
const data=JSON.stringify(report,null,2)
if(args.output)await writeFile(args.output,data+'\n')
process.stdout.write(data+'\n')
if(!report.bothPassed)process.exitCode=1
