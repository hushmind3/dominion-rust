import {useState} from 'react'
import {ArchiveRestore, Database, ShieldCheck} from 'lucide-react'
import {post} from '../../api'
import {Action, Notice, Panel, Subheading} from '../../components/UI'
import {TextInput} from '../../components/Primitives'

type Preview = {
  sourceAgents:number;sourceSessions:number;sourceMessages:number;
  importableAgents:number;importableSessions:number;
  alreadyPresentAgents:number;alreadyPresentSessions:number;
  skippedBoundSessions:number;skippedArchivedSessions:number;note:string;
}
type Applied = {
  importedAgents:number;importedSessions:number;importedMessages:number;
  existingAgents:number;existingSessions:number;
  skippedBoundSessions:number;skippedArchivedSessions:number;
}

/** Operator-controlled main-to-native copy. Never modifies source SQLite. */
export function LegacyImportPanel(){
  const [path,setPath]=useState('')
  const [preview,setPreview]=useState<Preview|null>(null)
  const [applied,setApplied]=useState<Applied|null>(null)
  const [busy,setBusy]=useState(false)
  const [error,setError]=useState('')
  const inspect=async()=>{
    setBusy(true);setError('');setPreview(null);setApplied(null)
    try {setPreview(await post<Preview>('/api/system/legacy-sqlite/preview',{path:path.trim()}))}
    catch(e){setError((e as Error).message)}
    finally{setBusy(false)}
  }
  const importSelected=async()=>{
    if(!preview)return
    setBusy(true);setError('')
    try {
      setApplied(await post<Applied>('/api/system/legacy-sqlite/import',{path:path.trim(),confirm:true}))
      setPreview(null)
    }catch(e){setError((e as Error).message)}
    finally{setBusy(false)}
  }
  return <Panel className="min-w-0 p-5 xl:col-span-2">
    <Subheading title="기존 메인 데이터 가져오기" extra={<Database size={19}/>}/>
    <p className="mt-1 text-xs text-[#7C7469]">
      메인 앱의 dominion.sqlite에서 개인 대화와 에이전트를 읽기 전용으로 복사합니다.
      원본 DB와 이미 존재하는 서브4 데이터는 바꾸지 않습니다.
    </p>
    <label className="mt-4 block text-xs font-medium text-[#5C544A]" htmlFor="legacy-sqlite-path">메인 SQLite 파일의 절대 경로</label>
    <TextInput id="legacy-sqlite-path" className="mt-2 w-full" value={path}
      placeholder="/path/to/runtime/dominion.sqlite"
      onChange={e=>{setPath(e.target.value);setPreview(null);setApplied(null);setError('')}}/>
    <div className="mt-3 flex flex-wrap items-center gap-2">
      <Action variant="secondary" disabled={busy||!path.trim()} onClick={inspect}>
        <Database size={15}/> {busy?'확인 중…':'가져올 항목 미리보기'}
      </Action>
      {preview&&<Action disabled={busy} onClick={importSelected}>
        <ArchiveRestore size={15}/> 선택한 기록 가져오기
      </Action>}
    </div>
    {preview&&<div className="mt-4 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-xs text-[#554D43]" role="status">
      <strong>이관 대상: 에이전트 {preview.importableAgents}개 · 대화 {preview.importableSessions}개</strong>
      <p className="mt-1">원본에서 읽은 기록: 에이전트 {preview.sourceAgents}개, 대화 {preview.sourceSessions}개, 메시지 {preview.sourceMessages}개</p>
      <p className="mt-1">기존 ID 보존: 에이전트 {preview.alreadyPresentAgents}개, 대화 {preview.alreadyPresentSessions}개</p>
      <p className="mt-1">별도 이관 대상: 룸/채널 {preview.skippedBoundSessions}개, 보관 대화 {preview.skippedArchivedSessions}개</p>
      <p className="mt-2">API 키·암호화 비밀·기존 실행 기록은 복사하지 않습니다. 기존 연결은 다시 설정해야 합니다.</p>
    </div>}
    {applied&&<div className="mt-4 flex items-start gap-2 rounded-lg border border-[#DCE8DC] bg-[#F3FAF2] p-3 text-xs text-[#366344]" role="status">
      <ShieldCheck size={18}/>
      <span>복사 완료: 에이전트 {applied.importedAgents}개 · 대화 {applied.importedSessions}개 · 메시지 {applied.importedMessages}개.
        화면을 새로고침하면 가져온 기록이 보입니다. 원본은 그대로 유지됩니다.</span>
    </div>}
    {error&&<div className="mt-3"><Notice message={error}/></div>}
  </Panel>
}
