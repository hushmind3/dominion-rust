import {useEffect,useState} from 'react'
import {BrainCircuit,Copy,FlaskConical,Plus,RefreshCw,Sparkles} from 'lucide-react'
import {get,post} from '../api'
import {Button,SelectInput,TextArea,TextInput} from '../components/Primitives'
import {Notice,refresh} from '../components/UI'
import {PageToolbar} from '../components/PageToolbar'

type Algorithm = {
  id:string
  kind:string
  name:string
  variant:string
  description?:string
  parameters?:Record<string,unknown>
  source?:string
  status:string
  revision:string
  activatedAt?:string
}
type KindGroup = {kind:string;label:string;kernels:string[];active?:Algorithm;algorithms:Algorithm[]}

export function AlgorithmsPage(){
  const [kinds,setKinds]=useState<KindGroup[]>([])
  const [error,setError]=useState('')
  const [notice,setNotice]=useState('')
  const [busy,setBusy]=useState('')
  const [kind,setKind]=useState('adaptation.selection')
  const [name,setName]=useState('')
  const [variant,setVariant]=useState('')
  const [parameters,setParameters]=useState('{}')
  const [description,setDescription]=useState('')
  const [evidence,setEvidence]=useState('')

  const load=async()=>{
    setBusy('load')
    setError('')
    try{
      const data=await get<{kinds:KindGroup[]}>('/api/algorithms')
      setKinds(data.kinds||[])
    }catch(e){setError((e as Error).message)}
    finally{setBusy('')}
  }
  useEffect(()=>{void load()},[])

  const activate=async(algorithm:Algorithm)=>{
    setBusy(algorithm.id);setError('')
    try{await post('/api/algorithms/'+algorithm.id+'/activate',{});await load();setNotice(algorithm.name+'을(를) '+algorithm.kind+' 활성 알고리즘으로 선택했습니다.');refresh()}
    catch(e){setError((e as Error).message)}finally{setBusy('')}
  }
  const disable=async(algorithm:Algorithm)=>{
    setBusy(algorithm.id);setError('')
    try{await post('/api/algorithms/'+algorithm.id+'/disable',{});await load();refresh()}
    catch(e){setError((e as Error).message)}finally{setBusy('')}
  }
  const register=async()=>{
    setBusy('register');setError('')
    try{
      let parsed:Record<string,unknown>={}
      try{parsed=JSON.parse(parameters||'{}')}catch{throw Error('parameters는 JSON 객체여야 합니다.')}
      await post('/api/algorithms',{kind,name,variant:variant||name,description,parameters:parsed,source:'user'})
      setName('');setVariant('');setDescription('');setParameters('{}')
      await load();setNotice('알고리즘을 공간에 추가했습니다.')
    }catch(e){setError((e as Error).message)}finally{setBusy('')}
  }
  const generate=async()=>{
    setBusy('generate');setError('')
    try{
      const result=await post<{created:Algorithm[]}>('/api/algorithms/generate',{
        kind,
        evidence:evidence.split('\n').map(value=>value.trim()).filter(Boolean),
      })
      await load()
      setNotice('실패 증거에서 후보 '+result.created.length+'개를 생성했습니다.')
    }catch(e){setError((e as Error).message)}finally{setBusy('')}
  }

  const activeFor=(id:string)=>kinds.find(group=>group.kind===id)?.active

  return <div className="mx-auto w-full max-w-[1180px] px-4 py-5 sm:px-6">
    <PageToolbar>
      <strong className="page-toolbar-title">알고리즘 공간</strong>
      <Button size="sm" variant="ghost" onClick={()=>void load()} disabled={busy==='load'}><RefreshCw size={15} className={busy==='load'?'animate-spin':''}/>새로고침</Button>
    </PageToolbar>

    <div className="mb-5 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-4 text-sm text-[#6F675D]">
      <div className="mb-1 flex items-center gap-2 font-semibold text-[#24211D]"><BrainCircuit size={17}/>하네스와 분리된 교체 가능한 알고리즘</div>
      실행·도구·관측·체크포인트·승인은 하네스가 소유합니다. 이 화면의 알고리즘은 하네스가 종류별로 호출하는 판단 부품이며, 추가·교체·생성할 수 있습니다.
      <div className="mt-2 rounded-lg bg-emerald-50 px-3 py-2 text-emerald-800">
        한 알고리즘을 활성화하면 <strong>같은 종류만</strong> 교체됩니다. 다른 종류의 알고리즘, 도구 게이트웨이, 자기개선 실험은 계속 독립적으로 동작합니다.
      </div>
    </div>

    <Notice message={error}/>
    {notice&&<div role="status" className="mb-4 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-xs text-emerald-800">{notice}</div>}

    <div className="mb-6 grid gap-3 md:grid-cols-2">
      {kinds.map(group=><section key={group.kind} className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-[#9C9388]">{group.kind}</div>
            <h2 className="text-sm font-semibold text-[#24211D]">{group.label}</h2>
          </div>
          <span className={`shrink-0 rounded-full px-2 py-1 text-[10px] font-semibold ${group.active?'bg-emerald-50 text-emerald-700':'bg-amber-50 text-amber-700'}`}>{group.active?'active':'fallback'}</span>
        </div>
        <p className="mt-2 min-h-8 text-xs text-[#6F675D]">{group.active?group.active.description||group.active.name:'등록된 활성 알고리즘이 없습니다.'}</p>
        <div className="mt-3 flex flex-wrap gap-1.5">
          {group.algorithms.map(algorithm=><button key={algorithm.id} type="button" disabled={Boolean(busy)||algorithm.status==='active'} onClick={()=>void activate(algorithm)}
            className={`rounded-full border px-2.5 py-1 text-[11px] ${algorithm.status==='active'?'border-emerald-300 bg-emerald-50 text-emerald-800':'border-[#E7E0D5] text-[#6F675D] hover:border-[#C96442] hover:text-[#24211D]'}`}>
            {algorithm.name}{algorithm.status==='active'?' · 사용 중':''}
          </button>)}
        </div>
      </section>)}
    </div>

    <div className="grid gap-4 lg:grid-cols-2">
      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><Plus size={16}/>알고리즘 추가</h2>
        <div className="space-y-3">
          <label className="block text-xs text-[#6F675D]">종류
            <SelectInput value={kind} onChange={e=>setKind(e.target.value)}>
              {kinds.length?kinds.map(group=><option key={group.kind} value={group.kind}>{group.label} · {group.kind}</option>):<option value="adaptation.selection">적응 개입 선택</option>}
            </SelectInput>
          </label>
          <label className="block text-xs text-[#6F675D]">이름
            <TextInput value={name} onChange={e=>setName(e.target.value)} placeholder="예: uncertainty-aware"/>
          </label>
          <label className="block text-xs text-[#6F675D]">실행 커널
            <SelectInput value={variant} onChange={e=>setVariant(e.target.value)}>
              {(kinds.find(group=>group.kind===kind)?.kernels||[]).map(kernel=><option key={kernel} value={kernel}>{kernel}</option>)}
            </SelectInput>
            <span className="mt-1 block text-[10px] text-[#9C9388]">등록된 Rust 커널만 활성화할 수 있습니다. 새 커널은 엔진 구현으로 추가됩니다.</span>
          </label>
          <label className="block text-xs text-[#6F675D]">parameters (JSON)
            <TextArea rows={4} value={parameters} onChange={e=>setParameters(e.target.value)}/>
          </label>
          <label className="block text-xs text-[#6F675D]">설명
            <TextInput value={description} onChange={e=>setDescription(e.target.value)} placeholder="어떤 판단을 바꾸는가"/>
          </label>
          <Button variant="primary" disabled={Boolean(busy)||!name.trim()} onClick={()=>void register()}>{busy==='register'?'저장 중':'공간에 추가'}</Button>
        </div>
      </section>

      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><FlaskConical size={16}/>실패 증거에서 생성</h2>
        <p className="mb-3 text-xs text-[#6F675D]">실행 실패 기록을 근거로 같은 종류의 새 후보를 생성합니다. 생성된 후보는 자동 활성화되지 않습니다.</p>
        <div className="space-y-3">
          <label className="block text-xs text-[#6F675D]">종류
            <SelectInput value={kind} onChange={e=>setKind(e.target.value)}>
              {kinds.map(group=><option key={group.kind} value={group.kind}>{group.label}</option>)}
            </SelectInput>
          </label>
          <label className="block text-xs text-[#6F675D]">증거 (한 줄에 하나)
            <TextArea rows={5} value={evidence} onChange={e=>setEvidence(e.target.value)} placeholder="execution-id 또는 실패 설명"/>
          </label>
          <Button disabled={Boolean(busy)||!evidence.trim()} onClick={()=>void generate()}>{busy==='generate'?<RefreshCw size={15} className="animate-spin"/>:<Sparkles size={15}/>}후보 생성</Button>
        </div>
      </section>
    </div>

    <section className="mt-4 rounded-2xl border border-[#E7E0D5] bg-white p-4">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><Copy size={16}/>전체 알고리즘</h2>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[760px] border-collapse text-left text-xs">
          <thead><tr className="border-b border-[#EFE9DF] text-[#9C9388]"><th className="py-2 pr-3">종류</th><th className="py-2 pr-3">이름</th><th className="py-2 pr-3">variant</th><th className="py-2 pr-3">revision</th><th className="py-2 pr-3">상태</th><th className="py-2">동작</th></tr></thead>
          <tbody>
            {kinds.flatMap(group=>group.algorithms.map(algorithm=><tr key={algorithm.id} className="border-b border-[#F3EEE5]">
              <td className="py-2 pr-3 text-[#6F675D]">{group.label}</td>
              <td className="py-2 pr-3 font-medium text-[#24211D]">{algorithm.name}</td>
              <td className="py-2 pr-3">{algorithm.variant}</td>
              <td className="py-2 pr-3 font-mono text-[10px] text-[#9C9388]">{algorithm.revision?.slice(0,12)}</td>
              <td className="py-2 pr-3">{algorithm.status}</td>
              <td className="py-2"><div className="flex gap-2">
                {algorithm.status!=='active'&&<Button size="sm" disabled={Boolean(busy)} onClick={()=>void activate(algorithm)}>활성화</Button>}
                {algorithm.status==='active'&&<Button size="sm" variant="ghost" disabled={Boolean(busy)||activeFor(algorithm.kind)?.id===algorithm.id&&group.algorithms.length<2} onClick={()=>void disable(algorithm)}>비활성화</Button>}
              </div></td>
            </tr>))}
          </tbody>
        </table>
      </div>
    </section>
  </div>
}
