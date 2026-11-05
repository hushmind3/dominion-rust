import {useState,useEffect,useRef} from 'react'
import {get,post} from '../api'
import {Panel,Subheading,Notice} from './UI'
import {Button,TextArea,TextInput} from './Primitives'

type Settings={instructions:string;enabled:boolean}
export function AssistantSettings(){
 const [text,setText]=useState(''),[enabled,setEnabled]=useState(true),[dirty,setDirty]=useState(false),[message,setMessage]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[loading,setLoading]=useState(true)
 const version=useRef(0),dirtyRef=useRef(false)
 dirtyRef.current=dirty
 useEffect(()=>{
  let mounted=true,timer:ReturnType<typeof setTimeout>|undefined
  const load=async()=>{const current=version.current;try{const saved=await get<Settings>('/api/settings/assistant');if(mounted&&current===version.current&&!dirtyRef.current){setText(saved.instructions||'');setEnabled(saved.enabled!==false);setError('')}}catch(e){if(mounted)setError((e as Error).message)}finally{if(mounted)setLoading(false)}}
  void load();const update=()=>{clearTimeout(timer);timer=setTimeout(()=>void load(),200)}
  window.addEventListener('dominion-refresh',update);return()=>{mounted=false;clearTimeout(timer);window.removeEventListener('dominion-refresh',update)}
 },[])
 const changed=()=>{dirtyRef.current=true;setDirty(true);setMessage('')}
 const save=async()=>{
  if(busy||!dirty)return;setBusy(true);setError('')
  try{const saved=await post<Settings>('/api/settings/assistant',{instructions:text,enabled});version.current+=1;setText(saved.instructions);setEnabled(saved.enabled);dirtyRef.current=false;setDirty(false);setMessage('공통 지침을 저장했습니다. 다음 응답부터 적용합니다.')}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <Panel className="min-w-0 p-5 xl:col-span-2"><Subheading title="모든 에이전트의 공통 지침"/><p className="mb-3 text-sm leading-6 text-[#6d6358]">개별 페르소나와 함께 대화·회의·메신저 응답에 적용합니다. 기억 데이터와 별도로 저장되는 주인의 지침입니다.</p><TextArea aria-label="모든 에이전트의 공통 지침" rows={5} maxLength={16000} value={text} disabled={busy||loading} onChange={e=>{changed();setText(e.target.value)}} placeholder="예: 한국어로 답하고, 완료 보고에는 확인한 증거와 남은 일을 구분한다."/><div className="mt-3 flex flex-wrap items-center justify-between gap-3"><label className="flex items-center gap-2 text-sm"><TextInput type="checkbox" checked={enabled} disabled={busy||loading} onChange={e=>{changed();setEnabled(e.target.checked)}}/>공통 지침 적용</label><span className="text-sm text-[#9C8A70]">{dirty?'저장하지 않은 변경 · ':''}{Array.from(text).length.toLocaleString()}/16,000자</span><Button disabled={busy||loading||!dirty} onClick={()=>void save()}>{busy?'저장 중':'지침 저장'}</Button></div>{message&&<p className="mt-2 text-sm" role="status">{message}</p>}<Notice message={error}/></Panel>
}
