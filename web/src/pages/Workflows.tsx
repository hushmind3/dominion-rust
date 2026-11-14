import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {StatusPill} from '../components/StatusPill'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {useState} from 'react'
import {ArrowDown,ArrowRight,Check,ChevronRight,Layers3,Play,Plus,Settings2,Trash2,Workflow,X} from 'lucide-react'
import {del,post} from '../api'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {Action,FormField,Headline,Notice,Panel,Subheading,useRemote} from '../components/UI'
import {t,useLocale} from '../i18n'
import {statusLabel} from '../components/StatusPill'
type Step={id?:string;name:string;prompt:string;agentId:string;providerId:string;model:string}
type Flow={id?:string;name:string;description:string;steps:Step[]}
type WorkflowArtifact={id:string;workflowId:string;jobId:string;sessionId:string;
 content:string;sha256:string;steps:number;createdAt:string;
 usage?:{totalTokens?:number;calls?:number;estimatedCalls?:number}}
type WorkflowJob={id:string;kind:string;status:string;stepIndex?:number;stepTotal?:number;
 input?:{workflowId?:string};result?:{sessionId?:string;artifactId?:string;reason?:string}}
const emptyStep=(agentId=''):Step=>({name:t('wf.newStep'),prompt:'',agentId,providerId:'',model:''});
export function WorkflowsPage({openSession}:{openSession:(id:string)=>void}){
 useLocale()
 const flows=useRemote<Flow[]>('/api/workflows',[]);
 const agents=useRemote<any[]>('/api/agents',[]);
 const jobs=useRemote<WorkflowJob[]>('/api/jobs',[]);
 const artifacts=useRemote<WorkflowArtifact[]>('/api/workflow-artifacts',[]);
 const [artifactSource,setArtifactSource]=useState<Record<string,string>>({});
 const [tokenBudgets,setTokenBudgets]=useState<Record<string,string>>({});
 const [expandedArtifact,setExpandedArtifact]=useState<string>('');
 const [edit,setEdit]=useState<Flow|null>(null),[inputs,setInputs]=useState<Record<string,string>>({}),[error,setError]=useState(''),[info,setInfo]=useState(''),[saving,setSaving]=useState(false);
 const setStep=(index:number,patch:Partial<Step>)=>{if(!edit)return;setEdit({...edit,steps:edit.steps.map((s,i)=>i===index?{...s,...patch}:s)})};
 const move=(index:number,change:number)=>{if(!edit||index+change<0||index+change>=edit.steps.length)return;const steps=[...edit.steps];[steps[index],steps[index+change]]=[steps[index+change],steps[index]];setEdit({...edit,steps})};
 const save=async()=>{if(!edit||saving)return;setSaving(true);setError('');try{await post('/api/workflows',edit);setEdit(null);setInfo(t('wf.saved'));await flows.reload()}catch(e){setError((e as Error).message)}finally{setSaving(false)}};
 const start=async(f:Flow)=>{if(!f.id)return;try{
  await post('/api/workflows/'+f.id+'/run',{
   input:inputs[f.id]||'',
   ...(artifactSource[f.id]?{artifactId:artifactSource[f.id]}:{}),
   tokenBudget:Math.max(0,Math.min(2000000,Number(tokenBudgets[f.id])||0))
  });
  setInfo(t('wf.queued',{name:f.name}));await jobs.reload();await artifacts.reload()
 }catch(e){setError((e as Error).message)}};
 const downloadArtifact=(a:WorkflowArtifact)=>{
  const file=new Blob([a.content],{type:'text/markdown;charset=utf-8'});
  const url=URL.createObjectURL(file);const link=document.createElement('a');
  link.href=url;link.download='dominion-workflow-'+a.id+'.md';link.click();
  URL.revokeObjectURL(url);
 };
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8"><Headline eyebrow={t('ui.eyebrow.workflows')} title={t('wf.title')} description={t('wf.description')} actions={<Action onClick={()=>{setError('');setEdit({name:t('wf.new'),description:'',steps:[emptyStep(agents.value[0]?.id||'')]})}}><Plus size={17}/> {t('wf.create')}</Action>}/>
 {error&&<Notice message={error}/>}
 <ExecutionMonitor/>
 {info&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{info}</div>}
 <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]"><Workflow size={22}/></div><div><strong>{t('wf.badges')}</strong><p>{t('wf.note.input')}{"{{input}}"}{t('wf.note.mid')}{"{{previous}}"}{t('wf.note.end')}</p></div></div>
 <Panel className="mb-5 p-4">
  <div className="flex items-center justify-between gap-3">
   <strong className="text-[13px]">완료한 결과물 · 다음 작업에 재사용</strong>
   <Button size="sm" variant="secondary" onClick={()=>void artifacts.reload()}>결과 새로고침</Button>
  </div>
  {artifacts.value.length===0?<p className="mt-2 text-xs text-[#7C7469]">완료한 워크플로의 최종 산출물이 여기에 보존됩니다.</p>:
   <div className="mt-3 grid gap-2">{artifacts.value.slice(0,8).map(a=>
    <div key={a.id} className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-xs">
     <div className="flex flex-wrap items-center justify-between gap-2">
      <span className="font-semibold">{flows.value.find(f=>f.id===a.workflowId)?.name||'워크플로 결과'} · {a.steps}단계</span>
      <span className="text-[#7C7469]">모델 기록 {a.usage?.totalTokens??0}토큰 · {a.createdAt?.slice(0,10)}</span>
     </div>
     <div className="mt-2 flex flex-wrap gap-2">
      <Button size="sm" variant="secondary" onClick={()=>setExpandedArtifact(x=>x===a.id?'':a.id)}>내용 {expandedArtifact===a.id?'접기':'보기'}</Button>
      <Button size="sm" variant="secondary" onClick={()=>downloadArtifact(a)}>Markdown 다운로드</Button>
     </div>
     {expandedArtifact===a.id&&<div className="mt-2">
       <p className="text-[#7C7469]">SHA-256 {a.sha256}</p>
       <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap rounded-lg border border-[#EFE9DF] bg-white p-3 text-xs">{a.content}</pre>
      </div>}
    </div>)}</div>}
 </Panel>
 <Subheading title={t('wf.registered')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{flows.value.length} WORKFLOWS</span>}/>
 <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-2">{flows.value.map(f=>{const job=jobs.value.find(j=>j.kind==='workflow'&&j.input?.workflowId===f.id);return <Panel key={f.id} className="flex min-w-0 flex-col gap-4 p-5"><div className="flex min-w-0 items-start gap-3"><span className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Workflow size={22}/></span><div><h3>{f.name}</h3><p>{f.description||t('wf.independent')}</p></div><Button size="icon" variant="ghost" onClick={()=>setEdit({...f})}><Settings2 size={18}/></Button><Button size="icon" variant="ghost" onClick={async()=>{if(f.id&&confirm(t('wf.confirmDelete'))){await del('/api/workflows/'+f.id);flows.reload()}}}><Trash2 size={17}/></Button></div><div className="flex min-w-0 flex-col gap-2">{f.steps.map((s,i)=><div className="flex min-w-0 items-center gap-3 rounded-lg bg-[#FAF7F1] px-3 py-2" key={s.id||i}><span>{i+1}</span><div><strong>{s.name}</strong><small>{agents.value.find(a=>a.id===s.agentId)?.name||s.agentId}</small></div>{i<f.steps.length-1&&<ArrowRight size={14}/>}</div>)}</div><div className="grid gap-2 rounded-xl bg-[#FAF7F1] p-3">
 <TextInput value={inputs[f.id||'']||''} onChange={e=>setInputs(x=>({...x,[f.id||'']:e.target.value}))} placeholder={t('wf.inputOptional')}/>
 <div className="grid gap-2 sm:grid-cols-[1fr_130px_auto]">
  <SelectInput aria-label="이전 작업 결과물 재사용" value={artifactSource[f.id||'']||''}
   onChange={e=>setArtifactSource(x=>({...x,[f.id||'']:e.target.value}))}>
   <option value="">처음부터 실행</option>
   {artifacts.value.map(a=><option key={a.id} value={a.id}>
    {flows.value.find(flow=>flow.id===a.workflowId)?.name||a.workflowId} · {a.createdAt?.slice(0,10)}
   </option>)}
  </SelectInput>
  <TextInput aria-label="토큰 상한, 0은 제한 없음" inputMode="numeric"
   value={tokenBudgets[f.id||'']||''} onChange={e=>setTokenBudgets(x=>({...x,[f.id||'']:e.target.value}))}
   placeholder="토큰 상한(선택)"/>
  <Button onClick={()=>void start(f)}><Play size={17}/> 실행</Button>
 </div>
 <p className="text-[11px] text-[#7C7469]">이전 결과를 선택하면 SHA가 일치하는 원문을 다음 작업의 참고 입력으로 전달합니다. 예산 도달 시 추가 모델 호출을 중단합니다.</p>
 </div><div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3 text-xs text-[#7C7469]"><span>{t('wf.steps',{count:f.steps.length})}</span>{job&&<><StatusPill status={job.status}>{statusLabel(job.status)} {job.stepIndex||0}/{job.stepTotal||f.steps.length}</StatusPill>{job.result?.sessionId&&<Button onClick={()=>openSession(job.result!.sessionId!)}>{t('wf.viewResult')} <ArrowRight size={14}/></Button>}
 {job.result?.artifactId&&<Button size="sm" variant="secondary" onClick={()=>{
    void artifacts.reload();setExpandedArtifact(job.result!.artifactId!);
 }}>산출물 보기</Button>}</>}</div></Panel>})}{!flows.value.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-[#7C7469]"><Workflow size={26}/><strong>{t('wf.empty')}</strong><p>{t('wf.emptyDesc')}</p></div></Panel>}</div>
 {edit&&<DialogBackdrop onDismiss={()=>setEdit(null)}><DialogPanel size="wide"><div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small>{t('ui.label.workflowBuilder')}</small><h2>{t('wf.builder')}</h2></div><Button size="icon" variant="ghost" onClick={()=>setEdit(null)}><X size={18}/></Button></div><div className="grid gap-4 p-5"><FormField label={t('wf.name')}><TextInput  value={edit.name} onChange={e=>setEdit({...edit,name:e.target.value})}/></FormField><FormField label={t('wf.desc')}><TextInput  value={edit.description} onChange={e=>setEdit({...edit,description:e.target.value})}/></FormField><Subheading title={t('wf.stepsEdit')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{edit.steps.length} STEPS</span>}/>{edit.steps.map((step,i)=><div key={step.id||i} className="grid min-w-0 gap-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4"><div className="flex flex-wrap items-center justify-between gap-3"><strong>{i+1}. {step.name}</strong><div><Button size="icon" variant="ghost" title={t('wf.up')} disabled={i===0} onClick={()=>move(i,-1)}><ChevronRight size={16} style={{transform:'rotate(-90deg)'}}/></Button><Button size="icon" variant="ghost" title={t('wf.down')} disabled={i===edit.steps.length-1} onClick={()=>move(i,1)}><ChevronRight size={16} style={{transform:'rotate(90deg)'}}/></Button><Button size="icon" variant="ghost" title={t('wf.delete')} disabled={edit.steps.length===1} onClick={()=>setEdit({...edit,steps:edit.steps.filter((_,j)=>j!==i)})}><Trash2 size={16}/></Button></div></div><FormField label={t('wf.stepName')}><TextInput  value={step.name} onChange={e=>setStep(i,{name:e.target.value})}/></FormField><FormField label={t('wf.agent')}><SelectInput  value={step.agentId} onChange={e=>setStep(i,{agentId:e.target.value})}><option value="">{t('wf.pickAgent')}</option>{agents.value.map(a=><option value={a.id} key={a.id}>{a.name}{a.model?' · '+a.model:''}</option>)}</SelectInput></FormField><FormField label={t('wf.instructions')}><TextArea className="min-h-28" rows={4} value={step.prompt} onChange={e=>setStep(i,{prompt:e.target.value})} placeholder={t('wf.previous')}/></FormField></div>)}<Action variant="secondary" disabled={edit.steps.length>=30} onClick={()=>setEdit({...edit,steps:[...edit.steps,emptyStep(agents.value[0]?.id||'')]})}><Plus size={16}/> {t('wf.addStep')}</Action></div>{error&&<Notice message={error}/>}<div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5"><Action variant="secondary" onClick={()=>setEdit(null)}>{t('wf.cancel')}</Action><Action onClick={save} disabled={saving||!edit.name.trim()||edit.steps.some(s=>!s.prompt.trim()||!s.agentId)}><Check size={16}/> {t('wf.save')}</Action></div></DialogPanel></DialogBackdrop>}
 </div>
}
