import {useState} from 'react'
import {ArrowRight,Check,Cloud,HardDrive,Plus,RefreshCw,Settings2,X} from 'lucide-react'
import {Provider,get,post} from '../api'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {ModelExplorer} from '../components/ModelExplorer'

type ProviderPreset={name:string;baseUrl:string;kind:string;local:boolean;model?:string;reflection?:boolean}
const providerPresets:ProviderPreset[]=[

 {name:'Google Gemini',baseUrl:'https://generativelanguage.googleapis.com/v1beta/openai',kind:'openai',local:false},
 {name:'OpenAI',baseUrl:'https://api.openai.com/v1',kind:'openai',local:false},
 {name:'OpenRouter',baseUrl:'https://openrouter.ai/api/v1',kind:'openai',local:false},
 {name:'Vercel AI Gateway',baseUrl:'https://ai-gateway.vercel.sh/v1',kind:'openai',local:false},
 {name:'Groq',baseUrl:'https://api.groq.com/openai/v1',kind:'openai',local:false},
 {name:'DeepSeek',baseUrl:'https://api.deepseek.com/v1',kind:'openai',local:false},
 {name:'Mistral',baseUrl:'https://api.mistral.ai/v1',kind:'openai',local:false},
 {name:'xAI',baseUrl:'https://api.x.ai/v1',kind:'openai',local:false},
 {name:'Together',baseUrl:'https://api.together.xyz/v1',kind:'openai',local:false},
 {name:'Perplexity',baseUrl:'https://api.perplexity.ai',kind:'openai',local:false},
 {name:'Reflection',baseUrl:'https://api.reflection.ai/openai/v1',kind:'openai',local:false,model:'Beam-1.0-501B-A23B',reflection:true},
 {name:'LM Studio',baseUrl:'http://127.0.0.1:1234/v1',kind:'openai',local:true},
 {name:'OpenCode Go',baseUrl:'http://127.0.0.1:4399/v1',kind:'openai',local:true},
 {name:'Ollama',baseUrl:'http://127.0.0.1:11434/v1',kind:'ollama',local:true}
]
export function ModelHub(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const [counts,setCounts]=useState<Record<string,{list:string[];error?:string;busy?:boolean}>>({})
 const [error,setError]=useState('')
 const [modal,setModal]=useState(false),[presetMode,setPresetMode]=useState(false)
 const [edit,setEdit]=useState<Provider|null>(null)
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false),[modelId,setModelId]=useState(''),[reflection,setReflection]=useState(false)
 const probe=async(p:Provider)=>{
  setCounts(old=>({...old,[p.id]:{list:[],busy:true}}))
  try{const list=await get<{id:string}[]>('/api/providers/'+p.id+'/models');setCounts(old=>({...old,[p.id]:{list:list.map(m=>m.id)}}))}
  catch(e){setCounts(old=>({...old,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const open=(p?:Provider)=>{setEdit(p||null);setPresetMode(false);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local));setModelId(p?.model||'');setReflection(Boolean(p?.reflection ?? p?.learning));setError('');setModal(true)}
 const openPreset=(preset:ProviderPreset)=>{setEdit(null);setPresetMode(true);setName(preset.name);setBaseUrl(preset.baseUrl);setKind(preset.kind);setLocal(preset.local);setApiKey('');setModelId(preset.model||'');setReflection(Boolean(preset.reflection));setError('');setModal(true)}
 const save=async()=>{
  if(reflection&&!modelId.trim()){setError('회고 분석 모델로 사용하려면 모델 ID가 필요합니다.');return}
  try{
   const saved=await post<Provider>('/api/providers',{id:edit?.id,name,baseUrl,kind,apiKey:apiKey||undefined,local,enabled:true,model:modelId.trim()||undefined,reflection})
   setModal(false);await providers.reload();refresh()
   if(!modelId.trim()) void probe(saved)
  }catch(e){setError((e as Error).message)}
 }
 return <div className="mx-auto w-full max-w-[1200px] px-4 py-5 sm:px-6 lg:py-6">
  <header className="mb-5 flex flex-wrap items-end justify-between gap-3">
   <div><h1 className="m-0 text-[19px] font-bold tracking-tight text-[#24211D]">모델 관리</h1><p className="mt-1 text-[12px] text-[#7a8798]">로컬 모델을 등록하고 실행하거나 새 모델을 다운로드합니다.</p></div>
   <Button type="button" onClick={()=>open()} className="inline-flex h-8 items-center gap-2 rounded-lg border border-[#E1D9CC] bg-white px-3 text-[11px] font-semibold text-[#40506a] hover:bg-[#FAF7F1]"><Plus size={14}/> API 연결 추가</Button>
  </header>
  <section className="mb-5 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-4 flex flex-wrap items-end justify-between gap-3"><div><h2 className="m-0 text-[17px] font-bold tracking-[-.02em] text-[#24211D]">제공자 빠른 연결</h2><p className="mt-1 text-[12px] text-[#7C7469]">여기는 <strong className="text-[#3A342D]">모델 API 키</strong>를 등록하는 곳입니다. Google/GitHub 로그인(OAuth)은 GitHub·YouTube 계정 연동이며 모델 키가 아닙니다. OpenAI 호환 엔드포인트만 사용합니다. (DOMINION 엔진은 외부 네이티브 SDK에 의존하지 않습니다.)</p></div><Button type="button" onClick={()=>open()} className="inline-flex h-9 items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"><Plus size={14}/> 직접 입력</Button></div>
   <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-5">{providerPresets.map(preset=>{const existing=providers.value.find(p=>p.baseUrl.replace(/\/$/,'')===preset.baseUrl.replace(/\/$/,''));const needsKey=Boolean(existing&&existing.hasKey===false);return <div key={preset.name} className="flex min-w-0 items-center justify-between gap-2 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] px-3 py-2.5"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{preset.name}</strong><small className="block truncate text-[10px] text-[#A79C8E]">{existing?(needsKey?'API 키 필요':'연결됨'):(preset.local?'로컬':'API 키 필요')}</small></div><Button variant={existing?'secondary':'primary'} onClick={()=>existing?open(existing):openPreset(preset)} className="!px-2.5 !py-1 !text-[11px]">{existing?(needsKey?'키 입력':'설정'):'등록'}</Button></div>})}</div>
    {providers.value.length>0&&<details className="mt-4 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3"><summary className="cursor-pointer text-[12px] font-semibold text-[#3A342D]">등록된 제공자 {providers.value.length}개 관리</summary><div className="mt-2 divide-y divide-[#EFE9DF]">{providers.value.map(p=><div key={p.id} className="flex flex-wrap items-center gap-3 py-2.5"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#C96442]">{p.local?<HardDrive size={15}/>:<Cloud size={15}/>}</span><div className="min-w-0 flex-1"><div className="text-[12px] font-semibold text-[#3A342D]">{p.name}{(p.reflection ?? p.learning)&&<span className="ml-2 rounded bg-[#FBE7D8] px-1.5 py-0.5 text-[9px] font-bold text-[#A94E30]">회고</span>}{p.hasKey===false&&<span className="ml-2 rounded bg-[#EDE8DE] px-1.5 py-0.5 text-[9px] text-[#8A8176]">키 필요</span>}{!p.enabled&&<span className="ml-2 text-[10px] text-[#A79C8E]">비활성화</span>}</div><div className="mt-0.5 break-all text-[11px] text-[#8A8176]">{p.baseUrl}</div>{p.model&&<div className="mt-0.5 text-[10px] text-[#9C9388]">모델 · {p.model}</div>}{counts[p.id]?.error&&<div className="mt-1 text-[11px] text-rose-600">{counts[p.id].error}</div>}{counts[p.id]?.list.length>0&&<div className="mt-1 text-[10px] text-emerald-700">{counts[p.id].list.length}개 모델 인식됨 · {counts[p.id].list.slice(0,2).join(', ')}</div>}</div><Button className="rounded-md px-2 py-1 text-[11px] text-[#C96442] hover:bg-[#FBF1EC]" type="button" onClick={()=>void probe(p)}>{counts[p.id]?.busy?'확인 중':'테스트'}</Button><Button className="rounded-md p-2 text-[#8A8176] hover:bg-[#F3EEE5]" type="button" onClick={()=>open(p)} aria-label={p.name+' 설정'}><Settings2 size={15}/></Button></div>)}</div></details>}
  </section>
  <ModelExplorer/>
  {!modal&&error&&<Notice message={error}/>}
  {modal&&<DialogBackdrop onDismiss={()=>setModal(false)}><DialogPanel size="default">
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-4"><h2 className="text-[15px] font-bold">{presetMode&&!edit?name+' 연결':edit?'연결 편집':'모델 서버 연결'}</h2><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setModal(false)}><X size={16}/></Button></div>
   <div className="grid gap-3 p-4">
    {presetMode&&!edit?<>
     <div className="rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3">
      <strong className="block text-[13px] text-[#3A342D]">{name}</strong>
      <div className="mt-1 break-all text-[11px] text-[#8A8176]">{baseUrl}</div>
      {modelId&&<div className="mt-1 text-[11px] text-[#9C9388]">모델 · {modelId}</div>}
     </div>
     <FormField label="API 키"><TextInput autoFocus type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder="API 키를 붙여넣으세요"/></FormField>
     {reflection&&<p className="rounded-lg bg-[#FFF7EF] px-3 py-2 text-[11px] text-[#A94E30]">회고 분석 모델로 자동 지정됩니다.</p>}
    </>:<>
     <FormField label="연결 이름"><TextInput value={name} onChange={e=>setName(e.target.value)} placeholder="예: 내 LM Studio"/></FormField>
     <FormField label="API URL"><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="http://127.0.0.1:1234/v1"/></FormField>
     <FormField label="형식"><SelectInput value={kind} onChange={e=>setKind(e.target.value)}><option value="openai">OpenAI 호환</option><option value="ollama">Ollama</option></SelectInput></FormField>
     <FormField label="기본 모델 ID"><TextInput value={modelId} onChange={e=>setModelId(e.target.value)} placeholder="예: gpt-oss-20b"/></FormField>
     <FormField label="API 키"><TextInput type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder="기존 키 유지 시 비워두기"/></FormField>
     <label className="flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={local} onChange={e=>setLocal(e.target.checked)}/> 로컬 서버</label>
     <label className="flex items-start gap-2 rounded-xl border border-[#F0DFD2] bg-[#FFF9F4] p-3 text-[12px] text-[#6F5A4B]"><TextInput type="checkbox" checked={reflection} onChange={e=>setReflection(e.target.checked)}/><span><strong className="block text-[#3A342D]">회고 분석 모델로 사용</strong><small className="mt-0.5 block text-[11px] text-[#9C9388]">실행 후 실패·교정을 분석해 경험을 추출할 때만 사용합니다. 일반 대화 모델과 별개로 지정할 수 있습니다.</small></span></label>
    </>}
    {error&&<Notice message={error}/>}
   </div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-4"><Button onClick={()=>setModal(false)}>취소</Button><Button variant="primary" onClick={()=>void save()}><Check size={14}/> 저장</Button></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
