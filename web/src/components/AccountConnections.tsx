import {useEffect,useState} from 'react'
import {get,post,del} from '../api'
import {useRemote,Panel,Subheading,FormField,Notice} from './UI'
import {Button,TextInput} from './Primitives'

type AccountProvider={id:string;name:string;configured:boolean;connected:boolean;account?:string;callback?:string;connectedAt?:string}
type Credentials={clientId:string;secret:string}
type ConsentAttempt={id:string;wasConnected:boolean;connectedAt?:string}
const emptyCredentials=():Credentials=>({clientId:'',secret:''})

export function AccountConnections(){
 const rows=useRemote<AccountProvider[]>('/api/oauth/providers',[])
 const [editing,setEditing]=useState(''),[drafts,setDrafts]=useState<Record<string,Credentials>>({}),[error,setError]=useState(''),[pending,setPending]=useState<ConsentAttempt|null>(null),[busy,setBusy]=useState(''),[saved,setSaved]=useState('')
 useEffect(()=>{
  if(!pending)return
  let count=0
  const timer=setInterval(()=>{void rows.reload();if(++count>=200){setPending(null);setSaved('인증 완료 여부를 아직 확인하지 못했습니다. 상태를 새로 확인하거나 다시 연결하세요.')}},3000)
  return()=>clearInterval(timer)
 },[pending])
 useEffect(()=>{
  if(!pending)return
  const account=rows.value.find(row=>row.id===pending.id)
  const fresh=account?.connected&&(!pending.wasConnected||Boolean(account.connectedAt&&account.connectedAt!==pending.connectedAt))
  if(fresh){setPending(null);setSaved((account?.name||'계정')+' 연결을 확인했습니다.')}
 },[rows.value,pending])
 useEffect(()=>{const update=()=>void rows.reload();window.addEventListener('focus',update);return()=>window.removeEventListener('focus',update)},[])
 const edit=(id:string,patch:Partial<Credentials>)=>{setDrafts(previous=>({...previous,[id]:{...(previous[id]||emptyCredentials()),...patch}}));setSaved('')}
 const connect=async(row:AccountProvider)=>{
  if(busy||pending)return;setBusy('connect:'+row.id);setError('');setSaved('')
  try{const result=await post<{opened:boolean}>('/api/oauth/'+row.id+'/open',{});if(!result.opened)throw Error('시스템 브라우저를 열 수 없습니다.');setPending({id:row.id,wasConnected:row.connected,connectedAt:row.connectedAt})}
  catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const save=async(row:AccountProvider)=>{
  const draft=drafts[row.id]||emptyCredentials();if(busy||!draft.clientId.trim()||(row.id==='github'&&!draft.secret.trim()))return
  setBusy('save:'+row.id);setError('');setSaved('')
  try{await post('/api/oauth/providers/'+row.id,{clientId:draft.clientId.trim(),clientSecret:draft.secret.trim()});setDrafts(previous=>({...previous,[row.id]:emptyCredentials()}));setEditing('');setSaved(row.name+' 앱 설정을 저장했습니다.');await rows.reload()}
  catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const disconnect=async(row:AccountProvider)=>{
  if(busy)return;setBusy('disconnect:'+row.id);setError('')
  try{await del('/api/oauth/'+row.id);if(pending?.id===row.id)setPending(null);setSaved(row.name+' 연결을 해제했습니다.');await rows.reload()}
  catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 return <><p className="mb-4 text-sm leading-6 text-[#6d6358]">Google 기본 계정과 YouTube 업로드 권한을 각각 연결합니다. YouTube는 Google OAuth 앱 설정을 함께 사용할 수 있습니다. 계정 동의는 시스템 브라우저에서 진행합니다.</p>
  <div className="grid gap-4 md:grid-cols-3">{rows.value.map(row=>{const draft=drafts[row.id]||emptyCredentials();const isPending=pending?.id===row.id;return <Panel key={row.id} className="flex min-w-0 flex-col gap-3 p-4"><Subheading title={row.name}/><p className="text-sm">{row.id==='google'?'이름·이메일·프로필':row.id==='youtube'?'YouTube 채널 확인·영상 업로드':'저장소·계정 접근'}</p><strong className="text-sm">{row.connected?'연결됨 · '+(row.account||'계정'):row.configured?'연결 준비됨':'앱 설정 필요'}</strong><div className="flex flex-wrap gap-2"><Button disabled={!row.configured||Boolean(busy)||Boolean(pending)} onClick={()=>void connect(row)}>{isPending?'브라우저에서 연결 중':row.connected?'다시 연결':row.id==='google'?'Google로 연결':row.id==='youtube'?'YouTube 연결':'GitHub 연결'}</Button>{row.connected&&<Button variant="ghost" disabled={Boolean(busy)} onClick={()=>void disconnect(row)}>연결 해제</Button>}</div>{isPending&&<div className="grid gap-2 text-sm text-[#8F7B5F]"><p>브라우저에서 동의한 뒤 이 화면으로 돌아오세요.</p><Button size="sm" variant="ghost" onClick={()=>{setPending(null);setSaved('연결 확인 대기를 종료했습니다. 브라우저에서 완료한 계정 상태는 다시 확인할 수 있습니다.')}}>연결 확인 대기 끝내기</Button></div>}<button type="button" className="text-left text-sm underline underline-offset-4" aria-expanded={editing===row.id} onClick={()=>{setEditing(editing===row.id?'':row.id);setError('')}}>OAuth 앱 설정</button>{editing===row.id&&<div className="mt-1 grid gap-3"><p className="text-sm leading-6">개인 설치본은 개발자 콘솔에서 OAuth 앱을 한 번 등록해야 합니다.{row.id==='youtube'?' Google 설정이 있다면 새로 입력하지 않고 YouTube 연결 버튼을 사용하세요.':''}</p><FormField label={row.name+' Client ID'}><TextInput value={draft.clientId} disabled={Boolean(busy)} onChange={e=>edit(row.id,{clientId:e.target.value})}/></FormField><FormField label={row.id==='github'?'GitHub Client Secret':row.name+' Client Secret · 데스크톱 앱은 선택'}><TextInput type="password" value={draft.secret} disabled={Boolean(busy)} onChange={e=>edit(row.id,{secret:e.target.value})}/></FormField><p className="break-all text-sm">콜백: <code>{row.callback}</code></p><Button disabled={Boolean(busy)||!draft.clientId.trim()||(row.id==='github'&&!draft.secret.trim())} onClick={()=>void save(row)}>{busy==='save:'+row.id?'저장 중':row.name+' 앱 설정 저장'}</Button></div>}</Panel>})}</div>
  <Notice message={error||rows.error}/>{saved&&<p className="mt-3 text-sm" role="status">{saved}</p>}
 </>
}
