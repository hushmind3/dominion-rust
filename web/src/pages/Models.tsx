import {useMemo,useRef,useState} from 'react'
import {ArrowRight,Check,Cloud,HardDrive,KeyRound,Plus,RefreshCw,Search,Settings2,ShieldCheck,Sparkles,UserRound,X} from 'lucide-react'
import {Provider,del,get,post} from '../api'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {ModelExplorer} from '../components/ModelExplorer'
import {ModelPicker} from '../components/ModelPicker'
import {t,useLocale} from '../i18n'

type PresetCategory='free'|'cloud'|'local'
type ProviderPreset={name:string;baseUrl:string;kind:string;local:boolean;integration?:'freellmapi';free?:boolean;requestsPerMinute?:number;category:PresetCategory;auth?:'key'|'none';note?:string}
type CliCredential={id:string;name:string;available:boolean;supported:boolean;accountId?:string;model?:string;hint?:string}
type Tab='providers'|'catalog'|'local'|'oauth'

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
 {name:'OpenCode Go',baseUrl:'http://127.0.0.1:4399/v1',kind:'openai',local:true,category:'local',auth:'none'}
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
 const [oauthBusy,setOauthBusy]=useState('')
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false),[modelId,setModelId]=useState(''),[reflection,setReflection]=useState(false),[rpm,setRpm]=useState(''),[accountId,setAccountId]=useState(''),[testBusy,setTestBusy]=useState(false),[testResult,setTestResult]=useState<{ok:boolean;message:string}|null>(null)
 const probe=async(p:Provider)=>{
  setCounts(old=>({...old,[p.id]:{list:[],busy:true}}))
  try{const list=await get<{id:string}[]>('/api/providers/'+p.id+'/models');setCounts(old=>({...old,[p.id]:{list:list.map(m=>m.id)}}))}
  catch(e){setCounts(old=>({...old,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const open=(p?:Provider)=>{setEdit(p||null);setIntegration(p?.integration==='freellmapi'?'freellmapi':undefined);setFreePreset(false);setPresetMode(false);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local));setModelId(p?.reflectionModel||(p?.reflection??p?.learning?p?.model:'')||'');setReflection(Boolean(p?.reflection ?? p?.learning));setRpm(p?.requestsPerMinute?String(p.requestsPerMinute):'');setAccountId(p?.accountId||'');setTestResult(null);setError('');setModal(true)}
 const openPreset=(preset:ProviderPreset)=>{setEdit(null);setIntegration(preset.integration);setFreePreset(Boolean(preset.free&&preset.auth==='none'));setPresetMode(true);setName(preset.name);setBaseUrl(preset.baseUrl);setKind(preset.kind);setLocal(preset.local);setApiKey('');setModelId('');setReflection(false);setRpm(preset.requestsPerMinute?String(preset.requestsPerMinute):'');setAccountId('');setTestResult(null);setError('');setModal(true)}
 const needsNewKey=presetMode&&!freePreset&&(!local||integration==='freellmapi')
 const testConnection=async()=>{
  if(testBusy||!baseUrl.trim())return
  setTestBusy(true);setTestResult(null)
  try{
   const result=await post<{ok:boolean;message?:string;status?:number}>(`/api/providers/${edit?.id||'new'}/test`,{
    baseUrl:baseUrl.trim(),kind,integration,accountId:accountId.trim()||undefined,apiKey:apiKey.trim()||undefined,model:edit?.model,
   })
   setTestResult(result.ok?{ok:true,message:t('models.testOk')}:{ok:false,message:(result.message||t('models.testFail'))+(result.status?` (HTTP ${result.status})`:'')})
  }catch(e){setTestResult({ok:false,message:(e as Error).message})}
  finally{setTestBusy(false)}
 }
 const close=()=>{if(!saving)setModal(false)}
 const save=async()=>{
  if(saving||!baseUrl.trim()||(needsNewKey&&!apiKey.trim()))return
  if(reflection&&!modelId.trim()){setError(t('models.dlg.needReflectionModel'));return}
  setSaving(true);setError('')
  try{
   const saved=await post<Provider>('/api/providers',{
    ...edit,name:name.trim(),baseUrl:baseUrl.trim(),kind,apiKey:apiKey.trim()||undefined,local,integration,accountId:accountId.trim()||undefined,requestsPerMinute:Number(rpm)||0,
    toolMode:edit?.kind===kind?edit.toolMode:undefined,
    enabled:edit?.enabled??true,model:edit?.model,reflection,reflectionModel:reflection?modelId.trim():undefined,
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
 const importCli=async(source:string)=>{
  if(oauthBusy)return
  setOauthBusy(source);setError('')
  try{
   const saved=await post<Provider>('/api/providers/cli/import',{source})
   providers.setValue(rows=>rows.some(p=>p.id===saved.id)?rows.map(p=>p.id===saved.id?saved:p):[...rows,saved])
   setSavedId(saved.id);setTab('providers');refresh()
  }catch(e){setError((e as Error).message)}
  finally{setOauthBusy('')}
 }
 const visiblePresets=useMemo(()=>providerPresets.filter(preset=>{
  if(tab==='local'&&preset.category!=='local')return false
  if(tab==='providers'&&category!=='all'&&preset.category!==category)return false
  if(category!=='all'&&preset.category!==category)return false
  const term=query.trim().toLowerCase()
  return !term||(preset.name+' '+preset.baseUrl).toLowerCase().includes(term)
 }),[tab,category,query])
 const visibleProviders=useMemo(()=>providers.value.filter(p=>{
  if(tab==='local'&&!p.local)return false
  const term=query.trim().toLowerCase()
  return !term||(p.name+' '+p.baseUrl).toLowerCase().includes(term)
 }),[providers.value,tab,query])
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
    {tabs.map(item=><button key={item.id} type="button" onClick={()=>{setTab(item.id);setCategory('all')}} className={'inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] font-semibold transition-colors '+(tab===item.id?'bg-[#C96442] text-white':'text-[#6F675D] hover:bg-[#F3EEE5]')}>{item.label}{item.count!==undefined&&<span className={'rounded px-1.5 py-0.5 text-[10px] '+(tab===item.id?'bg-white/20':'bg-[#F0EAE0] text-[#8A8176]')}>{item.count}</span>}</button>)}
   </div>
   {(tab==='providers'||tab==='local')&&<><div className="relative min-w-[180px] flex-1 sm:max-w-[280px]"><Search size={14} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[#A79C8E]"/><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder={t('models.providerSearch')} className="!pl-8 !text-[12px]"/></div>
    {tab==='providers'&&<div className="flex flex-wrap gap-1">{(['all','free','cloud'] as const).map(value=><button key={value} type="button" onClick={()=>setCategory(value)} className={'rounded-lg border px-2.5 py-1.5 text-[11px] font-semibold '+(category===value?'border-[#C96442] bg-[#FBF1EC] text-[#A94E30]':'border-[#E7E0D5] bg-white text-[#7C7469]')}>{t(value==='all'?'models.filter.all':value==='free'?'models.filter.free':'models.filter.cloud')}</button>)}</div>}</>}
  </div>

  {tab==='catalog'&&<ModelExplorer/>}

  {tab==='oauth'&&<section className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-4"><h2 className="m-0 text-[16px] font-bold text-[#24211D]">{t('models.oauth.title')}</h2><p className="mt-1 text-[12px] text-[#7C7469]">{t('models.oauth.desc')}</p></div>
   <div className="grid grid-cols-1 gap-3 md:grid-cols-2">{cli.value.map(item=><div key={item.id} className="rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-4">
    <div className="flex items-start justify-between gap-3"><div className="min-w-0"><strong className="block text-[13px] text-[#3A342D]">{item.name}</strong><small className={'mt-1 inline-flex items-center gap-1 text-[11px] '+(item.available?'text-emerald-700':'text-[#9C9388]')}><UserRound size={12}/>{item.available?t('models.oauth.available'):t('models.oauth.unavailable')}</small></div>{item.supported&&item.available?<Button type="button" variant="primary" size="sm" disabled={Boolean(oauthBusy)} onClick={()=>void importCli(item.id)}>{oauthBusy===item.id?<RefreshCw size={13} className="animate-spin"/>:<KeyRound size={13}/>} {oauthBusy===item.id?t('models.oauth.importing'):t('models.oauth.import')}</Button>:<span className="rounded bg-[#EFE9DF] px-2 py-1 text-[10px] font-semibold text-[#8A8176]">{item.supported?t('models.oauth.unavailable'):t('models.oauth.coming')}</span>}</div>
    {item.accountId&&<div className="mt-3 break-all rounded-lg bg-white px-2.5 py-2 text-[10px] text-[#8A8176]">{t('models.oauth.account')} · {item.accountId}</div>}
    {item.model&&<div className="mt-1.5 text-[11px] text-[#8A8176]">{t('models.defaultModel')} · {item.model}</div>}
    <p className="mt-2 text-[11px] leading-5 text-[#9C9388]">{item.hint}</p>
   </div>)}</div>
  </section>}

  {(tab==='providers'||tab==='local')&&<section className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-4 flex flex-wrap items-end justify-between gap-3"><div><h2 className="m-0 text-[17px] font-bold tracking-[-.02em] text-[#24211D]">{tab==='local'?t('models.tab.local'):t('models.apiTitle')}</h2><p className="mt-1 text-[12px] text-[#7C7469]">{t('models.apiDesc')}</p></div><Button type="button" onClick={()=>open()} className="inline-flex h-9 items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"><Plus size={14}/> {t('models.direct')}</Button></div>

   {visibleProviders.length>0&&<details ref={providerList} open className="mb-4 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3"><summary className="cursor-pointer text-[12px] font-semibold text-[#3A342D]">{t('models.registeredList',{count:visibleProviders.length})}</summary><div className="mt-2 divide-y divide-[#EFE9DF]">{visibleProviders.map(p=><div key={p.id} className="flex flex-wrap items-center gap-3 py-2.5"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#C96442]">{p.local?<HardDrive size={15}/>:p.kind==='codex'?<Sparkles size={15}/>:<Cloud size={15}/>}</span><div className="min-w-0 flex-1"><div className="text-[12px] font-semibold text-[#3A342D]">{p.name}{p.kind==='codex'&&<span className="ml-2 rounded bg-[#E9E1F5] px-1.5 py-0.5 text-[9px] font-bold text-[#6B4FA0]">OAuth</span>}{(p.reflection ?? p.learning)&&<span className="ml-2 rounded bg-[#FBE7D8] px-1.5 py-0.5 text-[9px] font-bold text-[#A94E30]">{t('models.review')}</span>}{(!p.local||p.integration==='freellmapi')&&p.hasKey===false&&<span className="ml-2 rounded bg-[#EDE8DE] px-1.5 py-0.5 text-[9px] text-[#8A8176]">{t('models.needKey')}</span>}{!p.enabled&&<span className="ml-2 text-[10px] text-[#A79C8E]">{t('models.disabled')}</span>}</div><div className="mt-0.5 break-all text-[11px] text-[#8A8176]">{p.baseUrl}</div>{p.model&&<div className="mt-0.5 text-[10px] text-[#9C9388]">{t('models.defaultModel')} · {p.model}</div>}{p.requestsPerMinute?<div className="mt-0.5 text-[10px] text-[#9C9388]">{t('models.perMinute',{count:p.requestsPerMinute})}</div>:null}{counts[p.id]?.error&&<div className="mt-1 text-[11px] text-rose-600">{counts[p.id].error}</div>}{counts[p.id]&&!counts[p.id].busy&&!counts[p.id].error&&!counts[p.id].list.length&&<div className="mt-1 text-[11px] text-[#8A8176]">{t('models.emptyList')}</div>}{(counts[p.id]?.list.length??0)>0&&<div className="mt-1 text-[10px] text-emerald-700">{t('models.recognized',{count:counts[p.id].list.length})} · {counts[p.id].list.slice(0,2).join(', ')}</div>}</div><Button className="rounded-md px-2 py-1 text-[11px] text-[#C96442] hover:bg-[#FBF1EC]" type="button" disabled={counts[p.id]?.busy} onClick={()=>void probe(p)}>{counts[p.id]?.busy?t('models.checking'):t('models.checkModels')}</Button>{savedId===p.id&&<span role="status" className="text-[11px] text-emerald-700">{t('models.saved')}</span>}{p.enabled!==false&&<a className="text-[11px] font-semibold text-[#C96442] hover:underline" href={'#/chat?provider='+encodeURIComponent(p.id)}>{t('models.chatSelect')}</a>}<Button className="rounded-md p-2 text-[#8A8176] hover:bg-[#F3EEE5]" type="button" onClick={()=>open(p)} aria-label={t('models.dlg.settingsAria',{name:p.name})}><Settings2 size={15}/></Button></div>)}</div></details>}

   <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">{visiblePresets.map(preset=>{const existing=providers.value.find(p=>sameProvider(p,preset));const needsKey=Boolean(existing&&!preset.free&&(!preset.local||preset.integration==='freellmapi')&&existing.hasKey===false);return <div key={preset.name} className="flex min-w-0 flex-col gap-2 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] px-3 py-3"><div className="flex items-start justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{preset.name}</strong><small className="block truncate text-[10px] text-[#A79C8E]">{preset.baseUrl.replace(/^https?:\/\//,'')}</small></div><span className={'shrink-0 rounded px-1.5 py-0.5 text-[9px] font-bold '+(preset.category==='free'?'bg-emerald-50 text-emerald-700':preset.category==='local'?'bg-[#E9E1F5] text-[#6B4FA0]':'bg-[#EDE8DE] text-[#8A8176]')}>{preset.category==='free'&&preset.free?t('models.filter.free'):preset.category==='local'?t('models.filter.local'):t('models.filter.cloud')}</span></div><div className="flex items-center justify-between gap-2"><small className="min-w-0 truncate text-[10px] text-[#A79C8E]">{existing?(needsKey?t('models.needKey'):existing.enabled===false?t('models.off'):t('models.registered')):(preset.free?t('models.freeProvider'):preset.integration==='freellmapi'?t('models.integrationKey'):preset.local?t('models.local'):t('models.needKey'))}</small><Button variant={existing?'secondary':'primary'} onClick={()=>existing?open(existing):openPreset(preset)} className="!px-2.5 !py-1 !text-[11px]">{existing?(needsKey?t('models.keyInput'):t('models.settings')):t('models.register')}</Button></div></div>})}</div>
   {!visiblePresets.length&&<p className="py-8 text-center text-[12px] text-[#9C9388]">{t('models.noPreset')}</p>}
  </section>}

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
     <FormField label={t('models.dlg.apiKey')}><TextInput type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder={edit?.hasKey?t('models.dlg.keepKey'):t('models.dlg.keyIfNeeded')}/></FormField>
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
    <FormField label={t('models.dlg.rpm')} hint={t('models.dlg.rpmHint')}><TextInput type="number" min={0} max={10000} value={rpm} onChange={e=>setRpm(e.target.value)} placeholder="0"/></FormField>
    {error&&<Notice message={error}/>}
    {testResult&&<div className={'rounded-xl border px-3 py-2 text-[12px] '+(testResult.ok?'border-[#CFE0CF] bg-[#F4FAF3] text-[#4F6F52]':'border-[#EAD3CB] bg-[#FFF4F0] text-[#9A5B48]')}>{testResult.message}</div>}
   </div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-4">{edit&&edit.id!=='dominion-native'&&<Button variant="danger" className="mr-auto" disabled={saving} onClick={()=>void disconnect()}>{t('models.disconnect')}</Button>}<Button variant="ghost" disabled={saving||testBusy||!baseUrl.trim()} onClick={()=>void testConnection()}>{testBusy?<RefreshCw size={14} className="animate-spin"/>:<ShieldCheck size={14}/>} {testBusy?t('models.testing'):t('models.test')}</Button><Button onClick={close} disabled={saving}>{t('models.cancel')}</Button><Button variant="primary" disabled={saving||!baseUrl.trim()||(needsNewKey&&!apiKey.trim())} onClick={()=>void save()}><Check size={14}/> {saving?t('models.saving'):t('models.save')}</Button></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
