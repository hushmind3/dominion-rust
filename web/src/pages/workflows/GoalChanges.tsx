import {useState} from 'react'
import {FileDiff,RefreshCw,RotateCcw,Trash2} from 'lucide-react'
import {post} from '../../api'
import {Button} from '../../components/Primitives'
import {Notice,useRemote} from '../../components/UI'

type Change={path:string;diff:string;currentSha256:string;canUndo:boolean;undoReason?:string}
type Preview={files?:Change[];sameHead?:boolean;baselineClean?:boolean;notice?:string;
 isolated?:boolean;status?:string;diff?:string;stagedDiff?:string;
 untracked?:string;branch?:string;baseCommit?:string;truncated?:boolean}

/** Only verified tool-owned paths are displayed, never a blind git reset. */
export function GoalChanges({goalId,finished,isolated=false,discarded=false}:{
 goalId:string;finished:boolean;isolated?:boolean;discarded?:boolean
}){
 const preview=useRemote<Preview>('/api/goals/'+encodeURIComponent(goalId)+'/changes',{})
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[info,setInfo]=useState('')
 const [justDiscarded,setJustDiscarded]=useState(false)
 const discard=async()=>{
  if(!finished||busy||discarded||justDiscarded)return
  if(!window.confirm('이 목표의 격리 작업공간을 폐기하시겠습니까?\n\n격리 공간의 미반영 코드와 추적되지 않은 파일이 삭제됩니다. 원본 체크아웃에는 적용하지 않습니다.'))return
  setBusy(true);setError('');setInfo('')
  try{
   await post('/api/goals/'+encodeURIComponent(goalId)+'/worktree/discard',{})
   setJustDiscarded(true)
   setInfo('격리 작업공간을 폐기했습니다. 원본 저장소는 수정하지 않았습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const undo=async(file:Change)=>{
  if(!finished||!file.canUndo||busy)return
  if(!window.confirm('이 파일의 현재 내용을 작업 시작 커밋 상태로 복원하시겠습니까?\n'+file.path+'\n파일 SHA-256이 일치하는 경우에만 실행합니다.'))return
  setBusy(true);setError('');setInfo('')
  try{
   await post('/api/goals/'+encodeURIComponent(goalId)+'/undo',{
    path:file.path,expectedSha256:file.currentSha256,
   })
   setInfo('Git 원본과 대조하여 한 파일을 복원했습니다. 변경 내역을 갱신했습니다.')
   await preview.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <div className="rounded-xl border border-[#E7E0D5] p-3">
  <div className="flex flex-wrap items-center justify-between gap-2">
   <div className="flex items-center gap-2"><FileDiff size={16}/><strong className="text-xs">실제 코드 변경 사항</strong></div>
   <Button size="sm" variant="ghost" onClick={()=>void preview.reload()}><RefreshCw size={13}/> 새로고침</Button>
  </div>
  {preview.error&&<Notice message={preview.error}/>}
  {error&&<Notice message={error}/>}
  {info&&<p className="mt-2 text-xs text-emerald-700">{info}</p>}
  {isolated&&<>
   {(discarded||justDiscarded)?<p className="mt-3 text-xs text-[#7C7469]">격리 작업공간이 폐기됐습니다. 별도 작업 기록과 기준 커밋은 유지됩니다.</p>:<>
    <p className="mt-2 text-[11px] text-[#7C7469]">원본 저장소를 보호하는 별도 Git worktree입니다. 운영체제 수준의 셸 격리는 아닙니다.</p>
    {preview.value.branch&&<p className="mt-2 break-all text-[11px]">작업 브랜치: <strong>{preview.value.branch}</strong> · 시작 커밋 {preview.value.baseCommit?.slice(0,12)}</p>}
    {preview.value.status&&<pre className="mt-2 max-h-28 overflow-auto whitespace-pre-wrap rounded-lg bg-[#FAF7F1] p-2 text-[11px]">{preview.value.status}</pre>}
    {(preview.value.diff||preview.value.stagedDiff)&&<details className="mt-2" open>
     <summary className="cursor-pointer text-xs font-semibold">코드 변경 비교</summary>
     <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#24211D] p-3 text-[11px] text-[#F8F3E8]">{preview.value.diff||preview.value.stagedDiff}</pre>
    </details>}
    {preview.value.untracked&&<p className="mt-2 whitespace-pre-wrap break-all text-[11px]">새 파일(내용 미노출): {preview.value.untracked}</p>}
    {preview.value.truncated&&<p className="mt-2 text-xs text-amber-700">변경 내용이 길어 일부만 표시됩니다. 원본 파일을 열어 검토하세요.</p>}
    <div className="mt-3 flex justify-end"><Button size="sm" variant="danger"
     disabled={busy||!finished||!preview.value.branch}
     onClick={()=>void discard()}><Trash2 size={13}/> 격리 작업공간 폐기</Button></div>
   </>}
  </>}
  {!isolated&&<>
  {!preview.value.baselineClean&&<p className="mt-2 text-[11px] text-amber-700">작업 시작 전 변경 사항이 있거나 Git 기준점을 만들 수 없어 자동 복원이 제한됩니다.</p>}
  {preview.value.sameHead===false&&<p className="mt-2 text-[11px] text-amber-700">작업 시작 후 Git HEAD가 이동했습니다. 자동 되돌리기가 제한됩니다.</p>}
  {(preview.value.files||[]).map(file=><details key={file.path} className="mt-3 border-t border-[#EFE9DF] pt-3">
   <summary className="cursor-pointer break-all text-xs font-semibold">{file.path}</summary>
   {file.diff?<pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#24211D] p-3 text-[11px] leading-5 text-[#F8F3E8]">{file.diff}</pre>:
    <p className="mt-2 text-[11px] text-[#8A8176]">현재 Git HEAD와 비교해 표시할 추적 파일 diff가 없습니다.</p>}
   <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
    <small className="break-all text-[10px] text-[#8A8176]">SHA-256: {file.currentSha256||'미지원 파일'}</small>
    {file.canUndo&&finished?
     <Button size="sm" variant="secondary" disabled={busy} onClick={()=>void undo(file)}><RotateCcw size={13}/> 이 파일 되돌리기</Button>:
     <small className="text-[10px] text-amber-700">자동 되돌리기 제한: {file.undoReason||(!finished?'목표 진행 중':'검증 불가')}</small>}
   </div>
  </details>)}
  {!preview.value.files?.length&&!preview.loading&&<p className="mt-3 text-[11px] text-[#8A8176]">현재까지 성공한 수정 도구가 기록한 파일이 없습니다.</p>}
  <p className="mt-3 text-[10px] leading-5 text-[#8A8176]">Git 저장소의 수정 파일을 성공 도구 기록과 대조합니다. 직접 실행한 외부 명령의 변경 사항과 동시 작업은 별도 검토가 필요합니다.</p>
  </>}
 </div>
}
