import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {classifyRuntimeFailure,engineForModel,memoryFit,modelPreflight,requiredMemory,selectRecommended,selectSmaller,planDownloadLaunch,preferredServerModelFormat} from '../src/utils/modelHardware.ts'

const gguf=(id,bytes)=>({id,format:'gguf',bytes})
const mlx=(id,bytes)=>({id,format:'mlx',bytes})
test('unknown memory does not produce invented hardware recommendations',()=>{
 assert.equal(memoryFit(4_000,0),'unknown')
 assert.deepEqual(selectRecommended([gguf('q4',4000)],0,0),{option:null,basis:null})
})
test('GPU and system RAM are independent budgets',()=>{
 const candidates=[gguf('q4',4_000),gguf('q8',8_000),mlx('mlx',9_000)]
 assert.deepEqual(selectRecommended(candidates,12_000,6_000).option?.id,'q4')
 assert.equal(selectRecommended([mlx('mlx',9_000)],0,15_000).option,null)
 assert.equal(selectRecommended([gguf('q8',8_000)],12_000,1_000).basis,'ram')
 assert.equal(requiredMemory(8_000),10_400)
})
test('fallback quantization is smaller and flagged when memory cannot fit it',()=>{
 const candidates=[gguf('q2',2_000),gguf('q4',4_000),gguf('q8',8_000)]
 assert.deepEqual(selectSmaller(candidates,8_000,6_000,0),{option:candidates[1],fits:true})
 assert.deepEqual(selectSmaller(candidates,8_000,1_000,0),{option:candidates[0],fits:false})
 assert.equal(selectSmaller(candidates,2_000,8_000,0).option,null)
})
test('model launch failures distinguish memory, files and engines',()=>{
 assert.equal(classifyRuntimeFailure('CUDA out of memory'),'memory')
 assert.equal(classifyRuntimeFailure('MLX 모델 파일이 완전하지 않습니다.'),'file')
 assert.equal(classifyRuntimeFailure('llama-server를 찾을 수 없습니다.'),'engine')
 assert.equal(classifyRuntimeFailure('MLX 실행기가 설치되지 않았습니다.'),'engine')
 assert.equal(classifyRuntimeFailure('unexpected exit'),'other')
})
test('model menu renders only one Hugging Face browser and unique tab id',()=>{
 const source=readFileSync(new URL('../src/pages/Models.tsx',import.meta.url),'utf8')
 assert.equal([...source.matchAll(/id:'huggingface'/g)].length,1)
 assert.equal([...source.matchAll(/<ModelExplorer[^>]*initialView="search"/g)].length,1)
 assert.match(source,/hideManagementNavigation/)
})

test('queued model launch resumes interrupted downloads instead of leaving them paused',()=>{
 assert.equal(planDownloadLaunch(undefined,false),'start')
 assert.equal(planDownloadLaunch('downloading',false),'wait')
 assert.equal(planDownloadLaunch('resolving',false),'wait')
 assert.equal(planDownloadLaunch('paused',false),'resume')
 assert.equal(planDownloadLaunch('failed',false),'resume')
 assert.equal(planDownloadLaunch('cancelled',false),'resume')
 assert.equal(planDownloadLaunch('completed',false),'start')
 assert.equal(planDownloadLaunch('completed',true),'installed')
})

