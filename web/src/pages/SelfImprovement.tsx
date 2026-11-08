import {useState} from 'react'
import {Activity,ArrowRight,BrainCircuit,CheckCircle2,ChevronDown,FileCode2,FlaskConical,GitBranch,Play,RefreshCw,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {Button,SelectInput,TextArea,TextInput} from '../components/Primitives'
import {FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'

type Dimension={kind:string;baseline?:string;active?:string;winner?:string;activeValue?:number;winnerValue?:number;recommendation?:string}
type AlgorithmEvaluation={id:string;dimensions:Dimension[];totalDimensions:number;modelCalls:number}
type Generated={kind:string;created:AlgorithmCandidate[]}
type AlgorithmCandidate={id:string;name?:string;kind:string;implementation?:string;status?:string;description?:string}
type Experiment={id:string;status:string;metrics?:{pairedCases:number;gains:number;regressions:number;holdoutCases:number;eligible:boolean};verdict?:string}
type Candidate={
 id:string;target:'algorithm'|'harness'|'improver';status:string;baseCommit:string;patchSha256:string;
 hypothesis:string;paths:string[];cases:{id:string;domain:string;holdout:boolean;filter:string}[];
 generatorModel?:string;lineageGeneration?:number;parentCandidateId?:string;
 evaluation?:Experiment|null;deployment?:{commitSha?:string}|null;
 createdAt?:string
}
type RsiList={version:string;candidates:Candidate[];minPairedCases:number}
const kinds=[
 {id:'memory.rerank',label:'기억 재검색·순위'},
 {id:'memory.importance',label:'기억 중요도'},
 {id:'adaptation.selection',label:'실패 대응 알고리즘'},
 {id:'strategy.selection',label:'전략 선택'},
 {id:'selfimprove.search',label:'자기개선 후보 탐색'},
]
const blankCases='[\n  {"id":"case-01","domain":"memory","holdout":true,"filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},\n  {"id":"case-02","domain":"memory","holdout":true,"filter":"engine::room_state::tests::a_hundred_unrelated_owner_decisions_do_not_evict_a_relevant_early_fact"}\n]'
function readableStatus(status:string){
 return ({proposed:'제안됨',inconclusive:'근거 부족',verified:'실험 검증',rejected:'거절됨',approved_for_merge:'수동 병합 승인',reported_deployed:'적용 보고'})[status]||status
}
function isSha(text:string){return /^[0-9a-f]{40}$/i.test(text.trim())}
function jsonObject(text:string,label:string){
 try{const parsed:unknown=JSON.parse(text);if(parsed && typeof parsed==='object' && !Array.isArray(parsed))return parsed as Record<string,unknown>}
 catch{}
 throw Error(label+'은 올바른 JSON 객체여야 합니다.')
}
function jsonCases(text:string){
 let value:unknown
 try{value=JSON.parse(text)}catch{throw Error('평가 계획은 올바른 JSON 배열이어야 합니다.')}
 if(!Array.isArray(value)||value.length<2)throw Error('최소 2개 평가 사례가 필요하며, 승인하려면 8개 이상이 필요합니다.')
 return value
}
export function SelfImprovementPage(){
 const data=useRemote<RsiList>('/api/rsi',{version:'manual-rsi-v1',candidates:[],minPairedCases:8})
 const [kind,setKind]=useState('memory.rerank')
 const [busy,setBusy]=useState('')
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [latestEval,setLatestEval]=useState<AlgorithmEvaluation|null>(null)
 const [generated,setGenerated]=useState<AlgorithmCandidate[]>([])
 const [manualOpen,setManualOpen]=useState(false)
 const [target,setTarget]=useState<Candidate['target']>('algorithm')
 const [commit,setCommit]=useState('')
 const [hypothesis,setHypothesis]=useState('')
 const [patch,setPatch]=useState('')
 const [cases,setCases]=useState(blankCases)
 const [model,setModel]=useState('')
 const [parentId,setParentId]=useState('')
 const [sourceCommit,setSourceCommit]=useState('')
 const [oracle,setOracle]=useState('manual-rsi-v1')
 const [chosen,setChosen]=useState('')
 const [report,setReport]=useState('')
 const [deployedCommit,setDeployedCommit]=useState('')
 const [deployedEvidence,setDeployedEvidence]=useState('')
 const [expanded,setExpanded]=useState<string[]>([])
 const [showPatch,setShowPatch]=useState(false)
 const selected=data.value.candidates.find(candidate=>candidate.id===chosen)
 const attempt=async(label:string,fn:()=>Promise<void>)=>{
  if(busy)return
  setBusy(label);setError('');setNotice('')
  try{await fn()}catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const diagnose=()=>void attempt('diagnose',async()=>{
  const evaluated=await get<AlgorithmEvaluation>('/api/algorithms/evaluate')
  setLatestEval(evaluated)
  const dimension=evaluated.dimensions?.find(row=>row.kind===kind)
  const evidence=dimension?[{
   kind,baseline:dimension.baseline,active:dimension.active,winner:dimension.winner,
   activeValue:dimension.activeValue,winnerValue:dimension.winnerValue,recommendation:dimension.recommendation
  }]:[]
  const result=await post<Generated>('/api/algorithms/generate',{kind,evidence})
  setGenerated(result.created||[])
  setNotice('알고리즘 진단과 후보 생성이 완료됐습니다. 후보는 자동으로 적용되지 않습니다.')
 })
 const register=()=>void attempt('register',async()=>{
  if(!isSha(commit))throw Error('원본 Git 커밋의 40자리 SHA를 입력하세요.')
  if(!hypothesis.trim()||!patch.trim())throw Error('개선 가설과 전체 Git diff 패치를 입력하세요.')
  const created=await post<Candidate>('/api/rsi',{
   target,baseCommit:commit.trim(),hypothesis:hypothesis.trim(),patch,
   cases:jsonCases(cases),oracleVersion:oracle.trim(),generatorModel:model.trim()||'manual',
   ...(parentId.trim()?{parentCandidateId:parentId.trim(),generatorSourceCommit:sourceCommit.trim()}:{}),
  })
  setChosen(created.id);setManualOpen(false);setNotice('코드 자기개선 후보를 등록했습니다. 실제 코드 변경은 아직 일어나지 않았습니다.')
  await data.reload()
 })
 const submitReport=()=>void attempt('evaluate',async()=>{
  if(!selected)throw Error('평가할 후보를 선택하세요.')
  const result=await post<Candidate>('/api/rsi/'+encodeURIComponent(selected.id)+'/evaluate',jsonObject(report,'실험 보고서'))
  setNotice(result.status==='verified'?'동일 조건의 실험 결과가 승격 기준을 만족했습니다.':'실험 결과를 기록했습니다: '+readableStatus(result.status))
  await data.reload()
 })
 const decide=(action:'approve'|'reject')=>void attempt('decide',async()=>{
  if(!selected)throw Error('후보를 선택하세요.')
  if(!window.confirm(action==='approve'?'패치와 실제 실험 증거를 검토하셨습니까? 수동 병합 승인으로만 변경되며 코드가 자동 배포되지 않습니다.':'이 후보를 거절하시겠습니까?'))return
  await post('/api/rsi/'+encodeURIComponent(selected.id)+'/decide',{
   action,confirmation:action==='approve'?'APPROVE':'',
   baseCommit:selected.baseCommit,patchSha256:selected.patchSha256,operator:'owner',
   reviewNote:'DOMINION 자기개선 화면에서 수동 결정'
  })
  setNotice(action==='approve'?'수동 병합 승인으로 표시했습니다. 실제 적용은 별도 작업입니다.':'후보를 거절했습니다.')
  await data.reload()
 })
 const deployment=()=>void attempt('deployment',async()=>{
  if(!selected||!isSha(deployedCommit))throw Error('실제 적용된 커밋의 40자리 SHA를 입력하세요.')
  if(!window.confirm('코드가 실제로 적용되었고 CI 증거를 확인하셨습니까?'))return
  await post('/api/rsi/'+encodeURIComponent(selected.id)+'/deployment',{
   commitSha:deployedCommit.trim(),operator:'owner',confirmation:'DEPLOYED',
   ciEvidence:deployedEvidence.trim(),
  })
  setNotice('적용 결과를 운영자 보고로 기록했습니다. 서버가 Git 배포를 독립적으로 확인한 것은 아닙니다.')
  await data.reload()
 })
 const toggle=(id:string)=>setExpanded(old=>old.includes(id)?old.filter(row=>row!==id):[...old,id])
 return <div className="mx-auto w-full max-w-[1180px] px-5 py-5 sm:px-7 lg:pb-10">
  <Headline eyebrow="DOMINION · RSI" title="재귀적 자기개선" description="알고리즘 진단부터 코드 수준 자기개선 제안·독립 실험·수동 승인을 한 곳에서 관리합니다."
   actions={<Button variant="secondary" disabled={!!busy} onClick={()=>void data.reload()}><RefreshCw size={15}/> 새로고침</Button>}/>
  <Notice message={error||data.error}/>
  {notice&&<div role="status" className="mb-4 flex gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-[12px] text-emerald-800"><CheckCircle2 size={16} className="shrink-0"/>{notice}</div>}
  <Panel className="mb-5 p-5">
   <div className="flex flex-wrap items-start justify-between gap-3">
    <div className="flex gap-3"><span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#A94E30]"><BrainCircuit size={21}/></span>
     <div><h2 className="text-[15px] font-bold">자기개선 시작</h2><p className="mt-1 text-[12px] text-[#7C7469]">Rust 평가기를 실행하고 기존 알고리즘을 바탕으로 개선 후보를 생성합니다. 모델 호출 0회, 코드 변경 없음.</p></div></div>
    <StateTag online>수동 통제</StateTag>
   </div>
   <div className="mt-4 flex flex-wrap items-end gap-3">
    <FormField label="개선 대상 알고리즘"><SelectInput className="min-w-[220px]" value={kind} onChange={e=>setKind(e.target.value)} disabled={!!busy}>
     {kinds.map(item=><option key={item.id} value={item.id}>{item.label}</option>)}
    </SelectInput></FormField>
    <Button variant="primary" disabled={!!busy} onClick={diagnose}><Play size={16}/>{busy==='diagnose'?'진단 중…':'자기개선 시작'}</Button>
    <Button variant="secondary" onClick={()=>setManualOpen(open=>!open)}><FileCode2 size={15}/>{manualOpen?'코드 개선안 닫기':'코드 자체 개선안 등록'}</Button>
   </div>
   {latestEval&&<div className="mt-5 border-t border-[#EFE9DF] pt-4">
    <Subheading title="알고리즘 진단 결과" extra={<span className="text-[11px] text-[#8A8176]">LLM 호출 {latestEval.modelCalls??0}회</span>}/>
    <div className="grid gap-2 sm:grid-cols-2">{latestEval.dimensions?.map(item=><div key={item.kind} className="rounded-lg border border-[#EDE5D9] bg-[#FAF7F1] p-3 text-[12px]">
     <strong>{item.kind}</strong><div className="mt-1 text-[#7C7469]">활성: {item.active||'기본'} · 최적: {item.winner||'-'}</div>
     <span className="text-[11px]">활성 점수 {Number(item.activeValue||0).toFixed(2)} → 후보 점수 {Number(item.winnerValue||0).toFixed(2)}</span>
    </div>)}</div>
   </div>}
   {!!generated.length&&<div className="mt-4 border-t border-[#EFE9DF] pt-4">
    <Subheading title={'생성된 후보 '+generated.length+'개'}/>
    <div className="grid gap-2">{generated.map(item=><div key={item.id} className="flex flex-wrap items-center gap-2 rounded-lg border border-[#E7E0D5] bg-white px-3 py-2 text-[12px]">
     <Activity size={14} className="shrink-0 text-[#A94E30]"/><span className="flex-1"><strong>{item.implementation||item.name}</strong><span className="ml-2 text-[#7C7469]">{item.description}</span></span>
     <StateTag online={false}>{item.status||'평가 대기'}</StateTag>
    </div>)}</div>
    <p className="mt-2 text-[11px] text-[#8A8176]">후보 생성은 최적화의 첫 단계이며, 실제 채택에는 별도 비교 실험과 승인이 필요합니다.</p>
   </div>}
  </Panel>

  {manualOpen&&<Panel className="mb-5 p-5">
   <Subheading title="코드 수준 RSI — 후보 등록" extra={<StateTag>패치 직접 등록</StateTag>}/>
   <p className="mb-4 text-[12px] text-[#7C7469]">LLM이나 개발자가 작성한 Git diff를 등록합니다. 이 화면에서 자동 코드 생성이나 자동 배포는 수행하지 않습니다.</p>
   <div className="grid gap-3 sm:grid-cols-2">
    <FormField label="수정 대상"><SelectInput value={target} onChange={e=>setTarget(e.target.value as Candidate['target'])}>
     <option value="algorithm">알고리즘</option><option value="harness">하네스</option><option value="improver">개선기 자체 (재귀)</option>
    </SelectInput></FormField>
    <FormField label="고정 평가 버전"><TextInput value={oracle} onChange={e=>setOracle(e.target.value)} placeholder="manual-rsi-v1"/></FormField>
    <FormField label="원본 Git 커밋 SHA (40자리)"><TextInput value={commit} onChange={e=>setCommit(e.target.value)} placeholder="git rev-parse HEAD" spellCheck={false}/></FormField>
    <FormField label="개선안을 만든 모델"><TextInput value={model} onChange={e=>setModel(e.target.value)} placeholder="예: Sol 6 High, DeepSeek Flash"/></FormField>
    <FormField label="이전 개선기 후보 ID (선택)"><TextInput value={parentId} onChange={e=>setParentId(e.target.value)} placeholder="rsi-..."/></FormField>
    <FormField label="이전 개선기 적용 커밋 (선택)"><TextInput value={sourceCommit} onChange={e=>setSourceCommit(e.target.value)} placeholder="이전 세대에서 실제 적용한 SHA"/></FormField>
   </div>
   <div className="mt-3 grid gap-3">
    <FormField label="개선 가설"><TextArea rows={3} value={hypothesis} onChange={e=>setHypothesis(e.target.value)} placeholder="어떤 실패를 어떤 구조적 변경으로 줄이는지 기술합니다."/></FormField>
    <FormField label="코드 변경 패치 (unified Git diff)"><TextArea rows={7} className="font-mono text-[11px]" value={patch} onChange={e=>setPatch(e.target.value)} placeholder="diff --git a/src/engine/... b/src/engine/..."/></FormField>
    <FormField label="동결 실험 계획 (JSON 배열)"><TextArea rows={5} className="font-mono text-[11px]" value={cases} onChange={e=>setCases(e.target.value)}/></FormField>
   </div>
   <div className="mt-4 flex flex-wrap items-center justify-between gap-2"><p className="text-[11px] text-[#8A8176]">최소 8개 paired 테스트·holdout 2개가 승인 기준입니다.</p>
    <Button variant="primary" disabled={!!busy} onClick={register}><GitBranch size={16}/>{busy==='register'?'등록 중…':'자기개선 후보 등록'}</Button>
   </div>
  </Panel>}

  <div className="mb-3"><Subheading title={'코드 개선 후보 · '+data.value.candidates.length+'개'} extra={<span className="text-[11px] text-[#8A8176]">원본과 수정판의 독립 실험 후 수동 승인</span>}/></div>
  {!data.value.candidates.length?<Panel className="p-7 text-center text-[12px] text-[#7C7469]">아직 등록된 코드 수준 RSI 후보가 없습니다. 상단에서 진단을 실행하거나 코드 개선안을 등록하세요.</Panel>:
  <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
   <div className="grid content-start gap-2">
    {data.value.candidates.map(item=><Button key={item.id} onClick={()=>{setChosen(item.id);setReport('');setShowPatch(false)}} className={'!flex !h-auto !min-w-0 !flex-col !items-start !justify-start !p-4 !text-left '+(selected?.id===item.id?'!border-[#C96442]':'')}>
     <span className="flex w-full items-center justify-between gap-2"><strong>{item.target==='improver'?'개선기 자체':item.target==='algorithm'?'알고리즘':'하네스'} · {item.lineageGeneration||1}세대</strong><StateTag online={item.status==='verified'||item.status==='reported_deployed'}>{readableStatus(item.status)}</StateTag></span>
     <span className="mt-2 w-full truncate text-[12px] text-[#5C544A]">{item.hypothesis}</span>
     <span className="mt-2 text-[10px] text-[#8A8176]">{item.id.slice(0,20)}… · {item.paths?.length||0}개 파일</span>
    </Button>)}
   </div>
   <Panel className="min-w-0 p-4">
    {selected?<div className="grid gap-3">
     <Subheading title="후보 실험·승인" extra={<StateTag online={selected.status==='verified'}>{readableStatus(selected.status)}</StateTag>}/>
     <p className="text-[12px] text-[#5C544A]">{selected.hypothesis}</p>
     <div className="break-all rounded-lg bg-[#F7F2EA] p-3 font-mono text-[10px] text-[#7C7469]">기준 {selected.baseCommit}<br/>패치 {selected.patchSha256}</div>
     <div className="text-[12px] text-[#7C7469]">대상 파일: {selected.paths?.join(', ')}</div>
     <button type="button" onClick={()=>setShowPatch(v=>!v)} className="flex items-center gap-1 text-left text-[12px] font-semibold text-[#A94E30]"><ChevronDown size={15}/> {showPatch?'변경 코드 닫기':'변경 코드 보기'}</button>
     {showPatch&&<pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-[#E7E0D5] p-3 text-[10px]">{(selected as Candidate & {patch?:string}).patch}</pre>}
     {selected.evaluation&&<div className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[12px]">
      <strong>실험 결과: {selected.evaluation.verdict||selected.evaluation.status}</strong>
      <p className="mt-1 text-[#7C7469]">검사 {selected.evaluation.metrics?.pairedCases||0}건 · 개선 {selected.evaluation.metrics?.gains||0}건 · 회귀 {selected.evaluation.metrics?.regressions||0}건</p>
     </div>}
     {(selected.status==='proposed'||selected.status==='inconclusive')&&<>
      <FormField label="격리 실험 보고서 (JSON)"><TextArea className="font-mono text-[11px]" rows={6} value={report} onChange={e=>setReport(e.target.value)} placeholder="scripts/rsi-lab.sh가 생성한 report.json 내용"/></FormField>
      <Button variant="primary" disabled={!!busy||!report.trim()} onClick={submitReport}><FlaskConical size={15}/> {busy==='evaluate'?'판정 중…':'실험 결과 판정'}</Button>
     </>}
     {selected.status==='verified'&&<Button variant="primary" disabled={!!busy} onClick={()=>decide('approve')}><ShieldCheck size={15}/> 검증된 후보 수동 승인</Button>}
     {['proposed','inconclusive','verified'].includes(selected.status)&&<Button variant="danger" disabled={!!busy} onClick={()=>decide('reject')}>후보 거절</Button>}
     {selected.status==='approved_for_merge'&&<>
      <FormField label="실제 반영된 커밋 SHA"><TextInput value={deployedCommit} onChange={e=>setDeployedCommit(e.target.value)}/></FormField>
      <FormField label="CI 증거 링크 또는 기록"><TextInput value={deployedEvidence} onChange={e=>setDeployedEvidence(e.target.value)}/></FormField>
      <Button variant="secondary" disabled={!!busy} onClick={deployment}><CheckCircle2 size={15}/> 적용 결과 수동 기록</Button>
     </>}
     {selected.status==='reported_deployed'&&<p className="text-[12px] text-[#5C544A]">적용 보고 커밋: <code>{selected.deployment?.commitSha}</code></p>}
     <p className="text-[11px] text-[#8A8176]">후보가 검증·승인되어도 소스 변경, 배포, 모델 학습은 자동 실행되지 않습니다.</p>
    </div>:<div className="grid min-h-40 place-content-center text-center text-[12px] text-[#7C7469]">왼쪽 후보를 선택하면 평가 보고서·수동 승인 상태를 관리할 수 있습니다.</div>}
   </Panel>
  </div>}
  <Panel className="mt-5 p-4">
   <div className="flex items-start gap-3"><ShieldCheck size={19} className="shrink-0 text-[#A94E30]"/><div>
    <strong className="text-[12px]">이 화면의 자동화 범위</strong>
    <p className="mt-1 text-[12px] leading-6 text-[#7C7469]">상단 버튼은 Rust 알고리즘의 결정론적 진단·기존 커널 후보 생성까지 실행합니다. 코드 수준 재귀적 자기개선은 패치가 준비된 경우 후보를 등록하고, 같은 조건의 격리 실험 결과를 제출해 수동 승인하는 방식입니다. LLM이 Rust 소스를 자동으로 다시 쓰는 원클릭 실행은 아직 구현되지 않았습니다.</p>
   </div></div>
  </Panel>
 </div>
}
