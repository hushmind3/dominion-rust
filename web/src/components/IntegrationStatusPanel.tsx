import {useState} from 'react'
import {Activity,Blocks,CheckCircle2,ChevronDown,ChevronUp,Copy,RefreshCw,Server,ShieldCheck,ShieldOff,TerminalSquare} from 'lucide-react'
import {get} from '../api'
import {Button} from './Primitives'
import {Notice,Panel,useRemote} from './UI'
import {useLocale} from '../i18n'

type NodeStatus={
 id:string;url:string;weight:number;writerConfigured:boolean;
 state:'not_probed'|'ready'|'at_capacity'|'unavailable'|'unreachable_or_unauthorized'|'invalid_protocol'|'timeout';
 active?:number;slots?:number;writerAvailable?:boolean;writerRepositories?:string[]
}
type IntegrationStatus={
 addonsEnabled:boolean;
 bridge:{configured:boolean;providers:string[];writerEnabled:boolean;consultTool:string;writerTool:string};
 remote:{configured:boolean;probed:boolean;nodes:NodeStatus[]}
}
const empty:IntegrationStatus={
 addonsEnabled:false,bridge:{configured:false,providers:[],writerEnabled:false,consultTool:'',writerTool:''},
 remote:{configured:false,probed:false,nodes:[]}
}
export function IntegrationStatusPanel(){
 const ko=useLocale()==='ko'
 const status=useRemote<IntegrationStatus>('/api/v1/integrations/status',empty)
 const [open,setOpen]=useState(true),[checking,setChecking]=useState(false)
 const [probe,setProbe]=useState<IntegrationStatus|null>(null),[error,setError]=useState('')
 const data=probe||status.value
 const inspect=async()=>{
  if(checking)return
  setChecking(true);setError('')
  try{setProbe(await get<IntegrationStatus>('/api/v1/integrations/status?probe=true'))}
  catch(e){setError((e as Error).message)}
  finally{setChecking(false)}
 }
 const nodes=data.remote.nodes
 return <Panel className="mb-5 overflow-hidden">
  <button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)} className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left">
   <span className="flex min-w-0 items-center gap-3"><span className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#F3EEE5]"><Server size={17}/></span>
    <span><strong className="block text-sm">{ko?'외부 에이전트 · 분산 Worker 상태':'External agents & remote Workers'}</strong>
     <small className="block text-[11px] text-[#7C7469]">{ko?'Rust 런타임에 실제 등록된 연결만 표시합니다.':'Only live Rust registry configurations are shown.'}</small></span></span>
   {open?<ChevronUp size={16}/>:<ChevronDown size={16}/>}
  </button>
  {open&&<div className="space-y-4 border-t border-[#E7E0D5] p-4">
   <Notice message={status.error||error}/>
   {!data.addonsEnabled&&<p className="text-xs text-amber-800">{ko?'현재 선택 기능 모듈이 비활성화돼 있습니다. DOMINION_ADDONS_ENABLED를 확인하세요.':'Optional integrations disabled; check DOMINION_ADDONS_ENABLED.'}</p>}
   <div className="grid gap-3 lg:grid-cols-2">
    <div className="rounded-xl border border-[#E7E0D5] bg-white p-4">
     <div className="flex items-center justify-between gap-2"><strong className="flex items-center gap-2 text-xs"><TerminalSquare size={16}/>{ko?'외부 코딩 에이전트':'External coding agents'}</strong>
      <span className={'text-[11px] font-semibold '+(data.bridge.configured?'text-emerald-700':'text-[#9C9388]')}>{data.bridge.configured?(ko?'등록됨':'Configured'):(ko?'미설정':'Not configured')}</span>
     </div>
     <p className="mt-2 text-xs text-[#7C7469]">{ko?'Codex · Claude Code · Qwen Code 등 실제 설치 경로를 Rust가 승인한 경우에만 상담/격리된 코드 작업 도구를 제공합니다.':'Only operator-approved CLI binaries can enter the existing approval-gated Rust tool path.'}</p>
     {data.bridge.providers.length>0?<div className="mt-3 flex flex-wrap gap-1.5">{data.bridge.providers.map(provider=><span key={provider} className="rounded-lg bg-[#F3EEE5] px-2 py-1 text-[11px]">{provider}</span>)}</div>:<p className="mt-2 text-[11px] text-[#9C9388]">{ko?'연결된 외부 CLI가 없습니다.':'No configured external CLI binaries.'}</p>}
     <div className="mt-3 flex items-center gap-2 text-xs text-[#7C7469]">{data.bridge.writerEnabled?<ShieldCheck size={15}/>:<ShieldOff size={15}/>} {data.bridge.writerEnabled?(ko?'격리된 Writer 허용':'Isolated Writer enabled'):(ko?'조회 전용 또는 비활성화':'Advisory only / disabled')}</div>
     <details className="mt-3 text-[11px] text-[#7C7469]"><summary className="cursor-pointer font-semibold">{ko?'연결 설정 방법':'How to configure'}</summary>
      <p className="mt-2">{ko?'DOMINION_AGENT_BRIDGE_ALLOW_UNMETERED=true를 설정하고, 필요한 CLI의 절대 경로를 DOMINION_AGENT_BRIDGE_CODEX / _CLAUDE / _CODEX_APP / _QWEN에 지정한 후 Rust 서버를 재시작하세요. 코드 수정은 DOMINION_AGENT_BRIDGE_ENABLE_WRITER=true가 추가로 필요합니다.':'Set DOMINION_AGENT_BRIDGE_ALLOW_UNMETERED=true and operator-owned absolute executable paths in DOMINION_AGENT_BRIDGE_CODEX / _CLAUDE / _CODEX_APP / _QWEN. Restart Rust. Writer also requires DOMINION_AGENT_BRIDGE_ENABLE_WRITER=true.'}</p>
     </details>
    </div>
    <div className="rounded-xl border border-[#E7E0D5] bg-white p-4">
     <div className="flex flex-wrap items-center justify-between gap-2">
      <strong className="flex items-center gap-2 text-xs"><Blocks size={16}/>{ko?'원격 Rust Worker':'Remote Rust Workers'}</strong>
      <Button size="sm" variant="secondary" disabled={checking||!data.remote.configured} onClick={()=>void inspect()}><RefreshCw size={13}/>{checking?(ko?'연결 검사 중':'Probing'):(ko?'실제 연결 검사':'Probe connections')}</Button>
     </div>
     {!data.remote.configured?<p className="mt-3 text-xs text-[#9C9388]">{ko?'설정된 원격 노드가 없습니다.':'No remote nodes configured.'}</p>:<div className="mt-3 space-y-2">
      {nodes.map(node=><div key={node.id} className="rounded-lg border border-[#EFE9DF] p-3">
       <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-xs">{node.id}</strong>
        <span className={'inline-flex items-center gap-1 text-[11px] '+(node.state==='ready'?'text-emerald-700':node.state==='at_capacity'?'text-amber-700':'text-[#7C7469]')}>
         {node.state==='ready'?<CheckCircle2 size={13}/>:<Activity size={13}/>}
         {node.state==='not_probed'?(ko?'미검사':'Not probed'):node.state==='ready'?(ko?'사용 가능':'Ready'):node.state==='at_capacity'?(ko?'작업 슬롯 가득 참':'At capacity'):node.state==='timeout'?(ko?'연결 시간 초과':'Timed out'):node.state==='invalid_protocol'?(ko?'프로토콜 불일치':'Protocol mismatch'):node.state==='unreachable_or_unauthorized'?(ko?'접속 또는 인증 실패':'Unreachable / unauthorized'):node.state}
        </span></div>
       <p className="mt-1 break-all text-[11px] text-[#7C7469]">{node.url}</p>
       {node.active!=null&&node.slots!=null&&<p className="mt-1 text-[11px] text-[#7C7469]">{ko?'작업 중':'Active'} {node.active} / {ko?'전체 슬롯':'Slots'} {node.slots} · {ko?'가중치':'Weight'} {node.weight}</p>}
       <p className="mt-1 text-[11px] text-[#7C7469]">{ko?'쓰기 기능':'Writer'}: {node.writerAvailable?(ko?'원격 응답 확인됨':'Verified by worker'):node.writerConfigured?(ko?'토큰 설정됨 · 미검사':'Token configured · not verified'):(ko?'비활성':'Disabled')}</p>
       {Boolean(node.writerRepositories?.length)&&<p className="mt-1 break-all text-[11px] text-[#7C7469]">{ko?'허용 저장소':'Allowed repositories'}: {node.writerRepositories?.join(', ')}</p>}
      </div>)}
     </div>}
     <details className="mt-3 text-[11px] text-[#7C7469]"><summary className="cursor-pointer font-semibold">{ko?'노드 등록 안내':'Node registry setup'}</summary>
      <p className="mt-2">{ko?'DOMINION_REMOTE_WORKERS_JSON에 노드 id·url·token·writerToken(선택)을 설정하고 Rust를 재시작합니다. 원격 노드는 별도의 DOMINION_REMOTE_WORKER_ACCEPT_TOKEN과 실제 모델 연결이 필요합니다. 전용 인증 키는 이 화면에 반환되지 않습니다.':'Configure node id/url/token/optional writerToken in DOMINION_REMOTE_WORKERS_JSON, then restart Rust. The remote machine requires DOMINION_REMOTE_WORKER_ACCEPT_TOKEN and a model provider. No capability secrets are returned to this page.'}</p>
     </details>
    </div>
   </div>
   <p className="text-[11px] text-[#8A8176]">{ko?'상태 조회와 연결 검사는 도구를 실행하거나 유료 모델을 호출하지 않습니다. 승인과 변경 기록은 기존 Rust Harness에서 관리합니다.':'Discovery never dispatches a model or tool. Native Run approvals remain authoritative.'}</p>
  </div>}
 </Panel>
}
