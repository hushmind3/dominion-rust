import {GoalConsole} from './workflows/GoalConsole'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {StatusPill} from '../components/StatusPill'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {useEffect,useRef,useState} from 'react'
import {ArrowDown,ArrowRight,Check,ChevronRight,Layers3,Play,Plus,Settings2,Trash2,Workflow,X} from 'lucide-react'
import {del,post} from '../api'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {Action,FormField,Headline,Notice,Panel,Subheading,useRemote} from '../components/UI'
import {t,useLocale} from '../i18n'
import {statusLabel} from '../components/StatusPill'
import {DeliverableDialog,useWorkflowDeliverable} from './workflows/DeliverablePanel'
type Step={id?:string;name:string;prompt:string;agentId:string;providerId:string;model:string;verifyPath?:string}
type Flow={id?:string;name:string;description:string;steps:Step[]}
const emptyStep=(agentId=''):Step=>({name:t('wf.newStep'),prompt:'',agentId,providerId:'',model:''});
export function WorkflowsPage({openSession}:{openSession:(id:string)=>void}){
 useLocale()
 const flows=useRemote<Flow[]>('/api/workflows',[]);
 const agents=useRemote<any[]>('/api/agents',[]);
 const jobs=useRemote<any[]>('/api/jobs',[]);
 const [edit,setEdit]=useState<Flow|null>(null),[inputs,setInputs]=useState<Record<string,string>>({}),[error,setError]=useState(''),[info,setInfo]=useState(''),[saving,setSaving]=useState(false);
 const delivery=useWorkflowDeliverable(),openedAgent=useRef('');
 const [inFlight,setInFlight]=useState(''),[retryIds,setRetryIds]=useState<Record<string,string>>({});
 useEffect(()=>{
  const raw=window.location.hash.split('?')[1]||'';
  const agentId=new URLSearchParams(raw).get('agent');
  if(!agentId||openedAgent.current===agentId)return;
  const agent=agents.value.find(a=>a.id===agentId);
  if(!agent)return;
  openedAgent.current=agentId;
  const reviewer=agents.value.find(a=>a.id!==agentId&&a.providerId)||agent;
  setEdit({name:agent.name+' · 실행과 독립 검토',description:'실제 에이전트 2단계 수행 후 영속 산출물로 납품합니다.',
   steps:[
    {name:'실행',agentId:agent.id,providerId:'',model:'',
     prompt:'다음 사용자 과제를 실제 가능한 도구로 수행하고 결과·근거를 남기세요:\n{{input}}'},
    {name:'결과 검토·납품',agentId:reviewer.id,providerId:'',model:'',
     prompt:'이전 에이전트의 결과를 검토하고 해결된 내용·남은 문제·실제 증거를 구분해 최종 결과물을 작성하세요. 새 작업을 했다고 주장하지 마세요.\n과제: {{input}}\n이전 단계 결과: {{previous}}'}
   ]});
 },[agents.value]);
 const setStep=(index:number,patch:Partial<Step>)=>{if(!edit)return;setEdit({...edit,steps:edit.steps.map((s,i)=>i===index?{...s,...patch}:s)})};
 const move=(index:number,change:number)=>{if(!edit||index+change<0||index+change>=edit.steps.length)return;const steps=[...edit.steps];[steps[index],steps[index+change]]=[steps[index+change],steps[index]];setEdit({...edit,steps})};
 const save=async()=>{if(!edit||saving)return;setSaving(true);setError('');try{await post('/api/workflows',edit);setEdit(null);setInfo(t('wf.saved'));await flows.reload()}catch(e){setError((e as Error).message)}finally{setSaving(false)}};
 const start=async(f:Flow)=>{
  if(!f.id||inFlight)return;
  const requestId=retryIds[f.id]||crypto.randomUUID();
  setRetryIds(ids=>({...ids,[f.id!]:requestId}));
  setInFlight(f.id);setError('');
  try{
   const job=await post<{id:string}>('/api/workflows/'+encodeURIComponent(f.id)+'/run',{
    input:inputs[f.id]||'',requestId
   });
   setRetryIds(ids=>{const next={...ids};delete next[f.id!];return next});
   setInfo(f.name+' 실행 등록 · 작업 ID '+job.id);
   await jobs.reload()
  }catch(e){setError((e as Error).message+' · 재시도해도 같은 요청 ID로 중복 과금을 방지합니다.')}
  finally{setInFlight('')}
 };
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8"><Headline eyebrow={t('ui.eyebrow.workflows')} title={t('wf.title')} description={t('wf.description')} actions={<Action onClick={()=>{setError('');setEdit({name:t('wf.new'),description:'',steps:[emptyStep(agents.value[0]?.id||'')]})}}><Plus size={17}/> {t('wf.create')}</Action>}/>
 {error&&<Notice message={error}/>}
 {delivery.error&&<Notice message={delivery.error}/>}
 {delivery.item&&<DeliverableDialog item={delivery.item} onClose={delivery.close} openSession={openSession}/>}
 <GoalConsole openSession={openSession}/>
 <ExecutionMonitor/>
 {info&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{info}</div>}
 <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]"><Workflow size={22}/></div><div><strong>{t('wf.badges')}</strong><p>{t('wf.note.input')}{"{{input}}"}{t('wf.note.mid')}{"{{previous}}"}{t('wf.note.end')}</p></div></div>
 <Subheading title={t('wf.registered')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{flows.value.length} WORKFLOWS</span>}/>
 <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-2">{flows.value.map(f=>{const job=jobs.value.filter(j=>j.kind==='workflow'&&j.input?.workflowId===f.id)
 .sort((a,b)=>String(b.queuedAt||'').localeCompare(String(a.queuedAt||'')))[0];return <Panel key={f.id} className="flex min-w-0 flex-col gap-4 p-5"><div className="flex min-w-0 items-start gap-3"><span className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Workflow size={22}/></span><div><h3>{f.name}</h3><p>{f.description||t('wf.independent')}</p></div><Button size="icon" variant="ghost" onClick={()=>setEdit({...f})}><Settings2 size={18}/></Button><Button size="icon" variant="ghost" onClick={async()=>{if(f.id&&confirm(t('wf.confirmDelete'))){await del('/api/workflows/'+f.id);flows.reload()}}}><Trash2 size={17}/></Button></div><div className="flex min-w-0 flex-col gap-2">{f.steps.map((s,i)=><div className="flex min-w-0 items-center gap-3 rounded-lg bg-[#FAF7F1] px-3 py-2" key={s.id||i}><span>{i+1}</span><div><strong>{s.name}</strong><small>{agents.value.find(a=>a.id===s.agentId)?.name||s.agentId}</small></div>{i<f.steps.length-1&&<ArrowRight size={14}/>}</div>)}</div><div className="flex items-center gap-2 rounded-xl bg-[#FAF7F1] p-2"><TextInput value={inputs[f.id||'']||''} onChange={e=>setInputs(x=>({...x,[f.id||'']:e.target.value}))} placeholder={t('wf.inputOptional')}/><Button disabled={inFlight===f.id} onClick={()=>start(f)}><Play size={17} fill="currentColor"/></Button></div><div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3 text-xs text-[#7C7469]"><span>{t('wf.steps',{count:f.steps.length})}</span>{job&&<><StatusPill status={job.status}>{statusLabel(job.status)} {job.stepIndex||0}/{job.stepTotal||f.steps.length}</StatusPill>{job.result?.sessionId&&<Button onClick={()=>openSession(job.result.sessionId)}>{t('wf.viewResult')} <ArrowRight size={14}/></Button>}
 {job.result?.deliverableId&&<Button variant="secondary" disabled={delivery.busy} onClick={()=>void delivery.open(job.result.deliverableId)}>영속 산출물 보기</Button>}
 {job.error&&<span className="w-full text-amber-800">{String(job.error).slice(0,230)}</span>}</>}</div></Panel>})}{!flows.value.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-[#7C7469]"><Workflow size={26}/><strong>{t('wf.empty')}</strong><p>{t('wf.emptyDesc')}</p></div></Panel>}</div>
 {edit&&<DialogBackdrop onDismiss={()=>setEdit(null)}><DialogPanel size="wide"><div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small>{t('ui.label.workflowBuilder')}</small><h2>{t('wf.builder')}</h2></div><Button size="icon" variant="ghost" onClick={()=>setEdit(null)}><X size={18}/></Button></div><div className="grid gap-4 p-5"><FormField label={t('wf.name')}><TextInput  value={edit.name} onChange={e=>setEdit({...edit,name:e.target.value})}/></FormField><FormField label={t('wf.desc')}><TextInput  value={edit.description} onChange={e=>setEdit({...edit,description:e.target.value})}/></FormField><Subheading title={t('wf.stepsEdit')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{edit.steps.length} STEPS</span>}/>{edit.steps.map((step,i)=><div key={step.id||i} className="grid min-w-0 gap-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4"><div className="flex flex-wrap items-center justify-between gap-3"><strong>{i+1}. {step.name}</strong><div><Button size="icon" variant="ghost" title={t('wf.up')} disabled={i===0} onClick={()=>move(i,-1)}><ChevronRight size={16} style={{transform:'rotate(-90deg)'}}/></Button><Button size="icon" variant="ghost" title={t('wf.down')} disabled={i===edit.steps.length-1} onClick={()=>move(i,1)}><ChevronRight size={16} style={{transform:'rotate(90deg)'}}/></Button><Button size="icon" variant="ghost" title={t('wf.delete')} disabled={edit.steps.length===1} onClick={()=>setEdit({...edit,steps:edit.steps.filter((_,j)=>j!==i)})}><Trash2 size={16}/></Button></div></div><FormField label={t('wf.stepName')}><TextInput  value={step.name} onChange={e=>setStep(i,{name:e.target.value})}/></FormField><FormField label={t('wf.agent')}><SelectInput  value={step.agentId} onChange={e=>setStep(i,{agentId:e.target.value})}><option value="">{t('wf.pickAgent')}</option>{agents.value.map(a=><option value={a.id} key={a.id}>{a.name}{a.model?' · '+a.model:''}</option>)}</SelectInput></FormField><FormField label={t('wf.instructions')}><TextArea className="min-h-28" rows={4} value={step.prompt} onChange={e=>setStep(i,{prompt:e.target.value})} placeholder={t('wf.previous')}/></FormField><FormField label="선택: 단계 완료를 검증할 파일 경로" hint="예: reports/result.md. 현재 파일에서 실제 바이트가 바뀌어야 다음 에이전트 단계로 넘어갑니다. 미입력 단계는 모델 산출물만 기록합니다."><TextInput value={step.verifyPath||''} placeholder="reports/result.md" onChange={e=>setStep(i,{verifyPath:e.target.value})}/></FormField></div>)}<Action variant="secondary" disabled={edit.steps.length>=30} onClick={()=>setEdit({...edit,steps:[...edit.steps,emptyStep(agents.value[0]?.id||'')]})}><Plus size={16}/> {t('wf.addStep')}</Action></div>{error&&<Notice message={error}/>}<div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5"><Action variant="secondary" onClick={()=>setEdit(null)}>{t('wf.cancel')}</Action><Action onClick={save} disabled={saving||!edit.name.trim()||edit.steps.some(s=>!s.prompt.trim()||!s.agentId)}><Check size={16}/> {t('wf.save')}</Action></div></DialogPanel></DialogBackdrop>}
 </div>
}
