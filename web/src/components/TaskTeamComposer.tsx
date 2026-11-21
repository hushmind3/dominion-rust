import {useState} from 'react'
import {ChevronDown,ChevronUp,Plus,Trash2,Workflow} from 'lucide-react'
import {post,type Provider} from '../api'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'
import {Notice,Panel,refresh,useRemote} from './UI'
import {useProviderModels} from '../hooks/useProviderModels'
import {useLocale} from '../i18n'

type Role='investigator'|'writer'|'reviewer'
type Draft={id:string;role:Role;instruction:string;workspace:string;depends:string;budget:string;steps:string;successTools:string;auditor:boolean}
type Created={id:string}
const initial:Draft[]=[
 {id:'research',role:'investigator',instruction:'Inspect the current source and gather verifiable implementation requirements and evidence.',workspace:'project',depends:'',budget:'1000',steps:'12',successTools:'',auditor:false},
 {id:'implement',role:'writer',instruction:'Implement the requested changes using the verified research and record actual artifact evidence.',workspace:'project',depends:'research',budget:'1000',steps:'20',successTools:'',auditor:false},
 {id:'verify',role:'reviewer',instruction:'Independently verify the resulting implementation and report test evidence and remaining issues.',workspace:'project',depends:'implement',budget:'1000',steps:'12',successTools:'',auditor:false}
]
export function TaskTeamComposer({onCreated}:{onCreated:(id:string)=>void}){
 const ko=useLocale()==='ko'
 const [open,setOpen]=useState(false),[goal,setGoal]=useState('')
 const [repository,setRepository]=useState(''),[budget,setBudget]=useState('3000')
 const [providerId,setProviderId]=useState(''),[model,setModel]=useState('')
 const providers=useRemote<Provider[]>('/api/providers',[])
 const models=useProviderModels(providerId)
 const enabledProviders=providers.value.filter(provider=>provider.enabled!==false)
 const [steps,setSteps]=useState<Draft[]>(()=>initial.map(x=>({...x})))
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const change=(index:number,key:Exclude<keyof Draft,'auditor'>,value:string)=>setSteps(old=>old.map((task,i)=>
  i===index?{...task,[key]:value}:task))
 const submit=async()=>{
  if(busy)return
  setError('')
  const total=Number(budget)
  const taskBudget=steps.map(task=>Number(task.budget))
  if(!goal.trim()||goal.length>20000){setError(ko?'팀 목표를 입력하세요.':'Enter a team goal.');return}
  if(!Number.isFinite(total)||total<=0||taskBudget.some(n=>!Number.isFinite(n)||n<0)||
   taskBudget.reduce((a,b)=>a+b,0)>total+0.000001){
   setError(ko?'각 작업의 예산 합계가 전체 예산을 넘지 않도록 설정하세요.':'Task budget allocation must fit the team total.');return
  }
  if(steps.some(task=>!task.instruction.trim()||!task.id.trim()||
    !Number.isInteger(Number(task.steps))||Number(task.steps)<1||Number(task.steps)>100)){
   setError(ko?'각 작업의 ID·지시문·실행 단계(1~100)를 확인하세요.':'Check each task ID, instruction and step limit.');return
  }
  if(providerId&&(!enabledProviders.some(provider=>provider.id===providerId)||!model.trim())){
   setError(ko?'모델 제공자를 선택한 경우 실제 모델도 선택하세요.':'Select a model when selecting a model provider.');return
  }
  if(steps.some(task=>{
   const tools=task.successTools.split(',').map(item=>item.trim()).filter(Boolean)
   return tools.length+Number(task.auditor)>8||new Set(tools).size!==tools.length
    ||tools.some(tool=>!(/^[a-zA-Z0-9_.:-]{1,128}$/.test(tool)))
  })){
   setError(ko?'작업별 검증 도구는 중복 없이 최대 8개이며 실제 도구 ID를 입력해야 합니다.':'Acceptance requires 1–8 unique exact tool identifiers.');return
  }
  setBusy(true)
  try{
   const created=await post<Created>('/api/v1/task-teams',{
    instruction:goal.trim(),repository:repository.trim()||null,budgetKrw:total,
    providerId,model,
    tasks:steps.map((task,i)=>({
     id:task.id.trim(),instruction:'Team objective:\n'+goal.trim()+'\n\nWorker assignment:\n'+task.instruction.trim(),workspace:task.workspace.trim()||'project',
     role:task.role,depends_on:task.depends.split(',').map(x=>x.trim()).filter(Boolean),
     budget_krw:taskBudget[i],max_steps:Number(task.steps),
     acceptance_checks:[
      ...task.successTools.split(',').map(name=>name.trim()).filter(Boolean)
       .map(name=>({kind:'successful_tool',name})),
      ...(task.auditor?[{kind:'auditor_passed'}]:[])
     ]
    }))
   })
   setOpen(false)
   await refresh('/api/jobs','/api/executions')
   onCreated(created.id)
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <Panel className="mb-5 overflow-hidden">
  <button type="button" className="flex w-full items-center justify-between gap-3 px-4 py-4 text-left" aria-expanded={open} onClick={()=>setOpen(v=>!v)}>
   <span className="flex items-center gap-3"><span className="grid size-9 place-items-center rounded-lg bg-[#F3EEE5]"><Workflow size={18}/></span><span><strong className="block text-sm">{ko?'멀티에이전트 작업 팀':'Multi-agent Task Team'}</strong><small className="block text-xs text-[#7C7469]">{ko?'조사·구현·검증의 순서, 예산, 승인을 직접 관리':'Control worker dependencies, budgets, execution and approvals'}</small></span></span>
   {open?<ChevronUp size={18}/>:<ChevronDown size={18}/>}
  </button>
  {open&&<div className="space-y-4 border-t border-[#E7E0D5] px-4 py-4">
   <label className="block text-xs font-semibold">{ko?'팀 전체 목표':'Team goal'}<TextArea rows={3} value={goal} onChange={e=>setGoal(e.target.value)} placeholder={ko?'작업의 최종 목표와 완료 기준을 적으세요':'Describe the goal and acceptance outcome'}/></label>
   <div className="grid gap-3 sm:grid-cols-2">
    <label className="text-xs font-semibold">{ko?'Git 저장소 상대 경로 (선택)':'Git repository path (optional)'}<TextInput value={repository} onChange={e=>setRepository(e.target.value)} placeholder={ko?'작업공간 내 저장소 경로':'Relative path inside workspace'}/></label>
    <label className="text-xs font-semibold">{ko?'총 예산 (원)':'Total budget (KRW)'}<TextInput type="number" min="1" step="1" value={budget} onChange={e=>setBudget(e.target.value)}/></label>
   </div>
   <div className="grid gap-3 sm:grid-cols-2">
    <label className="text-xs font-semibold">{ko?'실행 모델 제공자':'Model provider'}
     <SelectInput value={providerId} onChange={e=>{setProviderId(e.target.value);setModel('')}}>
      <option value="">{ko?'Rust 기본 모델 사용':'Use default Rust model'}</option>
      {enabledProviders.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
     </SelectInput>
    </label>
    <label className="text-xs font-semibold">{ko?'모든 Worker가 사용할 모델':'Model for all Workers'}
     <SelectInput disabled={!providerId||models.loading} value={model} onChange={e=>setModel(e.target.value)}>
      <option value="">{providerId?(ko?'모델 선택':'Select a model'):(ko?'기본 모델':'Default model')}</option>
      {models.value.filter(item=>item.id).map(item=><option key={item.id} value={item.id}>{item.name||item.id}</option>)}
     </SelectInput>
    </label>
   </div>
   {providerId&&models.error&&<Notice message={models.error}/>}
   <p className="text-[11px] text-[#7C7469]">{ko?'팀을 만들면 초안(일시 정지)으로 저장됩니다. 실제 모델 호출은 아래 실행 상세에서 「팀 시작」을 누른 후 시작합니다.':'The team is saved as a paused draft; model execution starts only after you select Start in its detail view.'}</p>
   <div className="space-y-3">{steps.map((task,index)=><div key={index} className="rounded-xl border border-[#E7E0D5] bg-white p-3">
    <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-xs">{ko?'작업':'Task'} {index+1}</strong><Button size="sm" variant="ghost" disabled={steps.length<=1} onClick={()=>setSteps(items=>items.filter((_,i)=>i!==index))}><Trash2 size={13}/>{ko?'제거':'Remove'}</Button></div>
    <div className="mt-2 grid gap-2 sm:grid-cols-2">
     <label className="text-[11px]">{ko?'작업 ID':'Task ID'}<TextInput value={task.id} onChange={e=>change(index,'id',e.target.value)}/></label>
     <label className="text-[11px]">{ko?'역할':'Role'}<SelectInput value={task.role} onChange={e=>change(index,'role',e.target.value)}><option value="investigator">{ko?'조사':'Investigator'}</option><option value="writer">{ko?'구현':'Writer'}</option><option value="reviewer">{ko?'검증':'Reviewer'}</option></SelectInput></label>
    </div>
    <label className="mt-2 block text-[11px]">{ko?'작업 지시문':'Task instruction'}<TextArea rows={2} value={task.instruction} onChange={e=>change(index,'instruction',e.target.value)}/></label>
    <div className="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
     <label className="text-[11px]">{ko?'선행 작업 ID (쉼표)':'Depends on (comma-separated)'}<TextInput value={task.depends} onChange={e=>change(index,'depends',e.target.value)}/></label>
     <label className="text-[11px]">{ko?'작업공간 구분':'Workspace label'}<TextInput value={task.workspace} onChange={e=>change(index,'workspace',e.target.value)}/></label>
     <label className="text-[11px]">{ko?'예산 (원)':'Budget (KRW)'}<TextInput type="number" min="0" step="1" value={task.budget} onChange={e=>change(index,'budget',e.target.value)}/></label>
     <label className="text-[11px]">{ko?'최대 실행 단계':'Maximum steps'}<TextInput type="number" min="1" max="100" step="1" value={task.steps} onChange={e=>change(index,'steps',e.target.value)}/></label>
    </div>
    <div className="mt-3 grid gap-2 sm:grid-cols-2">
     <label className="text-[11px]">{ko?'완료 검증할 도구 ID (쉼표 구분)':'Required successful tool IDs (comma-separated)'}
      <TextInput value={task.successTools} onChange={e=>change(index,'successTools',e.target.value)} placeholder="read_file, git_status"/>
     </label>
     <label className="flex items-center gap-2 text-[11px]">
      <TextInput type="checkbox" checked={task.auditor}
       onChange={e=>setSteps(items=>items.map((item,i)=>i===index?{...item,auditor:e.target.checked}:item))}/>
      {ko?'독립 모델 감사 PASS 영수증 필수':'Require auditor PASS receipt'}
     </label>
    </div>
    {task.auditor&&<p className="mt-1 text-[10px] text-[#7C7469]">{ko?'실행 서버에서 별도 모델 감사를 활성화해야 PASS 영수증을 얻을 수 있습니다.':'Separate model auditing must be enabled on the server to produce a PASS receipt.'}</p>}
   </div>)}</div>
   <div className="flex flex-wrap gap-2">
    <Button variant="secondary" disabled={steps.length>=16} onClick={()=>setSteps(items=>[...items,{
     id:'task_'+(Math.max(0,...items.map(x=>Number(x.id.replace('task_',''))||0))+1),
     role:'investigator',instruction:'',workspace:'project',depends:'',budget:'0',steps:'10',successTools:'',auditor:false}])}><Plus size={14}/>{ko?'작업 추가':'Add task'}</Button>
    <Button variant="primary" disabled={busy||!goal.trim()} onClick={()=>void submit()}>{ko?'팀 초안 생성':'Create team draft'}</Button>
   </div>
   {error&&<Notice message={error}/>}
  </div>}
 </Panel>
}
