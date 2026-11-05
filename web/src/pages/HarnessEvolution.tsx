import {useEffect,useState} from 'react'
import {GitBranch,Play,Plus,RefreshCw,RotateCcw,ShieldAlert,ShieldCheck,Sparkles,Wrench} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextArea,TextInput} from '../components/Primitives'
import {Notice,refresh} from '../components/UI'
import {PageToolbar} from '../components/PageToolbar'

type Proposal = {
  id:string
  reason:string
  status:string
  baseRevision?:string
  baseBranch?:string
  predictedBenefit?:unknown
  evidenceIds?:unknown
  candidateTests?:{passed?:number;failed?:number}
  failure?:string
  logs?:string[]
  validation?:Record<string,unknown>
  promotion?:{branch?:string;commit?:string;requiresRestart?:boolean;mainUntouched?:boolean}
  createdAt?:string
}
type Payload = {proposals:Proposal[];sourceRoot?:string;selfUpdateEnabled?:boolean;autoEvolveEnabled?:boolean}

export function HarnessEvolutionPage(){
  const [data,setData]=useState<Payload>({proposals:[]})
  const [error,setError]=useState('')
  const [notice,setNotice]=useState('')
  const [busy,setBusy]=useState('')
  const [reason,setReason]=useState('')
  const [benefit,setBenefit]=useState('')
  const [evidence,setEvidence]=useState('')
  const [patch,setPatch]=useState('')
  const [open,setOpen]=useState<string>('')

  const load=async()=>{
    setBusy('load');setError('')
    try{setData(await get<Payload>('/api/harness-evolution'))}
    catch(e){setError((e as Error).message)}
    finally{setBusy('')}
  }
  useEffect(()=>{void load()},[])

  const propose=async()=>{
    setBusy('propose');setError('')
    try{
      await post('/api/harness-evolution',{
        reason,
        predictedBenefit:benefit,
        evidenceIds:evidence.split('\n').map(value=>value.trim()).filter(Boolean),
        patch,
      })
      setReason('');setBenefit('');setEvidence('');setPatch('')
      await load();setNotice('수정안을 저장했습니다. 아직 실행 트리에 적용되지 않았습니다.')
    }catch(e){setError((e as Error).message)}finally{setBusy('')}
  }
  const act=async(id:string,action:'generate'|'validate'|'promote'|'activate'|'rollback')=>{
    setBusy(id+action);setError('');setNotice('')
    try{
      const result=await post<Proposal>('/api/harness-evolution/'+id+'/'+action,{})
      await load()
      setNotice(
        action==='validate'
          ? '격리 worktree 검증 결과: '+result.status
          : action==='generate'
            ? '모델이 수정 diff를 생성했습니다. 이제 격리 검증을 실행할 수 있습니다.'
          : action==='promote'
            ? '전용 브랜치에 승격했습니다. main은 변경되지 않았습니다.'
            : action==='activate'
              ? '검증된 바이너리를 현재 실행 파일에 적용했습니다. 재시작 후 새 버전이 실행됩니다.'
              : '이전 바이너리와 승격 브랜치를 롤백했습니다.'
      )
      refresh()
    }catch(e){setError((e as Error).message)}finally{setBusy('')}
  }

  return <div className="mx-auto w-full max-w-[1180px] px-4 py-5 sm:px-6">
    <PageToolbar>
      <strong className="page-toolbar-title">하네스 자기개선</strong>
      <Button size="sm" variant="ghost" onClick={()=>void load()} disabled={busy==='load'}><RefreshCw size={15} className={busy==='load'?'animate-spin':''}/>새로고침</Button>
    </PageToolbar>

    <div className="mb-5 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-4 text-sm text-[#6F675D]">
      <div className="mb-1 flex items-center gap-2 font-semibold text-[#24211D]"><GitBranch size={17}/>Rust 구현 자체의 개선 순환</div>
      제안 → 격리 worktree 적용 → 실제 <code>cargo build/test</code> → 검증 → 전용 브랜치 승격 → 롤백.
      실행 중인 바이너리와 <code>main</code>은 건드리지 않습니다.
      <div className={`mt-2 rounded-lg px-3 py-2 ${data.selfUpdateEnabled?'bg-emerald-50 text-emerald-800':'bg-amber-50 text-amber-800'}`}>
        {data.selfUpdateEnabled?<><ShieldCheck size={14} className="mr-1 inline"/>승격 허용됨 (DOMINION_ALLOW_SELF_UPDATE=1)</>:<><ShieldAlert size={14} className="mr-1 inline"/>승격 잠김: DOMINION_ALLOW_SELF_UPDATE=1 필요</>}
        {data.sourceRoot&&<span className="ml-2 text-[11px] opacity-70">source: {data.sourceRoot}</span>}
      </div>
    </div>

    <Notice message={error}/>
    {notice&&<div role="status" className="mb-4 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-xs text-emerald-800">{notice}</div>}

    <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><Plus size={16}/>개선 수정안 제출</h2>
        <div className="space-y-3">
          <label className="block text-xs text-[#6F675D]">이유
            <TextInput value={reason} onChange={e=>setReason(e.target.value)} placeholder="반복되는 실패의 원인과 바꿀 동작"/>
          </label>
          <label className="block text-xs text-[#6F675D]">예상 효과
            <TextInput value={benefit} onChange={e=>setBenefit(e.target.value)} placeholder="예: 재시작 후 완료 노드 재실행 제거"/>
          </label>
          <label className="block text-xs text-[#6F675D]">증거 ID (한 줄에 하나)
            <TextArea rows={2} value={evidence} onChange={e=>setEvidence(e.target.value)} placeholder="execution-id / test name"/>
          </label>
          <label className="block text-xs text-[#6F675D]">unified diff
            <TextArea rows={14} value={patch} onChange={e=>setPatch(e.target.value)} placeholder="diff --git a/src/... b/src/..." className="font-mono text-[11px]"/>
          </label>
          <p className="text-[11px] text-[#9C9388]">허용 경로: src/, tests/, web/src/, Cargo.toml. 바이너리·rename·mode 변경·경로 이탈은 거부됩니다.</p>
          <Button variant="primary" disabled={Boolean(busy)||!reason.trim()||!patch.trim()} onClick={()=>void propose()}>{busy==='propose'?'저장 중':'수정안 저장'}</Button>
        </div>
      </section>

      <section className="space-y-3">
        {data.proposals.map(proposal=><article key={proposal.id} className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="text-[10px] font-mono text-[#9C9388]">{proposal.id}</div>
              <h3 className="text-sm font-semibold text-[#24211D]">{proposal.reason}</h3>
              {proposal.predictedBenefit!=null&&<p className="text-xs text-[#6F675D]">{String(proposal.predictedBenefit)}</p>}
            </div>
            <span className={`shrink-0 rounded-full px-2 py-1 text-[10px] font-semibold ${proposal.status==='validated'?'bg-emerald-50 text-emerald-700':proposal.status==='activated'?'bg-violet-50 text-violet-700':proposal.status==='needs_patch'?'bg-slate-100 text-slate-700':proposal.status==='promoted'?'bg-blue-50 text-blue-700':proposal.status==='rejected'?'bg-rose-50 text-rose-700':'bg-amber-50 text-amber-700'}`}>{proposal.status}</span>
          </div>
          <div className="mt-2 flex flex-wrap gap-3 text-[11px] text-[#6F675D]">
            <span>base: {proposal.baseRevision?.slice(0,10)||'-'}</span>
            {proposal.candidateTests&&<span>tests: {proposal.candidateTests.passed||0} passed / {proposal.candidateTests.failed||0} failed</span>}
            {proposal.promotion?.branch&&<span>branch: {proposal.promotion.branch}</span>}
          </div>
          {proposal.failure&&<p className="mt-2 rounded-lg bg-rose-50 px-3 py-2 text-[11px] text-rose-700">{proposal.failure}</p>}
          <div className="mt-3 flex flex-wrap gap-2">
            {proposal.status==='needs_patch'&&<Button size="sm" variant="primary" disabled={Boolean(busy)} onClick={()=>void act(proposal.id,'generate')}>{busy===proposal.id+'generate'?<RefreshCw size={14} className="animate-spin"/>:<Sparkles size={14}/>}패치 생성</Button>}
            {proposal.status!=='promoted'&&proposal.status!=='activated'&&proposal.status!=='rolled_back'&&proposal.status!=='needs_patch'&&<Button size="sm" disabled={Boolean(busy)} onClick={()=>void act(proposal.id,'validate')}>{busy===proposal.id+'validate'?<RefreshCw size={14} className="animate-spin"/>:<Play size={14}/>}격리 검증</Button>}
            {proposal.status==='validated'&&<Button size="sm" variant="primary" disabled={Boolean(busy)||!data.selfUpdateEnabled} onClick={()=>void act(proposal.id,'promote')}><Wrench size={14}/>브랜치 승격</Button>}
            {proposal.status==='promoted'&&<Button size="sm" variant="primary" disabled={Boolean(busy)||!data.selfUpdateEnabled} onClick={()=>void act(proposal.id,'activate')}><Play size={14}/>바이너리 적용</Button>}
            {(proposal.status==='promoted'||proposal.status==='activated')&&<Button size="sm" variant="danger" disabled={Boolean(busy)} onClick={()=>void act(proposal.id,'rollback')}><RotateCcw size={14}/>롤백</Button>}
            <Button size="sm" variant="ghost" onClick={()=>setOpen(open===proposal.id?'':proposal.id)}>{open===proposal.id?'로그 닫기':'로그 보기'}</Button>
          </div>
          {open===proposal.id&&<pre className="mt-3 max-h-72 overflow-auto rounded-lg bg-[#24211D] p-3 text-[10px] leading-relaxed text-[#E7E0D5]">{proposal.logs?.join('\n')||'아직 실행 로그가 없습니다.'}</pre>}
        </article>)}
        {!data.proposals.length&&<div className="rounded-2xl border border-dashed border-[#E7E0D5] p-8 text-center text-xs text-[#9C9388]">제출된 자기개선 수정안이 없습니다.</div>}
      </section>
    </div>
  </div>
}
