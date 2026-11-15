import {useEffect,useState} from 'react'
import {ArrowRight,Check,ChevronDown,Clock3,Play,ShieldCheck,Sparkles} from 'lucide-react'
import {get,post} from '../../api'
import {Button,SelectInput,TextArea,TextInput} from '../../components/Primitives'
import {GoalOwnerControls} from '../operations/GoalOwnerControls'

type PreparedGoal={
  ready:boolean
  taskKind?:string
  message?:string
  nextPage?:string
  executor?:{agentId?:string;providerId?:string;model?:string}
  selection?:{agentName?:string;providerName?:string;model?:string}
  recommendedWorktree?:boolean
  estimatedCostKrw?:number|null
  declaredRateKrwPerMillionTokens?:{input:number;output:number}|null
}
type GoalRecord={
  id:string
  goal:string
  status:string
  phase?:string
  plan?:string
  iterations?:number
  gaps?:string[]
  finalReport?:{accepted?:boolean;gaps?:string[]}
}
type Destination='models'|'activity'
const terminal=(status:string)=>['completed','cancelled','failed','needs_review'].includes(status)
const phases:Record<string,string>={
  plan:'계획 중',execute:'실행 중',review:'결과 확인 중',repair:'수정 중',
  replan:'다른 해결 방법 탐색 중',complete:'완료',
  needs_review:'검토 필요'
}
const states:Record<string,string>={
  queued:'작업 대기 중',running:'작업 중',completed:'작업 완료',
  needs_review:'확인할 내용이 있습니다',cancelled:'중단됨',failed:'실행 실패'
}