test('local model format follows the actual Rust server, not the browser OS',()=>{
 const macMlx={available:true,installable:false}
 const macNeedsSetup={available:false,installable:true}
 const unavailable={available:false,installable:false}
 assert.equal(preferredServerModelFormat(undefined,undefined,macMlx,{available:false}),null)
 assert.equal(preferredServerModelFormat('win32','x86_64',macMlx,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('linux','aarch64',macMlx,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('macos','x86_64',macMlx,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('macos','aarch64',macMlx,{available:false}),'mlx')
 assert.equal(preferredServerModelFormat('macos','arm64',macNeedsSetup,{available:false}),'mlx')
 assert.equal(preferredServerModelFormat('macos','aarch64',unavailable,{available:false}),'gguf')
 assert.equal(preferredServerModelFormat('macos','aarch64',macNeedsSetup,{available:true}),'gguf')
 assert.equal(preferredServerModelFormat('macos','aarch64',macMlx,{available:true}),'mlx')
})
test('model format selection uses runtime capabilities and preserves manual choices',()=>{
 const source=readFileSync(new URL('../src/components/ModelExplorer.tsx',import.meta.url),'utf8')
 assert.doesNotMatch(source,/navigator\.platform/)
 assert.match(source,/preferredServerModelFormat\(runtime\.capabilities\?\.memory\.platform/)
 assert.match(source,/if\(preferredFormat&&!userChoseFormat\.current\)setFormat\(preferredFormat\)/)
 assert.match(source,/userChoseFormat\.current=true;setFormat\(item\.id\)/)
})

test('preflight blocks missing native GGUF engine even when LM Studio is available',()=>{
 const caps={gguf:{available:true,path:null},mlx:{available:true,standalone:false},lmstudio:{available:true},memory:{free:8_000},gpu:{free:0}}
 assert.deepEqual(modelPreflight(gguf('managed-model.gguf',4_000),caps).status,'blocked')
 assert.equal(modelPreflight({id:'lms:imported',format:'gguf',bytes:4_000},caps).status,'ready')
 assert.equal(modelPreflight({id:'mlx:owner/model',format:'mlx',bytes:4_000},caps).status,'ready')
 assert.equal(modelPreflight({id:'external:mlx',format:'mlx',bytes:4_000},caps).status,'blocked')
})
test('preflight uses separate memory budgets and warns rather than forbidding a risky launch',()=>{
 const caps={gguf:{available:true,path:'/bin/llama-server'},mlx:{available:true,standalone:true},lmstudio:{available:false},memory:{free:2_000},gpu:{free:8_000}}
 assert.equal(modelPreflight(gguf('model',4_000),caps).status,'ready')
 assert.equal(modelPreflight(mlx('model',4_000),caps).status,'warning')
 assert.equal(modelPreflight(gguf('large',10_000),caps).status,'warning')
 assert.equal(modelPreflight(gguf('model',4_000),undefined).status,'unknown')
 assert.equal(modelPreflight({id:'lms:test',format:'gguf',bytes:4_000},caps).status,'blocked')
})
test('both installed and Hugging Face launch use the same fresh preflight',()=>{
 const source=readFileSync(new URL('../src/components/ModelExplorer.tsx',import.meta.url),'utf8')
 assert.match(source,/const snapshot=await get<Runtime>\('\/api\/model-library\/runtime'\)/)
 assert.match(source,/const check=modelPreflight\(model,snapshot.capabilities\)/)
 assert.match(source,/if\(check.status==='blocked'\)/)
 assert.match(source,/selectedPreflight&&<div/)
})

test('interrupted launch remembers the exact required execution engine',()=>{
 assert.equal(engineForModel({id:'lms:model-key',format:'gguf',bytes:4_000}),'lm-studio')
 assert.equal(engineForModel({id:'mlx:owner/model',format:'mlx',bytes:4_000}),'mlx-lm')
 assert.equal(engineForModel({id:'external:123',format:'mlx',bytes:4_000}),'mlx-lm')
 assert.equal(engineForModel({id:'owner--repo/q4.gguf',format:'gguf',bytes:4_000}),'llama-cpp')
})
test('engine installation retains failed model and resumes after a fresh runtime check',()=>{
 const source=readFileSync(new URL('../src/components/ModelExplorer.tsx',import.meta.url),'utf8')
 assert.match(source,/setPendingEngineLaunch\(current=>current\?\.id===model\.id\?current:/)
 assert.match(source,/setView\('import'\)/)
 assert.match(source,/const succeeded=await load\(pending.id\)/)
 assert.match(source,/if\(succeeded\)setView\(pending.from\)/)
 assert.match(source,/pendingEngineCheck\?\.status==='blocked'/)
 assert.match(source,/pendingEngineLaunch&&view!=='import'/)
 assert.match(source,/\/api\/model-library\/runtime\/install/)
 assert.match(source,/const snapshot=await get<Runtime>\('\/api\/model-library\/runtime'\)/)
})

test('public provider model discovery does not ask for or create credentials',()=>{
 const source=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
 const browse=source.slice(source.indexOf('async function loadCatalogModels('),source.indexOf('async function ensureCatalogCredential('))
 assert.match(browse,/get<\{models:RemoteModel\[\]/)
 assert.match(browse,/\/api\/providers\/catalog\//)
 assert.doesNotMatch(browse,/apiKey|catalogKey|post\(/)
 assert.match(source,/void loadCatalogModels\(id\)/)
 assert.match(source,/setCatalogKey\(''\)/)
 const auth=source.slice(source.indexOf('async function ensureCatalogCredential('),source.indexOf('async function saveCatalogModel('))
 assert.match(auth,/!key&&!linked/)
 assert.match(source,/조회는 무료 공개 메타데이터/)
})
test('OpenRouter free model lookup remains separate from authenticated registration',()=>{
 const source=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
 const browse=source.slice(source.indexOf('async function browseOpenRouterFree('),source.indexOf('async function ensureOpenRouterCredential('))
 assert.match(browse,/getOpenRouterFreeModels\(\)/)
 assert.doesNotMatch(browse,/routerKey\.trim\(\)/)
 assert.match(source,/\/api\/providers\/openrouter\/free-models/)
 assert.match(source,/무료 모델 조회 \(키 불필요\)/)
})

test('public catalog opens the provider model list without entering an API key',()=>{
 const models=readFileSync(new URL('../src/pages/Models.tsx',import.meta.url),'utf8')
 const account=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
 assert.match(models,/setBrowseProviderId\(p\.id\);setTab\('connect'\)/)
 assert.match(models,/initialProviderId=\{tab==='connect'\?browseProviderId:undefined\}/)
 assert.match(account,/initialCatalogApplied\.current=initialProviderId/)
 assert.match(account,/void loadCatalogModels\(initialProviderId\)/)
 assert.match(account,/setProviderPage\(Math\.floor\(index\/40\)\)/)
})
test('authenticated model registration can open the exact newly registered provider in chat',()=>{
 const account=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
 assert.match(account,/provider:registered\.providerId,model:registered\.model/)
 assert.match(account,/setRegistered\(\{providerId:saved\.id,model:catalogModel,source:'catalog'\}\)/)
 assert.match(account,/setRegistered\(\{providerId:duplicate\.id,model:catalogModel,source:'catalog'\}\)/)
 assert.match(account,/등록한 모델로 채팅하기/)
})
test('OpenRouter free model single registration reuses existing credentials without affecting browsing',()=>{
 const account=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
 const browse=account.slice(account.indexOf('async function browseOpenRouterFree('),account.indexOf('async function ensureOpenRouterCredential('))
 assert.doesNotMatch(browse,/routerKey\.trim\(\)/)
 assert.match(account,/async function saveOpenRouterModel\(\)/)
 assert.match(account,/const selectedModel=routerModels\.find\(model=>model\.id===routerSelected\)/)
 assert.match(account,/credentialSourceId:key\?undefined:linked\?\.id/)
 assert.match(account,/선택 모델 등록/)
 assert.match(account,/onClick=\{goToRegisteredChat\}/)
})

test('a public models.dev document is shared by provider and model browsing',()=>{
 const api=readFileSync(new URL('../../src/api/providers.rs',import.meta.url),'utf8')
 const fetches=[...api.matchAll(/get\("https:\/\/models\.dev\/api\.json"\)/g)]
 assert.equal(fetches.length,1)
 assert.match(api,/static MODELS_DEV_DOCUMENT:OnceLock/)
 assert.match(api,/fetched\.elapsed\(\)<Duration::from_secs\(86_400\)/)
 assert.match(api,/static MODELS_DEV_FETCH:tokio::sync::Mutex/)
 assert.match(api,/let _fetch_guard=MODELS_DEV_FETCH\.lock\(\)\.await/)
 assert.equal([...api.matchAll(/if let Some\(document\)=cached_models_dev_document\(\)\{return Ok\(document\);\}/g)].length,2)
 const models=api.slice(api.indexOf('pub async fn public_catalog_models('),api.indexOf('/// OpenRouter publishes'))
 assert.match(models,/load_models_dev_document\(\)\.await/)
 const providers=api.slice(api.indexOf('pub async fn provider_catalog('),api.indexOf('/// Refreshes every enabled provider'))
 assert.match(providers,/load_models_dev_document\(\)\.await/)
})
test('catalog registration selects the Anthropic protocol and chat route retains its model',()=>{
 const account=readFileSync(new URL('../src/components/DominionCloudAccount.tsx',import.meta.url),'utf8')
 const app=readFileSync(new URL('../src/App.tsx',import.meta.url),'utf8')
 assert.match(account,/catalogProviderKind=\(provider:CatalogProvider\)/)
 assert.match(account,/kind:catalogProviderKind\(entry\)/)
 assert.match(app,/provider:params\.get\('provider'\)\|\|undefined/)
 assert.match(app,/model:params\.get\('model'\)\|\|undefined/)
})
