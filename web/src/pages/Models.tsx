import {useEffect,useMemo,useRef,useState} from 'react'
import {ArrowRight,Check,Cloud,HardDrive,KeyRound,LogIn,Plus,RefreshCw,Search,Settings2,ShieldCheck,Sparkles,UserRound,X} from 'lucide-react'
import {Provider,del,get,post} from '../api'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {ModelExplorer} from '../components/ModelExplorer'
import {ModelPicker} from '../components/ModelPicker'
import {OAuthConnectionCard} from '../components/OAuthConnectionCard'
import {CliAuthProgress, type CliAuthStatus} from '../components/CliAuthProgress'
import {ReasoningSelect} from '../components/ReasoningSelect'
import {useProviderModels} from '../hooks/useProviderModels'
import {t,useLocale} from '../i18n'

type PresetCategory='free'|'cloud'|'local'
type ProviderPreset={name:string;baseUrl:string;kind:string;local:boolean;integration?:'freellmapi'|'opencode-go';free?:boolean;requestsPerMinute?:number;category:PresetCategory;auth?:'key'|'none';note?:string}
type CliCredential={id:string;name:string;available:boolean;supported:boolean;connected?:boolean;importAvailable?:boolean;accountId?:string;model?:string;hint?:string;loginSupported?:boolean;loginMethod?:'browser'|'browser-cli'|'api-key'}
type CatalogProvider={id:string;name:string;api:string;models:number;env?:string[];doc?:string}
type CatalogResponse={providers:CatalogProvider[];cached?:boolean;error?:string}
type FreeCatalogModel={id:string;free?:boolean|null;freeStatus?:string;freeSource?:string}
type FreeCatalogProvider={providerId:string;providerName:string;reachable:boolean;error?:string;models:FreeCatalogModel[]}
type FreeCatalog={status?:string;updatedAt?:string;providers:FreeCatalogProvider[];counts?:{free:number;paid:number;unknown:number}}
type Tab='providers'|'catalog'|'local'|'oauth'

const opencodeGoPreset:ProviderPreset={name:'OpenCode Go',baseUrl:'https://opencode.ai/inference/go/openai/v1',kind:'openai',local:false,category:'cloud',integration:'opencode-go'}

