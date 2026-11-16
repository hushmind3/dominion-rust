import {useEffect,useState} from 'react'
import {Archive,BookOpen,ChevronLeft,ChevronRight,RefreshCcw,ShieldCheck} from 'lucide-react'
import {get,post} from '../../api'
import {Action,Notice,Panel,Subheading} from '../../components/UI'
import {SelectInput,TextInput} from '../../components/Primitives'

type Item={kind:string;id:string;title:string;status:string;updatedAt:string}
type Page={total:number;rows:Item[];limit:number;offset:number}
const categories=[
 ['job','작업 이력'],['execution','실행 기록'],['operation','도구 실행'],
 ['room','그룹'],['message','대화 메시지'],['session','세션'],
 ['memory','이전 기억'],['experience','교정·실패 경험'],
 ['strategy','검증된 절차'],['workflow','워크플로']
] as const

/** Historical snapshots are never executable tasks or prompt context. */
export function LegacyHistoryPanel(){
  const [kind,setKind]=useState('job')
  const [offset,setOffset]=useState(0)
  const [revision,refresh]=useState(0)
  const [page,setPage]=useState<Page>({total:0,rows:[],limit:20,offset:0})
  const [selected,setSelected]=useState<Item|null>(null)
  const [detail,setDetail]=useState<unknown>(null)
  const [scope,setScope]=useState('global')
  const [error,setError]=useState('')
  const [notice,setNotice]=useState('')
  const [busy,setBusy]=useState(false)
  useEffect(()=>{
    let active=true
    setError('')
    get<Page>('/api/system/legacy-archive?kind='+encodeURIComponent(kind)+'&limit=20&offset='+offset)
      .then(value=>{if(active)setPage(value)})
      .catch(e=>{if(active)setError((e as Error).message)})
    return()=>{active=false}
  },[kind,offset,revision])
  const select=async(item:Item)=>{
    setError('');setNotice('');setSelected(item);setDetail(null)
    try{
      setDetail(await get('/api/system/legacy-archive/item?kind='+encodeURIComponent(item.kind)+'&id='+encodeURIComponent(item.id)))
    }catch(e){setError((e as Error).message)}
  }
  const promote=async()=>{
    if(!selected||selected.kind!=='memory'||busy)return
    setBusy(true);setError('');setNotice('')
    try{
      await post('/api/system/legacy-archive/promote-memory',
        {id:selected.id,scope:scope.trim(),confirm:true})
      setNotice('선택한 이전 기억을 '+scope+' 범위의 현재 기억에 저장했습니다.')
    }catch(e){setError((e as Error).message)}
    finally{setBusy(false)}
  }
  return <Panel className="min-w-0 p-5 xl:col-span-2">
    <Subheading title="이전 데이터 열람" extra={<Archive size={19}/>}/>
    <p className="mt-1 text-xs text-[#7C7469]">
      메인에서 가져온 작업·그룹·기억·대화를 조회합니다.
      이 이력은 현재 작업으로 자동 재개하거나 모델 문맥에 주입하지 않습니다.
    </p>
    <div className="mt-4 flex flex-wrap items-center gap-3">
      <SelectInput value={kind} onChange={e=>{setKind(e.target.value);setOffset(0);setSelected(null);setDetail(null);setNotice('')}}>
        {categories.map(([id,label])=><option key={id} value={id}>{label}</option>)}
      </SelectInput>
      <span className="text-xs text-[#7C7469]">{page.total}개 기록</span>
      <Action variant="secondary" onClick={()=>refresh(x=>x+1)}><RefreshCcw size={15}/> 새로고침</Action>
    </div>
    <div className="mt-3 max-h-64 space-y-2 overflow-y-auto">
      {page.rows.map(item=><button key={item.id} type="button"
        className={'block w-full rounded-lg border px-3 py-2 text-left text-xs '+(selected?.id===item.id?'border-[#C96442] bg-[#F7E8E1]':'border-[#E7E0D5] hover:bg-[#FAF7F1]')}
        onClick={()=>void select(item)}>
        <strong className="block break-all">{item.title||item.id}</strong>
        <span className="text-[#8A8176]">{item.status||'이력'} · {item.updatedAt}</span>
      </button>)}
      {!page.rows.length&&<p className="py-5 text-center text-xs text-[#8A8176]">이전 이력에 해당 기록이 없습니다.</p>}
    </div>
    <div className="mt-3 flex items-center justify-end gap-2">
      <Action variant="secondary" disabled={offset===0} onClick={()=>{setOffset(Math.max(0,offset-20));setSelected(null);setDetail(null)}}><ChevronLeft size={15}/> 이전</Action>
      <Action variant="secondary" disabled={offset+20>=page.total} onClick={()=>{setOffset(offset+20);setSelected(null);setDetail(null)}}>다음 <ChevronRight size={15}/></Action>
    </div>
    {selected&&<div className="mt-4 rounded-lg border border-[#E7E0D5] p-3 text-xs">
      <strong>이전 기록 상세: {selected.id}</strong>
      <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all bg-[#FAF7F1] p-2 text-[11px]">{detail?JSON.stringify(detail,null,2):'불러오는 중…'}</pre>
      {selected.kind==='memory'&&detail!==null&&<div className="mt-3">
        <label className="block text-xs font-medium">현재 기억에 저장할 범위 (global 또는 프로젝트 식별자)</label>
        <TextInput className="mt-2 w-full" value={scope} onChange={e=>setScope(e.target.value)}/>
        <div className="mt-2"><Action disabled={busy||!scope.trim()} onClick={promote}><BookOpen size={15}/> 선택한 기억 저장</Action></div>
        <p className="mt-2 text-[#8A8176]">이 기록은 클릭하기 전까지 현재 에이전트가 사용하지 않습니다. 원본 이력은 보존됩니다.</p>
      </div>}
    </div>}
    {notice&&<p role="status" className="mt-3 flex items-center gap-2 text-xs text-[#366344]"><ShieldCheck size={16}/>{notice}</p>}
    {error&&<div className="mt-3"><Notice message={error}/></div>}
  </Panel>
}
