import {useState} from 'react'
import {ArrowRight,Check,Cloud,HardDrive,Plus,RefreshCw,Settings2,X} from 'lucide-react'
import {Provider,get,post} from '../api'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {ModelExplorer} from '../components/ModelExplorer'

const providerPresets=[

 {name:'FreeLLMAPI',baseUrl:'http://127.0.0.1:3001/v1',kind:'openai',local:false},
 {name:'Google Gemini',baseUrl:'https://generativelanguage.googleapis.com/v1beta/openai',kind:'openai',local:false},
 {name:'OpenAI',baseUrl:'https://api.openai.com/v1',kind:'openai',local:false},
 {name:'OpenRouter',baseUrl:'https://openrouter.ai/api/v1',kind:'openai',local:false},
 {name:'Groq',baseUrl:'https://api.groq.com/openai/v1',kind:'openai',local:false},
 {name:'DeepSeek',baseUrl:'https://api.deepseek.com/v1',kind:'openai',local:false},
 {name:'Mistral',baseUrl:'https://api.mistral.ai/v1',kind:'openai',local:false},
 {name:'xAI',baseUrl:'https://api.x.ai/v1',kind:'openai',local:false},
 {name:'Together',baseUrl:'https://api.together.xyz/v1',kind:'openai',local:false},
 {name:'Perplexity',baseUrl:'https://api.perplexity.ai',kind:'openai',local:false},
 {name:'LM Studio',baseUrl:'http://127.0.0.1:1234/v1',kind:'openai',local:true},
 {name:'OpenCode Go',baseUrl:'http://127.0.0.1:4399/v1',kind:'openai',local:true},
 {name:'Ollama',baseUrl:'http://127.0.0.1:11434/v1',kind:'ollama',local:true}
]
export function ModelHub(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const [counts,setCounts]=useState<Record<string,{list:string[];error?:string;busy?:boolean}>>({})
 const [error,setError]=useState('')
 const [modal,setModal]=useState(false)
 const [edit,setEdit]=useState<Provider|null>(null)
 const [name,setName]=useState(''),[baseUrl,setBaseUrl]=useState(''),[kind,setKind]=useState('openai'),[apiKey,setApiKey]=useState(''),[local,setLocal]=useState(false),[model,setModel]=useState('')
 const probe=async(p:Provider)=>{
  setCounts(old=>({...old,[p.id]:{list:[],busy:true}}))
  try{const list=await get<{id:string}[]>('/api/providers/'+p.id+'/models');setCounts(old=>({...old,[p.id]:{list:list.map(m=>m.id)}}))}
  catch(e){setCounts(old=>({...old,[p.id]:{list:[],error:(e as Error).message}}))}
 }
 const inspectGateway=async(p:Provider)=>{try{const r=await get<any>('/api/providers/'+p.id+'/gateway');setCounts(old=>({...old,[p.id]:{list:r.models.map((m:any)=>m.id),error:`허브 응답 확인 · 준비된 모델 ${r.usableModels}개 · 추론 미시험`}}))}catch(e){setError((e as Error).message)}}
 const open=(p?:Provider)=>{setEdit(p||null);setName(p?.name||'');setBaseUrl(p?.baseUrl||'');setKind(p?.kind||'openai');setApiKey('');setLocal(Boolean(p?.local));setModel((p as any)?.model||'');setError('');setModal(true)}
 const openPreset=(preset:typeof providerPresets[number])=>{setEdit(null);setName(preset.name);setBaseUrl(preset.baseUrl);setKind(preset.kind);setLocal(preset.local);setModel(preset.name==='FreeLLMAPI'?'auto:fast':'');setApiKey('');setError('');setModal(true)}
 const configured=(preset:typeof providerPresets[number])=>providers.value.some(p=>p.baseUrl.replace(/\/$/,'')===preset.baseUrl.replace(/\/$/,''))
 const save=async()=>{
  try{
   await post('/api/providers',{id:edit?.id,name,baseUrl,kind,apiKey:apiKey||undefined,model:model||undefined,integration:name==='FreeLLMAPI'?'freellmapi':(edit as any)?.integration,local,enabled:true})
   setModal(false);await providers.reload();refresh()
  }catch(e){setError((e as Error).message)}
 }
 return <div className="mx-auto w-full max-w-[1200px] px-4 py-5 sm:px-6 lg:py-6">
  <header className="mb-5 flex flex-wrap items-end justify-between gap-3">
   <div><h1 className="m-0 text-[19px] font-bold tracking-tight text-[#24211D]">모델 관리</h1><p className="mt-1 text-[12px] text-[#7a8798]">클라우드 API, 로컬 모델, 선택형 API 허브를 연결합니다.</p></div>
   <Button type="button" onClick={()=>open()} className="inline-flex h-8 items-center gap-2 rounded-lg border border-[#E1D9CC] bg-white px-3 text-[11px] font-semibold text-[#40506a] hover:bg-[#FAF7F1]"><Plus size={14}/> API 연결 추가</Button>
  </header>
  <section className="mb-5 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5 shadow-[0_1px_2px_rgba(36,33,29,.04)]">
   <div className="mb-4 flex flex-wrap items-end justify-between gap-3"><div><h2 className="m-0 text-[17px] font-bold tracking-[-.02em] text-[#24211D]">제공자 빠른 연결</h2><p className="mt-1 text-[12px] text-[#7C7469]">여기는 <strong className="text-[#3A342D]">모델 API 키</strong>를 등록하는 곳입니다. Google 계정 로그인·YouTube 업로드 연결과 모델 API 키는 별도로 관리합니다. OpenAI 호환 엔드포인트만 사용합니다. (DOMINION 엔진은 외부 네이티브 SDK에 의존하지 않습니다.)</p></div><Button type="button" onClick={()=>open()} className="inline-flex h-9 items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"><Plus size={14}/> 직접 입력</Button></div>
   <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-5">{providerPresets.map(preset=>{const connected=configured(preset);return <div key={preset.name} className="flex min-w-0 items-center justify-between gap-2 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] px-3 py-2.5"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{preset.name}</strong><small className="block truncate text-[10px] text-[#A79C8E]">{preset.local?'로컬':'API 키 필요'}</small></div><Button variant={connected?'secondary':'primary'} disabled={connected} onClick={()=>openPreset(preset)} className="!px-2.5 !py-1 !text-[11px]">{connected?'등록됨':'등록'}</Button></div>})}</div>
    {providers.value.length>0&&<details className="mt-4 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3"><summary className="cursor-pointer text-[12px] font-semibold text-[#3A342D]">등록된 제공자 {providers.value.length}개 관리</summary><div className="mt-2 divide-y divide-[#EFE9DF]">{providers.value.map(p=><div key={p.id} className="flex flex-wrap items-center gap-3 py-2.5"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#C96442]">{p.local?<HardDrive size={15}/>:<Cloud size={15}/>}</span><div className="min-w-0 flex-1"><div className="text-[12px] font-semibold text-[#3A342D]">{p.name}{!p.enabled&&<span className="ml-2 text-[10px] text-[#A79C8E]">비활성화</span>}</div><div className="mt-0.5 break-all text-[11px] text-[#8A8176]">{p.baseUrl}</div>{counts[p.id]?.error&&<div className="mt-1 text-[11px] text-rose-600">{counts[p.id].error}</div>}{counts[p.id]?.list.length>0&&<div className="mt-1 text-[10px] text-emerald-700">{counts[p.id].list.length}개 모델 인식됨 · {counts[p.id].list.slice(0,2).join(', ')}</div>}</div><Button className="rounded-md px-2 py-1 text-[11px] text-[#C96442] hover:bg-[#FBF1EC]" type="button" onClick={()=>void probe(p)}>{counts[p.id]?.busy?'확인 중':'테스트'}</Button><Button className="rounded-md p-2 text-[#8A8176] hover:bg-[#F3EEE5]" type="button" onClick={()=>open(p)} aria-label={p.name+' 설정'}><Settings2 size={15}/></Button></div>)}</div></details>}
  </section>
  <section className="mb-5 rounded-xl border border-[#E7E0D5] bg-white p-4"><h2 className="text-base font-semibold">FreeLLMAPI 허브 연결</h2><p className="my-2 text-sm leading-6 text-[#62594e]">가입한 제공자들의 API 키는 FreeLLMAPI의 Keys에 등록하고, 여기에는 허브 통합 키 하나를 넣습니다. 기본 모델 auto:fast는 허브의 속도 우선 라우팅을 사용합니다. DOMINION Rust 엔진에서 표준 HTTP로 연결하며 허브 설치와 실행은 별도입니다.</p><div className="flex flex-wrap gap-3"><Button onClick={()=>openPreset(providerPresets[0])}>허브 연결 추가</Button><a className="text-sm underline" href="https://github.com/tashfeenahmed/freellmapi/blob/main/docs/en/api/01-rest-api.md" target="_blank" rel="noreferrer">공식 연결 문서</a></div>{providers.value.filter(p=>(p as any).integration==='freellmapi').map(p=><div key={p.id} className="mt-3 text-sm"><strong>{p.name}</strong><Button variant="ghost" onClick={()=>void inspectGateway(p)}>모델·사용량 진단</Button>{counts[p.id]?.error&&<p>{counts[p.id].error}</p>}</div>)}</section><ModelExplorer/>
  {!modal&&error&&<Notice message={error}/>}
  {modal&&<DialogBackdrop onDismiss={()=>setModal(false)}><DialogPanel size="default">
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-4"><h2 className="text-[15px] font-bold">{edit?'연결 편집':'모델 서버 연결'}</h2><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setModal(false)}><X size={16}/></Button></div>
   <div className="grid gap-3 p-4"><FormField label="연결 이름"><TextInput value={name} onChange={e=>setName(e.target.value)} placeholder="예: 내 LM Studio"/></FormField><FormField label="API URL"><TextInput value={baseUrl} onChange={e=>setBaseUrl(e.target.value)} placeholder="http://127.0.0.1:1234/v1"/></FormField><FormField label="형식"><SelectInput value={kind} onChange={e=>setKind(e.target.value)}><option value="openai">OpenAI 호환</option><option value="ollama">Ollama</option></SelectInput></FormField><FormField label="기본 모델 ID"><TextInput value={model} onChange={e=>setModel(e.target.value)} placeholder="허브: auto:fast · 제공자: 모델 ID"/></FormField><FormField label="API 키"><TextInput type="password" value={apiKey} onChange={e=>setApiKey(e.target.value)} placeholder="기존 키 유지 시 비워두기"/></FormField><label className="flex items-center gap-2 text-[12px]"><TextInput type="checkbox" checked={local} onChange={e=>setLocal(e.target.checked)}/> 로컬 서버</label>{error&&<Notice message={error}/>}</div>
   <div className="flex justify-end gap-2 border-t border-[#EFE9DF] p-4"><Button onClick={()=>setModal(false)}>취소</Button><Button variant="primary" onClick={()=>void save()}><Check size={14}/> 저장</Button></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