const providerPresets:ProviderPreset[]=[
 // Free tiers and local gateways. Keys are still required where the provider asks for one.
 {name:'FreeLLMAPI',baseUrl:'http://127.0.0.1:31415/v1',kind:'openai',local:false,integration:'freellmapi',free:true,category:'free',auth:'none'},
 {name:'OVHcloud (무료)',baseUrl:'https://oai.endpoints.kepler.ai.cloud.ovh.net/v1',kind:'openai',local:false,free:true,requestsPerMinute:2,category:'free'},
 {name:'Groq',baseUrl:'https://api.groq.com/openai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'z.ai',baseUrl:'https://api.z.ai/api/paas/v4',kind:'openai',local:false,free:true,category:'free'},
 {name:'Cloudflare Workers AI',baseUrl:'https://api.cloudflare.com/client/v4/accounts/ACCOUNT_ID/ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'OpenRouter',baseUrl:'https://openrouter.ai/api/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Vercel AI Gateway',baseUrl:'https://ai-gateway.vercel.sh/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Chutes',baseUrl:'https://llm.chutes.ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Cerebras',baseUrl:'https://api.cerebras.ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'SambaNova',baseUrl:'https://api.sambanova.ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Hyperbolic',baseUrl:'https://api.hyperbolic.xyz/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Nebius',baseUrl:'https://api.studio.nebius.ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Novita',baseUrl:'https://api.novita.ai/v3/openai',kind:'openai',local:false,free:true,category:'free'},
 // Metered cloud APIs.
 {name:'Google Gemini',baseUrl:'https://generativelanguage.googleapis.com/v1beta/openai',kind:'openai',local:false,category:'cloud'},
 {name:'Anthropic',baseUrl:'https://api.anthropic.com/v1',kind:'anthropic',local:false,category:'cloud'},
 {name:'OpenAI',baseUrl:'https://api.openai.com/v1',kind:'openai',local:false,category:'cloud'},
 {name:'DeepSeek',baseUrl:'https://api.deepseek.com/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Mistral',baseUrl:'https://api.mistral.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'xAI',baseUrl:'https://api.x.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Together',baseUrl:'https://api.together.xyz/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Fireworks',baseUrl:'https://api.fireworks.ai/inference/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Perplexity',baseUrl:'https://api.perplexity.ai',kind:'openai',local:false,category:'cloud'},
 {name:'GitHub Models',baseUrl:'https://models.github.ai/inference',kind:'openai',local:false,category:'cloud'},
 {name:'Requesty',baseUrl:'https://router.requesty.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Reflection',baseUrl:'https://api.reflection.ai/openai/v1',kind:'openai',local:false,category:'cloud'},
 // Local runtimes and localhost proxies.
 {name:'Ollama',baseUrl:'http://127.0.0.1:11434/v1',kind:'ollama',local:true,category:'local',auth:'none'},
 {name:'LM Studio',baseUrl:'http://127.0.0.1:1234/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'llama.cpp',baseUrl:'http://127.0.0.1:8080/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'vLLM',baseUrl:'http://127.0.0.1:8000/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'MLX',baseUrl:'http://127.0.0.1:8081/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'LiteLLM',baseUrl:'http://127.0.0.1:4000/v1',kind:'openai',local:true,category:'local',auth:'none'},
 opencodeGoPreset
]

const sameProvider=(p:Provider,preset:ProviderPreset)=>p.name===preset.name||p.baseUrl.replace(/\/$/,'')===preset.baseUrl.replace(/\/$/,'')

export function ModelHub(){
 useLocale()
 const providers=useRemote<Provider[]>('/api/providers',[])
 const cli=useRemote<CliCredential[]>('/api/providers/cli',[])
 const [counts,setCounts]=useState<Record<string,{list:string[];error?:string;busy?:boolean}>>({})
 const [error,setError]=useState(''),[saving,setSaving]=useState(false)
 const [savedId,setSavedId]=useState(''),[removedName,setRemovedName]=useState('')
 const [integration,setIntegration]=useState<ProviderPreset['integration']>()
 const [freePreset,setFreePreset]=useState(false)
 const providerList=useRef<HTMLDetailsElement>(null)
 const [modal,setModal]=useState(false),[presetMode,setPresetMode]=useState(false)
 const [edit,setEdit]=useState<Provider|null>(null)
 const [tab,setTab]=useState<Tab>('providers'),[category,setCategory]=useState<'all'|PresetCategory>('all'),[query,setQuery]=useState('')
 const catalog=useRemote<CatalogResponse>(tab==='catalog'?'/api/providers/catalog':null,{providers:[]})
 const [loginPending,setLoginPending]=useState(false),[loginUrl,setLoginUrl]=useState(''),[loginMessage,setLoginMessage]=useState(''),[deviceCode,setDeviceCode]=useState<{userCode:string;verificationUrl:string}|null>(null)
 const freeCatalog=useRemote<FreeCatalog>('/api/providers/free-catalog',{providers:[]})
 const [freeBusy,setFreeBusy]=useState(false)
 const [oauthBusy,setOauthBusy]=useState('')
 const [cliAuth,setCliAuth]=useState<CliAuthStatus|null>(null)
 const cliLoginPopup=useRef<Window|null>(null)
 const cliLoginPopupUrl=useRef('')
 const [openCodeAuthProvider,setOpenCodeAuthProvider]=useState('openai')
 const [cliModelCatalog,setCliModelCatalog]=useState<Record<string,string[]>>({})
 const [cliModelSelection,setCliModelSelection]=useState<Record<string,string>>({})
 const [cliModelBusy,setCliModelBusy]=useState('')
 const cliModelRequested=useRef(new Set<string>())
 const fetchCliModels=async(source:string)=>{
  const providerId='cli-'+source
  setCliModelBusy(source)
  try{
   const rows=await get<{id:string}[]>('/api/providers/'+providerId+'/models')
   const available=rows.map(r=>r.id).filter(Boolean)
   const defaultChoice=source==='gemini'?'auto':'default'
   const options=[defaultChoice,...available.filter(model=>model!==defaultChoice)]
   setCliModelCatalog(old=>({...old,[source]:options}))
   setCliModelSelection(old=>({...old,[source]:old[source]&&options.includes(old[source])?old[source]:defaultChoice}))
  }catch(e){setError((e as Error).message)}
  finally{setCliModelBusy('')}
 }
 useEffect(()=>{
  if(tab!=='oauth')return
  for(const item of cli.value){
   if(!item.connected||!['claude','gemini','opencode'].includes(item.id)||cliModelRequested.current.has(item.id))continue
   cliModelRequested.current.add(item.id)
   void fetchCliModels(item.id)
  }
 },[tab,cli.value])
 const saveCliModel=async(source:string)=>{
  const model=cliModelSelection[source]
  if(!model)return
  setCliModelBusy(source)
  try{
   await post('/api/providers/cli/'+source+'/model',{model})
   await Promise.all([providers.reload(),cli.reload()])
   setLoginMessage('사용할 모델을 저장했습니다. 이제 채팅에서 바로 선택할 수 있습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setCliModelBusy('')}
 }
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false),[modelId,setModelId]=useState(''),[defaultModel,setDefaultModel]=useState(''),[reflection,setReflection]=useState(false),[rpm,setRpm]=useState(''),[accountId,setAccountId]=useState(''),[testBusy,setTestBusy]=useState(false),[testResult,setTestResult]=useState<{ok:boolean;message:string}|null>(null)
 const [defaultEffort,setDefaultEffort]=useState('')
 const [costInput,setCostInput]=useState(''),[costOutput,setCostOutput]=useState('')
 const [qualityRating,setQualityRating]=useState('')
 const editModels=useProviderModels(modal&&edit?edit.id:'')
 const probe=async(p:Provider)=>{
  setCounts(old=>({...old,[p.id]:{list:[],busy:true}}))
  try{const list=await get<{id:string;catalogStale?:boolean}[]>('/api/providers/'+p.id+'/models');setCounts(old=>({...old,[p.id]:{list:list.map(m=>m.id),error:list.some(m=>m.catalogStale)?'최근 저장된 목록입니다. 실시간 Codex 조회가 실패했습니다.':undefined}}))}
  catch(e){setCounts(old=>({...old,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const open=(p?:Provider)=>{setEdit(p||null);setIntegration(p?.integration==='freellmapi'?'freellmapi':p?.integration==='opencode-go'?'opencode-go':undefined);setFreePreset(false);setPresetMode(false);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local));setModelId(p?.reflectionModel||(p?.reflection??p?.learning?p?.model:'')||'');setDefaultModel(p?.model||'');setDefaultEffort(String(p?.reasoningEffort||''));setReflection(Boolean(p?.reflection ?? p?.learning));setRpm(p?.requestsPerMinute?String(p.requestsPerMinute):'');setCostInput(typeof p?.costPerMillionInputKrw==='number'?String(p.costPerMillionInputKrw):'');setCostOutput(typeof p?.costPerMillionOutputKrw==='number'?String(p.costPerMillionOutputKrw):'');setQualityRating(typeof p?.capabilities?.quality==='number'?String(Math.round(p.capabilities.quality*100)):'');setAccountId(p?.accountId||'');setTestResult(null);setError('');setModal(true)}
 const openPreset=(preset:ProviderPreset)=>{setEdit(null);setIntegration(preset.integration);setFreePreset(Boolean(preset.free&&preset.auth==='none'));setPresetMode(true);setName(preset.name);setBaseUrl(preset.baseUrl);setKind(preset.kind);setLocal(preset.local);setApiKey('');setModelId('');setDefaultModel('');setDefaultEffort('');setReflection(false);setRpm(preset.requestsPerMinute?String(preset.requestsPerMinute):'');setCostInput('');setCostOutput('');setQualityRating('');setAccountId('');setTestResult(null);setError('');setModal(true)}
 const goProvider=providers.value.find(p=>p.integration==='opencode-go'||p.baseUrl.includes('opencode.ai/zen/go')||p.baseUrl.includes('opencode.ai/inference/go'))
 const connectOpencodeGo=()=>{if(goProvider)open(goProvider);else openPreset(opencodeGoPreset)}
 const needsNewKey=presetMode&&!freePreset&&(!local||integration==='freellmapi')
 const testConnection=async()=>{
  if(testBusy||!baseUrl.trim())return
  setTestBusy(true);setTestResult(null)
  try{
   const result=await post<{ok:boolean;message?:string;status?:number}>(`/api/providers/${edit?.id||'new'}/test`,{
    baseUrl:baseUrl.trim(),kind,integration,accountId:accountId.trim()||undefined,apiKey:apiKey.trim()||undefined,model:defaultModel.trim()||edit?.model,
   })
   setTestResult(result.ok?{ok:true,message:t('models.testOk')}:{ok:false,message:(result.message||t('models.testFail'))+(result.status?` (HTTP ${result.status})`:'')})
  }catch(e){setTestResult({ok:false,message:(e as Error).message})}
  finally{setTestBusy(false)}
 }
 const close=()=>{if(!saving)setModal(false)}
 const save=async()=>{
  if(saving||!baseUrl.trim()||(needsNewKey&&!apiKey.trim()))return
  if(reflection&&!modelId.trim()){setError(t('models.dlg.needReflectionModel'));return}
  if(qualityRating.trim()!==''&&(!Number.isFinite(Number(qualityRating))||Number(qualityRating)<0||Number(qualityRating)>100)){
   setError('자동 라우팅 품질 점수는 0~100점입니다.');return
  }
  if([costInput,costOutput].some(value=>value.trim()!==''&&(!Number.isFinite(Number(value))||Number(value)<0||Number(value)>100_000_000))){
   setError('토큰 가격은 0~1억원 범위의 숫자로 입력하세요.');return
  }
  setSaving(true);setError('')
  try{
   const capabilities={...(edit?.capabilities||{})}
   if(qualityRating.trim()!=='')capabilities.quality=Number(qualityRating)/100
   else delete capabilities.quality
   const saved=await post<Provider>('/api/providers',{
    ...edit,name:name.trim(),baseUrl:baseUrl.trim(),kind,apiKey:apiKey.trim()||undefined,local,integration,accountId:accountId.trim()||undefined,requestsPerMinute:Number(rpm)||0,
    toolMode:edit?.kind===kind?edit.toolMode:undefined,
    reasoningEffort:defaultEffort,enabled:edit?.enabled??true,model:defaultModel.trim()||edit?.model,reflection,reflectionModel:reflection?modelId.trim():undefined,
    capabilities,
    costPerMillionInputKrw:costInput.trim()===''?null:Number(costInput),
    costPerMillionOutputKrw:costOutput.trim()===''?null:Number(costOutput),
   })
   providers.setValue(rows=>rows.some(p=>p.id===saved.id)?rows.map(p=>p.id===saved.id?saved:p):[...rows,saved])
   setCounts(rows=>{const next={...rows};delete next[saved.id];return next})
   setSavedId(saved.id);setRemovedName('');setApiKey('');setModal(false);refresh()
   requestAnimationFrame(()=>{if(providerList.current)providerList.current.open=true})
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 const disconnect=async()=>{
  if(!edit||saving||edit.id==='dominion-native')return
  if(!window.confirm(t('models.dlg.disconnectConfirm',{name:edit.name})))return
  setSaving(true);setError('')
  try{
   await del('/api/providers/'+encodeURIComponent(edit.id))
   providers.setValue(rows=>rows.filter(provider=>provider.id!==edit.id))
   setCounts(rows=>{const next={...rows};delete next[edit.id];return next})
   setSavedId('');setRemovedName(edit.name);setApiKey('');setModal(false);refresh()
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 const startCodexDevice=async()=>{
  if(oauthBusy)return
  setOauthBusy('codex-device');setError('');setLoginMessage('');setLoginUrl('');setDeviceCode(null)
  try{
   const started=await post<{user_code:string;verification_url:string}>('/api/providers/codex/device',{})
   setDeviceCode({userCode:started.user_code,verificationUrl:started.verification_url})
   // The code and verification link are displayed below; the user opens it directly.
   setLoginPending(true)
  }catch(e){setOauthBusy('');setError((e as Error).message)}
 }
 const startCodexLogin=async()=>{
  if(oauthBusy)return
  // A blank tab must be opened synchronously from the click; after an
  // awaited API request browsers often block the OAuth login popup.
  const loginWindow=window.open('about:blank','_blank')
  setOauthBusy('codex-login');setError('');setLoginMessage('');setLoginUrl('');setDeviceCode(null)
  try{
   const started=await get<{url:string}>('/api/providers/codex/login')
   setLoginUrl(started.url)
   if(loginWindow)loginWindow.location.href=started.url
   setLoginPending(true)
   if(!loginWindow)setLoginMessage('팝업이 차단되었습니다. 아래 로그인 링크를 직접 누르세요.')
  }catch(e){loginWindow?.close();setOauthBusy('');setError((e as Error).message)}
 }
 useEffect(()=>{
  if(tab!=='oauth')return
  let alive=true
  get<{status:string;userCode?:string;verificationUrl?:string}>('/api/providers/codex/login/status').then(status=>{
   if(!alive||status.status!=='device_pending'||!status.userCode||!status.verificationUrl)return
   setDeviceCode({userCode:status.userCode,verificationUrl:status.verificationUrl})
   setLoginPending(true);setOauthBusy('codex-device')
  }).catch(()=>{})
  return()=>{alive=false}
 },[tab])
 useEffect(()=>{
  if(!loginPending)return
  let alive=true
  const timer=setInterval(async()=>{
   try{
    const status=await get<{status:string;message?:string}>('/api/providers/codex/login/status')
    if(!alive)return
    if(status.status==='success'){
     setLoginPending(false);setOauthBusy('');setDeviceCode(null);setLoginMessage(status.message||t('models.oauth.loginSuccess'))
     await Promise.all([providers.reload(),cli.reload()]);setSavedId('codex-oauth');refresh()
    }else if(status.status==='error'){
     setLoginPending(false);setOauthBusy('');setDeviceCode(null);setError(status.message||t('models.oauth.loginFailed'))
    }
   }catch{}
  },1500)
  return()=>{alive=false;clearInterval(timer)}
 },[loginPending])
 const startCliLogin=async(source:string)=>{
  if(oauthBusy)return
  // Open synchronously from the user click, as with Codex OAuth. Creating the
  // tab after the asynchronous Rust request would trigger popup blocking.
  const popup=window.open('about:blank','_blank')
  if(popup){
   popup.document.title='DOMINION · 로그인 연결 중'
   popup.document.body.textContent='공식 로그인 페이지를 준비하고 있습니다…'
  }
  cliLoginPopup.current=popup
  cliLoginPopupUrl.current=''
  setOauthBusy('login-'+source);setError('');setLoginMessage('')
  setCliAuth({source,status:'starting',message:'공식 로그인 창을 준비 중입니다.'})
  try{
   const started=await post<CliAuthStatus>('/api/providers/cli/'+encodeURIComponent(source)+'/login',{provider:source==='opencode'?openCodeAuthProvider:undefined})
   setCliAuth(started)
  }catch(e){popup?.close();cliLoginPopup.current=null;setCliAuth(null);setError((e as Error).message)}
  finally{setOauthBusy('')}
 }
 useEffect(()=>{
  if(!cliAuth)return
  if(cliAuth.url && cliLoginPopupUrl.current!==cliAuth.url){
   const popup=cliLoginPopup.current
   if(popup && !popup.closed){
    popup.location.href=cliAuth.url
    cliLoginPopupUrl.current=cliAuth.url
    cliLoginPopup.current=null
   }
  }
  if(['completed','failed','cancelled'].includes(cliAuth.status)){
   cliLoginPopup.current?.close()
   cliLoginPopup.current=null
  }
 },[cliAuth?.url,cliAuth?.status])
 useEffect(()=>{
  if(!cliAuth||['completed','failed','cancelled'].includes(cliAuth.status))return
  let live=true
  const source=cliAuth.source
  const timer=setInterval(()=>{
   void get<CliAuthStatus>('/api/providers/cli/'+encodeURIComponent(source)+'/login/status').then(next=>{
    if(!live)return
    setCliAuth(current=>current?.source===source?next:current)
    if(['completed','failed','cancelled'].includes(next.status)){
     void cli.reload().then(()=>providers.reload()).then(()=>{
      if(next.status==='completed')void fetchCliModels(source)
     })
     if(next.status==='completed')setLoginMessage('로그인 완료. 모델 목록을 불러옵니다.')
    }
   }).catch(e=>{if(live)setError((e as Error).message)})
  },1200)
  return()=>{live=false;clearInterval(timer)}
 },[cliAuth?.source,cliAuth?.status])
 const sendCliInput=async(input:string)=>{
  if(!cliAuth)return
  try{await post('/api/providers/cli/'+encodeURIComponent(cliAuth.source)+'/login/input',{input})}
  catch(e){setError((e as Error).message)}
 }
 const cancelCliAuth=async()=>{
  if(!cliAuth)return
  try{
   await post('/api/providers/cli/'+encodeURIComponent(cliAuth.source)+'/login/cancel',{})
   setCliAuth({...cliAuth,status:'cancelled',message:'로그인을 취소했습니다.'})
  }catch(e){setError((e as Error).message)}
 }
 const refreshFreeCatalog=async()=>{
  if(freeBusy)return
  setFreeBusy(true);setError('')
  try{const next=await post<FreeCatalog>('/api/providers/free-catalog/refresh',{});freeCatalog.setValue(next)}
  catch(e){setError((e as Error).message)}
  finally{setFreeBusy(false)}
 }
 const importCli=async(source:string)=>{
  if(oauthBusy)return
  setOauthBusy(source);setError('')
  try{
   const saved=await post<Provider&{importedCount?:number;skipped?:string[]}>('/api/providers/cli/import',{source})
   setSavedId(saved.id);await Promise.all([providers.reload(),cli.reload()]);refresh()
   if(source==='opencode-go')setLoginMessage('OpenCode Go 키를 DOMINION에 저장했습니다. 모델을 선택하고 연결을 검사하세요.')
   else if(saved.importedCount&&saved.importedCount>1)setLoginMessage(`API 키 ${saved.importedCount}개를 가져왔습니다.`)
  }catch(e){setError((e as Error).message)}
  finally{setOauthBusy('')}
 }
 const visiblePresets=useMemo(()=>providerPresets.filter(preset=>{
  if(tab==='local'&&preset.category!=='local')return false
  if(tab==='providers'&&preset.category==='local')return false
  if(tab==='catalog'||tab==='oauth')return false
  if(tab==='providers'&&category!=='all'&&preset.category!==category)return false
  const term=query.trim().toLowerCase()
  return !term||(preset.name+' '+preset.baseUrl).toLowerCase().includes(term)
 }),[tab,category,query])
 const visibleProviders=useMemo(()=>providers.value.filter(p=>{
  const term=query.trim().toLowerCase()
  return !term||(p.name+' '+p.baseUrl).toLowerCase().includes(term)
 }),[providers.value,query])
 const tabs:Array<{id:Tab;label:string;count?:number}>= [
  {id:'providers',label:t('models.tab.providers'),count:providers.value.length},
  {id:'catalog',label:t('models.tab.catalog')},
  {id:'local',label:t('models.tab.local'),count:providerPresets.filter(p=>p.category==='local').length},
  {id:'oauth',label:t('models.tab.oauth')},
 ]
 return <div className="mx-auto w-full max-w-[1200px] px-4 py-5 sm:px-6 lg:py-6">
  <header className="mb-4 flex flex-wrap items-end justify-between gap-3">
   <div><h1 className="m-0 text-[19px] font-bold tracking-tight text-[#24211D]">{t('models.page.title')}</h1><p className="mt-1 text-[12px] text-[#7C7469]">{t('models.page.desc')}</p></div>
   <Button type="button" onClick={()=>open()} className="inline-flex h-8 items-center gap-2 rounded-lg border border-[#E1D9CC] bg-white px-3 text-[11px] font-semibold text-[#3A342D] hover:bg-[#FAF7F1]"><Plus size={14}/> {t('models.apiAdd')}</Button>
  </header>
  <div className="mb-4 flex flex-wrap items-center gap-2">
   <div className="flex flex-wrap gap-1 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-1">
    {tabs.map(item=><Button variant="bare" key={item.id} type="button" onClick={()=>{setTab(item.id);setCategory('all')}} className={'inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] font-semibold transition-colors '+(tab===item.id?'bg-[#C96442] text-white':'text-[#6F675D] hover:bg-[#F3EEE5]')}>{item.label}{item.count!==undefined&&<span className={'rounded px-1.5 py-0.5 text-[10px] '+(tab===item.id?'bg-white/20':'bg-[#F0EAE0] text-[#8A8176]')}>{item.count}</span>}</Button>)}
   </div>
   {(tab==='providers'||tab==='local')&&<><div className="relative min-w-[180px] flex-1 sm:max-w-[280px]"><Search size={14} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[#A79C8E]"/><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder={t('models.providerSearch')} className="!pl-8 !text-[12px]"/></div>
    {tab==='providers'&&<div className="flex flex-wrap gap-1">{(['all','free','cloud'] as const).map(value=><Button variant="bare" key={value} type="button" onClick={()=>setCategory(value)} className={'rounded-lg border px-2.5 py-1.5 text-[11px] font-semibold '+(category===value?'border-[#C96442] bg-[#FBF1EC] text-[#A94E30]':'border-[#E7E0D5] bg-white text-[#7C7469]')}>{t(value==='all'?'models.filter.all':value==='free'?'models.filter.free':'models.filter.cloud')}</Button>)}</div>}</>}
  </div>

  <section className="mb-4 flex flex-wrap items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2 text-[11px] text-[#7C7469]" title={t('models.freeCheck.note')}>
   <strong className="text-[#3A342D]">{t('models.freeCheck.title')}</strong>
   <span className="rounded bg-emerald-50 px-1.5 py-0.5 font-semibold text-emerald-700">{t('models.freeCheck.free',{count:freeCatalog.value.counts?.free||0})}</span>
   <span className="rounded bg-[#F1ECE3] px-1.5 py-0.5 font-semibold text-[#8A8176]">{t('models.freeCheck.unknown',{count:freeCatalog.value.counts?.unknown||0})}</span>
   <span className="rounded bg-[#F7EEE9] px-1.5 py-0.5 font-semibold text-[#A05B48]">{t('models.freeCheck.paid',{count:freeCatalog.value.counts?.paid||0})}</span>
   {freeCatalog.value.providers.filter(p=>!p.reachable).length>0&&<span className="font-semibold text-rose-600">{t('models.freeCheck.failed',{count:freeCatalog.value.providers.filter(p=>!p.reachable).length})}</span>}
   <span className="ml-auto text-[10px] text-[#9C9388]">{freeCatalog.value.updatedAt?t('models.freeCheck.updated',{time:new Date(freeCatalog.value.updatedAt).toLocaleString()}):t('models.freeCheck.empty')}</span>
   <Button type="button" variant="ghost" size="sm" disabled={freeBusy} onClick={()=>void refreshFreeCatalog()}>{freeBusy?<RefreshCw size={13} className="animate-spin"/>:<RefreshCw size={13}/>} {freeBusy?t('models.freeCheck.checking'):t('models.freeCheck.refresh')}</Button>
  </section>
  {tab==='catalog'&&<section className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-3 flex flex-wrap items-start justify-between gap-2"><div><h2 className="m-0 text-[16px] font-bold text-[#24211D]">{t('models.catalog.title')}</h2><p className="mt-1 text-[12px] text-[#7C7469]">{t('models.catalog.desc')}</p></div><Button type="button" variant="ghost" size="sm" disabled={catalog.loading} onClick={()=>void catalog.reload()}>{catalog.loading?<RefreshCw size={13} className="animate-spin"/>:<RefreshCw size={13}/>} {t('models.catalog.refresh')}</Button></div>
   <div className="mb-2"><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder={t('models.providerSearch')} className="!text-[12px]"/></div>
   {catalog.error&&<Notice message={catalog.error}/>}
   {catalog.loading&&<p className="py-4 text-center text-[11px] text-[#9C9388]">{t('models.catalog.loading')}</p>}
   <div className="grid max-h-[420px] grid-cols-1 gap-2 overflow-auto sm:grid-cols-2 xl:grid-cols-3">{catalog.value.providers.filter(p=>{const term=query.trim().toLowerCase();return !term||(p.name+' '+p.api).toLowerCase().includes(term)}).slice(0,60).map(p=><div key={p.id} className="flex min-w-0 items-center justify-between gap-2 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] px-3 py-2.5"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{p.name}</strong><small className="block truncate text-[10px] text-[#A79C8E]">{p.api.replace(/^https?:\/\//,'')} · {t('models.catalog.modelCount',{count:p.models})}</small></div><Button type="button" size="sm" variant="secondary" onClick={()=>openPreset({name:p.name,baseUrl:p.api,kind:p.id.includes('anthropic')||p.api.includes('anthropic')?'anthropic':p.id.includes('ollama')?'ollama':'openai',local:false,category:'cloud'})}>{t('models.register')}</Button></div>)}</div>
  </section>}

  {tab==='oauth'&&<section className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-4"><h2 className="m-0 text-[16px] font-bold text-[#24211D]">{t('models.oauth.title')}</h2><p className="mt-1 text-[12px] text-[#7C7469]">{t('models.oauth.desc')}</p></div>
   <div className="grid grid-cols-1 items-stretch gap-3 md:grid-cols-2">
    {cli.value.map(item=><OAuthConnectionCard
     key={item.id}
     title={item.name}
     status={item.connected?'DOMINION 연결됨':item.available?'CLI 로그인 확인':'로그인 필요'}
     statusTone={item.connected?'connected':item.available?'available':'disconnected'}
     accountId={item.accountId}
     model={item.model}
     hint={item.hint}
    >
     {item.id==='codex'&&<>
      <Button type="button" variant="primary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void startCodexLogin()}><LogIn size={13}/> {item.connected?'재로그인':'브라우저 로그인'}</Button>
      <Button type="button" variant="secondary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void startCodexDevice()}><KeyRound size={13}/> 기기 코드 로그인</Button>
     </>}
     {item.id==='opencode'&&<SelectInput aria-label="OpenCode 로그인 제공자" className="!w-auto !min-w-[128px] !py-1 !text-[12px]" value={openCodeAuthProvider} onChange={event=>setOpenCodeAuthProvider(event.target.value)}>
      <option value="openai">OpenAI</option><option value="anthropic">Anthropic</option><option value="google">Google</option><option value="github-copilot">GitHub Copilot</option>
     </SelectInput>}
     {item.loginMethod==='browser-cli'&&<Button type="button" variant="primary" size="sm" disabled={Boolean(oauthBusy)||Boolean(cliAuth&&!['completed','failed','cancelled'].includes(cliAuth.status))} onClick={()=>void startCliLogin(item.id)}>{oauthBusy==='login-'+item.id?<RefreshCw size={13} className="animate-spin"/>:<LogIn size={13}/>} {item.available?'브라우저 재로그인':'브라우저 로그인'}</Button>}
     {item.id==='opencode-go'&&<Button type="button" variant="primary" size="sm" onClick={connectOpencodeGo}><KeyRound size={13}/> {goProvider?'키 / 모델 설정':'API 키 연결'}</Button>}
     {item.supported&&(item.importAvailable??item.available)&&<Button type="button" variant="secondary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void importCli(item.id)}><KeyRound size={13}/> {item.id==='opencode-go'?'기존 키 가져오기':'기존 인증 가져오기'}</Button>}
     {(item.id==='claude'||item.id==='gemini')&&<Button type="button" variant="secondary" size="sm" onClick={()=>openPreset(providerPresets.find(p=>p.name===(item.id==='claude'?'Anthropic':'Google Gemini'))!)}><KeyRound size={13}/> 별도 API 연결(선택)</Button>}
     {item.connected&&['claude','gemini','opencode'].includes(item.id)&&<div className="flex w-full flex-wrap items-center gap-2 rounded-lg border border-[#E7E0D5] bg-white p-2">
      <SelectInput aria-label={item.name+' 사용 모델 선택'} className="min-w-[145px] flex-1 !py-1 !text-[12px]"
       value={cliModelSelection[item.id]||item.model||'default'}
       onChange={event=>setCliModelSelection(old=>({...old,[item.id]:event.target.value}))}>
       {(!cliModelCatalog[item.id]?.length)&&<option value={item.model||'default'}>{item.model||'CLI 기본 모델'}</option>}
       {(cliModelCatalog[item.id]||[]).map(model=><option key={model} value={model}>{model}</option>)}
      </SelectInput>
      <Button size="sm" type="button" variant="ghost" disabled={Boolean(cliModelBusy)} onClick={()=>void fetchCliModels(item.id)}><RefreshCw size={13}/> 모델 조회</Button>
      <Button size="sm" type="button" variant="secondary" disabled={Boolean(cliModelBusy)} onClick={()=>void saveCliModel(item.id)}>모델 적용</Button>
      <a className="rounded-lg bg-[#C96442] px-3 py-2 text-[12px] font-semibold text-white" href={'#/chat?provider=cli-'+item.id+'&model='+encodeURIComponent(cliModelSelection[item.id]||item.model||'default')}>채팅 시작</a>
     </div>}
     {item.id==='codex'&&item.connected&&<Button type="button" variant="ghost" size="sm" onClick={()=>{setTab('providers');const p=providers.value.find(p=>p.id==='codex-oauth');if(p)void probe(p)}}><RefreshCw size={13}/> 모델 확인</Button>}
     {item.id==='opencode-go'&&goProvider?.hasKey&&<Button type="button" variant="ghost" size="sm" disabled={counts[goProvider.id]?.busy} onClick={()=>void probe(goProvider)}><RefreshCw size={13}/> 모델 확인</Button>}
     <Button type="button" variant="ghost" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void cli.reload()}><RefreshCw size={13}/> 상태 새로고침</Button>
    </OAuthConnectionCard>)}
   </div>
   {cliAuth&&<CliAuthProgress session={cliAuth} onInput={sendCliInput} onCancel={cancelCliAuth}/>}
   {deviceCode&&<div className="mt-3 rounded-xl border border-[#E7D9C8] bg-[#FFF9F0] px-4 py-3"><div className="text-[11px] text-[#8A775F]">{t('models.oauth.deviceHint')}</div><div className="my-2 font-mono text-[26px] font-bold tracking-[.18em] text-[#A94E30]">{deviceCode.userCode}</div><a className="text-[11px] font-semibold text-[#C96442] hover:underline" href={deviceCode.verificationUrl} target="_blank" rel="noreferrer">{t('models.oauth.openDevice')}</a></div>}
   {loginPending&&!deviceCode&&<p className="mt-3 rounded-xl border border-[#E7E0DF] bg-[#F7F1E6] px-3 py-2 text-[11px] text-[#7C7469]">{t('models.oauth.loginHint')}{loginUrl&&<> · <a className="font-semibold text-[#C96442] hover:underline" href={loginUrl} target="_blank" rel="noreferrer">{t('models.oauth.openBrowser')}</a></>}</p>}
   {loginMessage&&<p className="mt-3 rounded-xl border border-[#CFE0CF] bg-[#F4FAF3] px-3 py-2 text-[11px] text-[#4F6F52]">{loginMessage}</p>}
  </section>}

  {(tab==='providers'||tab==='local')&&<section className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-4 flex flex-wrap items-end justify-between gap-3"><div><h2 className="m-0 text-[17px] font-bold tracking-[-.02em] text-[#24211D]">{tab==='local'?t('models.tab.local'):t('models.apiTitle')}</h2><p className="mt-1 text-[12px] text-[#7C7469]">{t('models.apiDesc')}</p></div><Button type="button" onClick={()=>open()} className="inline-flex h-9 items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"><Plus size={14}/> {t('models.direct')}</Button></div>

   {tab==='providers'&&visibleProviders.length>0&&<details ref={providerList} open className="mb-4 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3"><div className="max-h-[320px] overflow-auto"><summary className="cursor-pointer text-[12px] font-semibold text-[#3A342D]">{t('models.registeredList',{count:visibleProviders.length})}</summary><div className="mt-2 divide-y divide-[#EFE9DF]">{visibleProviders.map(p=><div key={p.id} className="flex flex-wrap items-center gap-3 py-2.5"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#C96442]">{p.local?<HardDrive size={15}/>:p.kind==='codex'?<Sparkles size={15}/>:<Cloud size={15}/>}</span><div className="min-w-0 flex-1"><div className="text-[12px] font-semibold text-[#3A342D]">{p.name}{p.kind==='codex'&&<span className="ml-2 rounded bg-[#E9E1F5] px-1.5 py-0.5 text-[9px] font-bold text-[#6B4FA0]">OAuth</span>}{(p.reflection ?? p.learning)&&<span className="ml-2 rounded bg-[#FBE7D8] px-1.5 py-0.5 text-[9px] font-bold text-[#A94E30]">{t('models.review')}</span>}{p.kind!=='cli'&&(!p.local||p.integration==='freellmapi')&&p.hasKey===false&&<span className="ml-2 rounded bg-[#EDE8DE] px-1.5 py-0.5 text-[9px] text-[#8A8176]">{t('models.needKey')}</span>}{!p.enabled&&<span className="ml-2 text-[10px] text-[#A79C8E]">{t('models.disabled')}</span>}</div><div className="mt-0.5 break-all text-[11px] text-[#8A8176]">{p.baseUrl}</div>{p.model&&<div className="mt-0.5 text-[10px] text-[#9C9388]">{t('models.defaultModel')} · {p.model}</div>}{p.requestsPerMinute?<div className="mt-0.5 text-[10px] text-[#9C9388]">{t('models.perMinute',{count:p.requestsPerMinute})}</div>:null}{counts[p.id]?.error&&<div className="mt-1 text-[11px] text-rose-600">{counts[p.id].error}</div>}{counts[p.id]&&!counts[p.id].busy&&!counts[p.id].error&&!counts[p.id].list.length&&<div className="mt-1 text-[11px] text-[#8A8176]">{t('models.emptyList')}</div>}{(counts[p.id]?.list.length??0)>0&&<div className="mt-1 text-[10px] text-emerald-700">{t('models.recognized',{count:counts[p.id].list.length})} · {counts[p.id].list.slice(0,2).join(', ')}</div>}</div><Button className="rounded-md px-2 py-1 text-[11px] text-[#C96442] hover:bg-[#FBF1EC]" type="button" disabled={counts[p.id]?.busy} onClick={()=>void probe(p)}>{counts[p.id]?.busy?t('models.checking'):t('models.checkModels')}</Button>{savedId===p.id&&<span role="status" className="text-[11px] text-emerald-700">{t('models.saved')}</span>}{p.enabled!==false&&<a className="text-[11px] font-semibold text-[#C96442] hover:underline" href={'#/chat?provider='+encodeURIComponent(p.id)}>{t('models.chatSelect')}</a>}<Button className="rounded-md p-2 text-[#8A8176] hover:bg-[#F3EEE5]" type="button" onClick={()=>p.kind==='cli'?setTab('oauth'):open(p)} aria-label={t('models.dlg.settingsAria',{name:p.name})}><Settings2 size={15}/></Button></div>)}</div></div></details>}

   <div className="grid max-h-[460px] grid-cols-1 gap-2 overflow-auto sm:grid-cols-2 xl:grid-cols-3">{visiblePresets.map(preset=>{const existing=providers.value.find(p=>sameProvider(p,preset));const needsKey=Boolean(existing&&!preset.free&&(!preset.local||preset.integration==='freellmapi')&&existing.hasKey===false);return <div key={preset.name} className="flex min-w-0 flex-col gap-2 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] px-3 py-3"><div className="flex items-start justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{preset.name}</strong><small className="block truncate text-[10px] text-[#A79C8E]">{preset.baseUrl.replace(/^https?:\/\//,'')}</small></div><span className={'shrink-0 rounded px-1.5 py-0.5 text-[9px] font-bold '+(preset.category==='free'?'bg-emerald-50 text-emerald-700':preset.category==='local'?'bg-[#E9E1F5] text-[#6B4FA0]':'bg-[#EDE8DE] text-[#8A8176]')}>{preset.category==='free'&&preset.free?t('models.filter.free'):preset.category==='local'?t('models.filter.local'):t('models.filter.cloud')}</span></div><div className="flex items-center justify-between gap-2"><small className="min-w-0 truncate text-[10px] text-[#A79C8E]">{existing?(needsKey?t('models.needKey'):existing.enabled===false?t('models.off'):t('models.registered')):(preset.free?t('models.freeProvider'):preset.integration==='freellmapi'?t('models.integrationKey'):preset.local?t('models.local'):t('models.needKey'))}</small><Button variant={existing?'secondary':'primary'} onClick={()=>existing?open(existing):openPreset(preset)} className="!px-2.5 !py-1 !text-[11px]">{existing?(needsKey?t('models.keyInput'):t('models.settings')):t('models.register')}</Button></div></div>})}</div>
   {!visiblePresets.length&&<p className="py-8 text-center text-[12px] text-[#9C9388]">{t('models.noPreset')}</p>}
  </section>}
  {tab==='local'&&<div className="mt-5"><ModelExplorer/></div>}

  {removedName&&<p role="status" className="mb-4 mt-4 text-[12px] text-emerald-700">{t('models.dlg.disconnected',{name:removedName})}</p>}
  {!modal&&<div className="mt-4"><Notice message={error||providers.error}/></div>}
  {modal&&<DialogBackdrop onDismiss={close}><DialogPanel size="default">
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-4"><h2 className="text-[15px] font-bold">{presetMode&&!edit?name+' '+t('models.editConnection'):edit?t('models.editConnection'):t('models.addConnection')}</h2><Button size="icon" variant="ghost" aria-label={t('models.close')} onClick={close} disabled={saving}><X size={16}/></Button></div>
   <div className="grid gap-3 p-4">
    {presetMode&&!edit?<>
     <div className="rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3">
      <strong className="block text-[13px] text-[#3A342D]">{name}</strong>
      {integration!=='freellmapi'&&<div className="mt-1 break-all text-[11px] text-[#8A8176]">{baseUrl}</div>}
     </div>
     {integration==='freellmapi'&&<FormField label={t('models.dlg.freeServer')}><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)}/></FormField>}
     {baseUrl.includes('ACCOUNT_ID')&&<FormField label={t('models.dlg.accountId')} hint={t('models.dlg.accountIdHint')}><TextInput value={accountId} onChange={e=>setAccountId(e.target.value)} placeholder="a1b2c3d4e5f6..."/></FormField>}
     {(baseUrl.includes('ACCOUNT_ID')||baseUrl.includes('cloudflare.com'))&&<a className="text-[11px] font-semibold text-[#C96442] hover:underline" href="https://dash.cloudflare.com/profile/api-tokens" target="_blank" rel="noreferrer">{t('models.dlg.createCfToken')}</a>}
     {freePreset?<p className="text-[11px] leading-5 text-[#8A8176]">{t('models.freeNote')}</p>:<FormField label={integration==='freellmapi'?t('models.dlg.integrationKey'):local?t('models.dlg.apiKeyOptional'):t('models.dlg.apiKey')} hint={integration==='freellmapi'?t('models.dlg.integrationHint'):undefined}><TextInput autoFocus type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder={t('models.dlg.pasteKey')}/></FormField>}
    </>:<>
     <FormField label={t('models.dlg.connectionName')} hint={t('models.dlg.connectionNameHint')}><TextInput value={name} onChange={e=>setName(e.target.value)} placeholder={t('models.dlg.connectionNamePlaceholder')}/></FormField>
     <FormField label={t('models.dlg.serverUrl')}><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="http://127.0.0.1:1234/v1"/></FormField>
     {baseUrl.includes('ACCOUNT_ID')&&<FormField label={t('models.dlg.accountId')} hint={t('models.dlg.accountIdHint')}><TextInput value={accountId} onChange={e=>setAccountId(e.target.value)} placeholder="a1b2c3d4e5f6..."/></FormField>}
     <FormField label={t('models.dlg.apiMode')}><SelectInput value={kind} onChange={e=>setKind(e.target.value)}><option value="openai">{t('models.dlg.openaiCompat')}</option><option value="anthropic">Anthropic</option><option value="codex">Codex (OAuth)</option><option value="ollama">Ollama</option></SelectInput></FormField>
     {kind==='codex'&&edit&&<FormField label="Codex 기본 모델" hint="계정 모델 목록에서 선택합니다. 모델이 보이지 않으면 OAuth 연결을 갱신하세요."><ModelPicker providerId={edit.id} catalogue={editModels} value={defaultModel} onChange={model=>{setDefaultModel(model);setDefaultEffort('')}} className="w-full"/></FormField>}
     <FormField label={t('chat.reasoningEffort')} hint="에이전트나 대화에서 별도로 지정하지 않았을 때 사용하는 기본값입니다."><ReasoningSelect value={defaultEffort} onChange={setDefaultEffort} model={editModels.value.find(item=>item.id===(defaultModel||edit?.model))} requireCatalog={kind==='codex'}/></FormField>
     {kind!=='codex'&&<FormField label={t('models.dlg.apiKey')}><TextInput type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder={edit?.hasKey?t('models.dlg.keepKey'):t('models.dlg.keyIfNeeded')}/></FormField>}
     <label className="flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={local} onChange={e=>setLocal(e.target.checked)}/> {t('models.dlg.localServer')}</label>
     {edit&&<details open={reflection||undefined} className="rounded-xl border border-[#F0DFD2] bg-[#FFF9F4] p-3">
      <summary className="cursor-pointer text-[12px]">{t('models.dlg.reflection')}</summary>
      <label className="my-2 flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={reflection} onChange={e=>setReflection(e.target.checked)}/> {t('models.dlg.useReflection')}</label>
      {reflection&&<FormField label={t('models.dlg.reflectionModel')} hint={t('models.dlg.reflectionHint')}>
       {baseUrl.trim()===edit.baseUrl&&kind===edit.kind&&!apiKey.trim()
        ?<ModelPicker providerId={edit.id} value={modelId} onChange={setModelId} className="w-full"/>
        :<TextInput value={modelId} onChange={e=>setModelId(e.target.value)} placeholder={t('models.dlg.modelIdPlaceholder')}/>}
      </FormField>}
     </details>}
    </>}
    {kind!=='codex'&&<FormField label="기본 모델 ID" hint="키를 등록한 후 제공자 모델 목록에서 선택하거나 ID를 직접 입력하세요.">
     <div className="grid gap-2">
      {edit?.hasKey&&baseUrl.trim()===edit.baseUrl&&kind===edit.kind&&<ModelPicker providerId={edit.id} value={defaultModel} onChange={setDefaultModel} className="w-full"/>}
      <TextInput value={defaultModel} onChange={e=>setDefaultModel(e.target.value)} placeholder={integration==='opencode-go'?'예: deepseek-v4.1-flash':t('models.dlg.modelIdPlaceholder')}/>
     </div>
    </FormField>}
    <FormField label={t('models.dlg.rpm')} hint={t('models.dlg.rpmHint')}><TextInput type="number" min={0} max={10000} value={rpm} onChange={e=>setRpm(e.target.value)} placeholder="0"/></FormField>
    <details className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
     <summary className="cursor-pointer text-[12px] font-semibold text-[#3A342D]">자동 라우팅 가격 기준 · ₩/100만 토큰</summary>
     <p className="my-2 text-[11px] leading-5 text-[#7C7469]">입·출력 가격을 모두 입력하면 자동 선택 시 비교합니다. 요금제 할인과 캐시 할인을 미리 추정하지 않으며, 채팅에서 지정한 제공자·모델은 변경하지 않습니다.</p>
     <div className="grid grid-cols-2 gap-2">
      <FormField label="입력 100만 토큰 (원)"><TextInput type="number" min={0} max={100000000} step="any" value={costInput} onChange={e=>setCostInput(e.target.value)} placeholder="예: 1000"/></FormField>
      <FormField label="출력 100만 토큰 (원)"><TextInput type="number" min={0} max={100000000} step="any" value={costOutput} onChange={e=>setCostOutput(e.target.value)} placeholder="예: 5000"/></FormField>
     </div>
     <FormField label="자동 선택 품질 점수 · 0~100" hint="선택 사항입니다. 실제 작업 성공률을 기준으로 직접 매긴 점수이며, 값이 없으면 기존 모델 능력 추정 기준을 사용합니다.">
      <TextInput type="number" min={0} max={100} step={1} value={qualityRating} onChange={e=>setQualityRating(e.target.value)} placeholder="측정 후 입력"/>
     </FormField>
    </details>
    {error&&<Notice message={error}/>} 
    {testResult&&<div className={'rounded-xl border px-3 py-2 text-[12px] '+(testResult.ok?'border-[#CFE0CF] bg-[#F4FAF3] text-[#4F6F52]':'border-[#EAD3CB] bg-[#FFF4F0] text-[#9A5B48]')}>{testResult.message}</div>}
   </div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-4">{edit&&edit.id!=='dominion-native'&&<Button variant="danger" className="mr-auto" disabled={saving} onClick={()=>void disconnect()}>{t('models.disconnect')}</Button>}<Button variant="ghost" disabled={saving||testBusy||!baseUrl.trim()} onClick={()=>void testConnection()}>{testBusy?<RefreshCw size={14} className="animate-spin"/>:<ShieldCheck size={14}/>} {testBusy?t('models.testing'):t('models.test')}</Button><Button onClick={close} disabled={saving}>{t('models.cancel')}</Button><Button variant="primary" disabled={saving||!baseUrl.trim()||(needsNewKey&&!apiKey.trim())} onClick={()=>void save()}><Check size={14}/> {saving?t('models.saving'):t('models.save')}</Button></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