/** First-party UI: preview is read-only; only a second explicit click queues a goal. */
export function QuickGoal({navigate}:{navigate:(page:Destination)=>void}){
  const [goalText,setGoalText]=useState('')
  const [prepared,setPrepared]=useState<PreparedGoal|null>(null)
  const [isolate,setIsolate]=useState(false)
  const [reviewPolicy,setReviewPolicy]=useState<'always'|'acceptance_ready'>('always')
  const [budget,setBudget]=useState(60000)
  const [verifyCommand,setVerifyCommand]=useState('')
  const [busy,setBusy]=useState(false)
  const [error,setError]=useState('')
  const [started,setStarted]=useState<GoalRecord|null>(null)
  const [statusError,setStatusError]=useState('')
  const [showDetails,setShowDetails]=useState(false)

  useEffect(()=>{
    if(!started?.id||terminal(started.status))return
    let mounted=true
    const poll=async()=>{
      try{
        const record=await get<GoalRecord>('/api/goals/'+encodeURIComponent(started.id))
        if(mounted){setStarted(record);setStatusError('')}
      }catch(err){
        if(mounted)setStatusError((err as Error).message)
      }
    }
    const interval=window.setInterval(()=>void poll(),4000)
    return()=>{mounted=false;window.clearInterval(interval)}
  },[started?.id,started?.status])

  const editGoal=(text:string)=>{
    setGoalText(text)
    setPrepared(null)
    setError('')
  }
  const preview=async()=>{
    if(!goalText.trim()||busy)return
    setBusy(true);setError('')
    try{
      const next=await post<PreparedGoal>('/api/goals/prepare',{goal:goalText.trim()})
      setPrepared(next)
      if(next.ready)setIsolate(next.recommendedWorktree===true)
    }catch(err){setError((err as Error).message)}
    finally{setBusy(false)}
  }
  const start=async()=>{
    if(!prepared?.ready||!prepared.executor||busy)return
    setBusy(true);setError('')
    try{
      // Pin the *previewed* route. A later automatic selection must not
      // silently switch the paid provider after the owner has seen it.
      const goal=await post<GoalRecord>('/api/goals',{
        goal:goalText.trim(),
        executor:prepared.executor,
        successCriteria:verifyCommand.trim()
          ?[{type:'command_succeeded',command:verifyCommand.trim()}]:[],
        isolateWorktree:isolate,
        reviewPolicy,
        tokenBudget:budget,
      })
      setStarted(goal)
      setPrepared(null)
      setGoalText('')
    }catch(err){setError((err as Error).message)}
    finally{setBusy(false)}
  }
  const gaps=[...(started?.gaps||[]),...(started?.finalReport?.gaps||[])]
    .filter((value,i,all)=>all.indexOf(value)===i)
    .slice(0,5)
  return <section aria-label="자율 작업 시작" className="mb-5 overflow-hidden rounded-2xl border border-[#E5D4C3] bg-[#FFFDF9] shadow-sm">
    <div className="border-b border-[#F0E7DD] bg-[#FCF6EE] px-5 py-4 sm:px-6">
      <div className="flex items-center gap-2 text-[12px] font-semibold text-[#AE5638]"><Sparkles size={17}/> DOMINION 자율 작업</div>
      <h2 className="mt-1 text-[21px] font-extrabold tracking-tight text-[#302821] sm:text-[24px]">어떤 일을 맡겨 볼까요?</h2>
      <p className="mt-1 text-[13px] text-[#6F675D]">하고 싶은 일을 평소 말하듯 적으세요. 도우미를 찾아 계획하고, 실제로 수행한 뒤 결과를 확인합니다.</p>
    </div>
    <div className="space-y-3 p-5 sm:p-6">
      <label htmlFor="dominion-quick-goal" className="block text-[13px] font-semibold text-[#3E372E]">원하는 결과</label>
      <TextArea id="dominion-quick-goal" rows={3} maxLength={12000}
        value={goalText} onChange={e=>editGoal(e.target.value)}
        placeholder="예: 내 프로젝트에서 로그인 오류를 찾아 수정하고 테스트해 줘. 또는 자료를 조사해서 요약해 줘."
        className="min-h-28 text-[14px]" disabled={busy}/>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-[11px] text-[#82766A]">실행 전에 사용 모델을 알려드립니다. 허락 없이 바로 비용이 발생하지 않습니다.</p>
        <Button variant="primary" onClick={()=>void preview()} disabled={busy||!goalText.trim()}>
          <Sparkles size={15}/>{busy?'확인 중…':'실행 준비 확인'}<ArrowRight size={15}/>
        </Button>
      </div>
      {error&&<p role="alert" className="rounded-lg bg-rose-50 p-3 text-[12px] text-rose-700">{error}</p>}
      {prepared&&!prepared.ready&&<div role="status" className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-4">
        <p className="text-[13px] text-amber-900">{prepared.message||'실행할 모델을 먼저 연결해 주세요.'}</p>
        <Button onClick={()=>navigate('models')}>모델 연결하기 <ArrowRight size={14}/></Button>
      </div>}
      {prepared?.ready&&<div className="space-y-3 rounded-xl border border-[#DFE7D9] bg-[#F6FAF3] p-4">
        <div className="flex items-center gap-2 text-[13px] font-bold text-[#396344]"><Check size={16}/> 준비되었습니다</div>
        <div className="grid gap-2 text-[13px] text-[#453E35] sm:grid-cols-2">
          <div><strong>담당 도우미</strong><p>{prepared.selection?.agentName||'Dominion 기본 실행기'}</p></div>
          <div><strong>사용 모델</strong><p>{prepared.selection?.providerName||prepared.executor?.providerId||'선택된 모델'}</p></div>
        </div>
        <p className="text-[12px] text-[#6F675D]">{prepared.message}</p>
        {prepared.declaredRateKrwPerMillionTokens&&<p className="text-[12px] text-[#6F675D]">제공자에 등록된 기준 요금 · 입력 100만 토큰당 {prepared.declaredRateKrwPerMillionTokens.input.toLocaleString('ko-KR')}원 / 출력 100만 토큰당 {prepared.declaredRateKrwPerMillionTokens.output.toLocaleString('ko-KR')}원 (실제 청구는 별도)</p>}
        <label className="flex cursor-pointer items-start gap-2 text-[13px]">
          <TextInput type="checkbox" checked={isolate} onChange={e=>setIsolate(e.target.checked)} className="mt-1"/>
          <span>원본을 보호하는 별도 공간에서 수정하기 <small className="block text-[11px] text-[#82766A]">프로젝트 파일 작업에 권장합니다. 완성 후 변경 사항을 직접 검토하고 적용합니다.</small></span>
        </label>
        <Button variant="ghost" className="text-[12px]" aria-expanded={showDetails}
          onClick={()=>setShowDetails(open=>!open)}>
          <ChevronDown size={14} className={showDetails?'rotate-180':''}/> 비용·검증 설정 {showDetails?'접기':'보기'}
        </Button>
        {showDetails&&<div className="grid gap-3 rounded-lg border border-[#E2E8DC] bg-white p-3 sm:grid-cols-2">
          <label className="text-[12px] font-semibold">작업 사용량 설정
            <SelectInput className="mt-1" value={budget} onChange={e=>setBudget(Number(e.target.value))}>
              <option value={20000}>가볍게</option><option value={60000}>보통</option>
              <option value={150000}>충분히</option>
            </SelectInput>
          </label>
          <label className="text-[12px] font-semibold">결과 검토
            <SelectInput className="mt-1" value={reviewPolicy} onChange={e=>setReviewPolicy(e.target.value as 'always'|'acceptance_ready')}>
              <option value="always">매 단계 꼼꼼히 확인</option>
              <option value="acceptance_ready">확인할 조건을 충족한 뒤 검토</option>
            </SelectInput>
          </label>
          <label className="text-[12px] font-semibold sm:col-span-2">반드시 성공해야 하는 검사 명령 (선택)
            <TextInput className="mt-1" value={verifyCommand} onChange={e=>setVerifyCommand(e.target.value)} maxLength={512}
              placeholder="예: cargo test 또는 npm test"/>
          </label>
          <p className="text-[11px] text-[#82766A] sm:col-span-2">사용량은 토큰 기준이며 실제 결제금액의 상한이 아닙니다. 유료 모델의 청구액은 제공자에 따라 달라집니다.</p>
        </div>}
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="flex items-center gap-1 text-[12px] text-[#6F675D]"><ShieldCheck size={15}/> 최종 완료는 실제 실행 결과로 검증</span>
          <Button variant="primary" onClick={()=>void start()} disabled={busy}>
            <Play size={15}/>{busy?'작업 등록 중…':'이 설정으로 시작'}
          </Button>
        </div>
      </div>}
      {started&&<div className="space-y-3 rounded-xl border border-[#E0D8CC] p-4" aria-live="polite">
        <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-[14px]">{states[started.status]||started.status}</strong>
          <span className="flex items-center gap-1 text-[12px] text-[#82766A]"><Clock3 size={14}/>{phases[started.phase||'']||started.phase||'상태 확인 중'}{typeof started.iterations==='number'?' · '+started.iterations+'단계':''}</span>
        </div>
        <p className="break-words text-[13px] text-[#50473E]">{started.goal}</p>
        {started.status==='completed'&&<p className="text-[12px] text-emerald-700">✅ 실행 완료. 최종 산출물과 적용 여부는 작업 기록에서 확인하세요.</p>}
        {gaps.length>0&&<div className="space-y-1 text-[12px] text-[#8A5445]">{gaps.map((gap,i)=><p key={i}>• {gap}</p>)}</div>}
        {statusError&&<p role="alert" className="text-[12px] text-rose-700">현재 상태를 새로고침하지 못했습니다: {statusError}</p>}
        <Button onClick={()=>navigate('activity')} className="text-[12px]">작업 기록·결과 보기 <ArrowRight size={13}/></Button>
        {!terminal(started.status)&&<GoalOwnerControls goalId={started.id}/>}
      </div>}
    </div>
  </section>
}
