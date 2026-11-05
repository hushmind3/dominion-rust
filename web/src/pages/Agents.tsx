import { DialogBackdrop, DialogPanel } from "../components/Dialog";
import { StatusPill } from "../components/StatusPill";
import {
  Button,
  TextInput,
  TextArea,
  SelectInput,
} from "../components/Primitives";
import { useEffect, useState } from "react";
import {
  Activity,
  ArrowRight,
  Bot,
  BrainCircuit,
  Check,
  CirclePlus,
  Clock3,
  Edit3,
  FolderInput,
  LoaderCircle,
  MessageSquare,
  Play,
  Plus,
  Save,
  Settings2,
  Sparkles,
  Trash2,
  UsersRound,
  X,
} from "lucide-react";
import { Agent, Provider, Room, del, get, post } from "../api";
import {
  Action,
  Empty,
  FormField,
  Headline,
  Notice,
  Panel,
  Subheading,
  refresh,
  useRemote,
} from "../components/UI";
const accents = [
  "#5979e8",
  "#9067d9",
  "#15aaa1",
  "#f2934e",
  "#e86e86",
  "#6d8b9f",
];
export function AgentsPage() {
  const agents = useRemote<Agent[]>("/api/agents", []),
    providers = useRemote<Provider[]>("/api/providers", []);
  const [edit, setEdit] = useState<Partial<Agent> | null>(null),
    [models, setModels] = useState<string[]>([]),
    [error, setError] = useState("");
  const [showPersonaImport,setShowPersonaImport]=useState(false);
  const [bulk, setBulk] = useState(false),
    [selected, setSelected] = useState<string[]>([]),
    [bulkProvider, setBulkProvider] = useState(""),
    [bulkModel, setBulkModel] = useState(""),
    [bulkModels, setBulkModels] = useState<string[]>([]),
    [bulkBusy, setBulkBusy] = useState(false),
    [bulkNotice, setBulkNotice] = useState("");
  useEffect(() => {
    if (!edit?.providerId) return;
    get<any[]>("/api/providers/" + edit.providerId + "/models")
      .then((x) => setModels(x.map((v) => v.id)))
      .catch(() => setModels([]));
  }, [edit?.providerId]);
  useEffect(() => {
    if (!bulkProvider) {
      setBulkModels([]);
      return;
    }
    get<any[]>("/api/providers/" + bulkProvider + "/models")
      .then((x) => setBulkModels(x.map((v) => v.id)))
      .catch(() => setBulkModels([]));
  }, [bulkProvider]);
  const save = async () => {
    if (!edit?.name) return;
    setError("");
    try {
      await post("/api/agents", edit);
      setEdit(null);
      await agents.reload();
      refresh();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const applyBulk = async () => {
    if (!selected.length || !bulkProvider || !bulkModel.trim() || bulkBusy)
      return;
    setBulkBusy(true);
    setError("");
    try {
      const r = await post<{ count: number }>("/api/agents/bulk-model", {
        agentIds: selected,
        providerId: bulkProvider,
        model: bulkModel.trim(),
      });
      setBulkNotice(r.count + "개 에이전트의 모델을 변경했습니다.");
      setBulk(false);
      await agents.reload();
      refresh();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBulkBusy(false);
    }
  };
  return (
    <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
      <Headline
        eyebrow="AGENT STUDIO"
        title="나만의 AI 팀."
        description="에이전트를 필요한 만큼 만들고, 모델과 역할을 자유롭게 배치하세요."
        actions={
          <>
            <Action variant="secondary" onClick={()=>setShowPersonaImport(true)}><FolderInput size={16}/> 기존 페르소나 가져오기</Action>
            <Action
              variant="secondary"
              onClick={() => {
                setBulk((x) => !x);
                setSelected([]);
                setBulkNotice("");
              }}
            >
              <Settings2 size={16} /> 선택 모델 일괄 변경
            </Action>
            <Action
              onClick={() =>
                setEdit({
                  name: "새 에이전트",
                  description: "",
                  aliases: [],
                  systemPrompt:
                    "당신은 숙련된 전문가입니다. 실제 행동과 정확한 보고를 우선합니다.",
                  providerId: "",
                  model: "",
                  color: accents[agents.value.length % accents.length],
                })
              }
            >
              <Plus size={17} /> 에이전트 만들기
            </Action>
          </>
        }
      />
      <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5">
        <div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]">
          <BrainCircuit size={22} />
        </div>
        <div>
          <strong>역할도, 모델도 자유롭게</strong>
          <p>
            각 에이전트는 독립된 페르소나와 모델 설정을 가지고
            대화·회의·자동화에 참여합니다.
          </p>
        </div>
      </div>
      {bulkNotice && (
        <div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700">
          <Check size={16} />
          {bulkNotice}
        </div>
      )}
      {bulk && (
        <Panel className="mb-6 grid gap-4 p-5">
          <Subheading
            title="선택한 에이전트의 모델 변경"
            extra={
              <span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">
                {selected.length} / {agents.value.length} SELECTED
              </span>
            }
          />
          <p>
            실제 등록된 에이전트만 대상으로 합니다. 각 프로필의 모델 연결만
            변경하며 이미 실행 중인 응답은 중단하지 않습니다.
          </p>
          <div className="grid grid-cols-1 items-end gap-3 md:grid-cols-3">
            <label>
              <TextInput
                type="checkbox"
                checked={
                  selected.length === agents.value.length &&
                  agents.value.length > 0
                }
                onChange={(e) =>
                  setSelected(
                    e.target.checked ? agents.value.map((a) => a.id) : [],
                  )
                }
              />{" "}
              전체 선택
            </label>
            <FormField label="모델 제공자">
              <SelectInput
                value={bulkProvider}
                onChange={(e) => {
                  setBulkProvider(e.target.value);
                  setBulkModel("");
                }}
              >
                <option value="">선택</option>
                {providers.value.map((p) => (
                  <option value={p.id} key={p.id}>
                    {p.name}
                  </option>
                ))}
              </SelectInput>
            </FormField>
            <FormField label="모델 ID">
              <TextInput
                list="bulk-model-options"
                value={bulkModel}
                onChange={(e) => setBulkModel(e.target.value)}
                placeholder="모델 ID 입력 또는 선택"
              />
              <datalist id="bulk-model-options">
                {bulkModels.map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            </FormField>
            <Action
              disabled={
                !selected.length ||
                !bulkProvider ||
                !bulkModel.trim() ||
                bulkBusy
              }
              onClick={applyBulk}
            >
              <Check size={16} />
              {bulkBusy ? "적용 중" : "선택한 " + selected.length + "개 적용"}
            </Action>
          </div>
        </Panel>
      )}
      <div className="grid grid-cols-1 items-stretch gap-4 md:grid-cols-2 xl:grid-cols-3">
        {agents.value.map((a, i) => (
          <Panel className="flex min-w-0 flex-col gap-3 p-5" key={a.id}>
            <div className="flex items-start justify-between gap-3">
              <div
                className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-xl bg-[#C96442] text-white"
                style={{ background: a.color || accents[i % accents.length] }}
              >
                {(a.avatar || a.avatarUrl) ? (
                  <img src={String(a.avatar || a.avatarUrl)} alt="" className="size-12 object-cover" />
                ) : (
                  <Bot size={26} />
                )}
              </div>
              <div className="flex flex-wrap items-center gap-1">
                {bulk && (
                  <label className="inline-flex items-center gap-2">
                    <TextInput
                      type="checkbox"
                      aria-label={a.name + " 선택"}
                      checked={selected.includes(a.id)}
                      onChange={(e) =>
                        setSelected((prev) =>
                          e.target.checked
                            ? Array.from(new Set([...prev, a.id]))
                            : prev.filter((id) => id !== a.id),
                        )
                      }
                    />
                  </label>
                )}
                <Button
                  size="icon"
                  variant="ghost"
                  onClick={() => setEdit({ ...a })}
                >
                  <Edit3 size={16} />
                </Button>
                {a.id !== "general" && (
                  <Button
                    size="icon"
                    variant="ghost"
                    onClick={async () => {
                      if (confirm(a.name + " 에이전트를 삭제하시겠습니까?")) {
                        await del("/api/agents/" + a.id);
                        agents.reload();
                      }
                    }}
                  >
                    <Trash2 size={16} />
                  </Button>
                )}
              </div>
            </div>
            <h3>{a.name}</h3>
            {a.role&&<span className="text-[11px] font-semibold text-[#9B8064]">{String(a.role)}</span>}
            <p>{a.description || "사용자가 설정하는 범용 에이전트"}</p>
            <div className="my-2 h-px bg-[#F3EEE5]" />
            <div className="mt-auto flex flex-col gap-1 border-t border-[#EFE9DF] pt-3 text-xs text-[#7C7469]">
              <span>
                <Sparkles size={15} />{" "}
                {providers.value.find((p) => p.id === a.providerId)?.name ||
                  "모델 미설정"}
              </span>
              <span>{a.model || "선택 필요"}</span>
            </div>
          </Panel>
        ))}
      </div>
      {error && <Notice message={error} />}
      {showPersonaImport&&<PersonaImportDialog agents={agents.value} onClose={()=>setShowPersonaImport(false)} onImported={async(count)=>{await agents.reload();refresh();setBulkNotice(count+'개 페르소나와 아이콘을 가져왔습니다.')}}/>}
      {edit && (
        <DialogBackdrop onDismiss={() => setEdit(null)}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
              <div>
                <small>AGENT DESIGNER</small>
                <h2>{edit.id ? "에이전트 편집" : "에이전트 만들기"}</h2>
              </div>
              <Button size="icon" variant="ghost" onClick={() => setEdit(null)}>
                <X size={19} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <FormField label="에이전트 이름">
                  <TextInput
                    value={edit.name || ""}
                    onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                  />
                </FormField>
                <FormField label="테마 색상">
                  <TextInput
                    type="color"
                    className="h-10 w-full cursor-pointer rounded-xl border border-[#E7E0D5] bg-white p-1"
                    value={edit.color || "#5979e8"}
                    onChange={(e) =>
                      setEdit({ ...edit, color: e.target.value })
                    }
                  />
                </FormField>
              </div>
              <FormField label="설명">
                <TextInput
                  value={edit.description || ""}
                  onChange={(e) =>
                    setEdit({ ...edit, description: e.target.value })
                  }
                  placeholder="담당 업무와 전문성"
                />
              </FormField>
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <FormField label="모델 제공자">
                  <SelectInput
                    value={edit.providerId || ""}
                    onChange={(e) =>
                      setEdit({
                        ...edit,
                        providerId: e.target.value,
                        model: "",
                      })
                    }
                  >
                    <option value="">선택</option>
                    {providers.value.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </SelectInput>
                </FormField>
                <FormField label="모델">
                  <SelectInput
                    value={edit.model || ""}
                    onChange={(e) =>
                      setEdit({ ...edit, model: e.target.value })
                    }
                  >
                    <option value="">모델 선택</option>
                    {models.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                    {edit.model && !models.includes(edit.model) && (
                      <option value={edit.model}>{edit.model}</option>
                    )}
                  </SelectInput>
                </FormField>
              </div>
              <FormField
                label="호출 이름 · 별칭 (쉼표 구분)"
                hint="메신저 회의실에서 이 이름으로 특정 에이전트를 지정할 수 있습니다."
              >
                <TextInput
                  value={(edit.aliases || []).join(", ")}
                  onChange={(e) =>
                    setEdit({ ...edit, aliases: e.target.value.split(",") })
                  }
                  placeholder="별칭1, 별칭2"
                />
              </FormField>
              <FormField label="프로필 이미지" hint="PNG/JPEG/WebP · 1MB 이하 · 저장하면 적용됩니다">
                <div className="flex items-center gap-3">
                  <div className="grid size-14 shrink-0 place-items-center overflow-hidden rounded-xl border border-[#E7E0D5] bg-[#FAF7F1]">
                    {(edit.avatar || edit.avatarUrl) ? (
                      <img src={String(edit.avatar || edit.avatarUrl)} alt="" className="size-14 object-cover" />
                    ) : (
                      <Bot size={22} className="text-[#C96442]" />
                    )}
                  </div>
                  <label className="cursor-pointer rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
                    이미지 선택
                    <input
                      type="file"
                      className="hidden"
                      accept="image/png,image/jpeg,image/webp"
                      onChange={(e) => {
                        const file = e.target.files?.[0];
                        if (!file) return;
                        if(!['image/png','image/jpeg','image/webp'].includes(file.type)){setError('PNG, JPEG, WebP 이미지를 선택하세요.');return;}
                        if(file.size>1024*1024){setError('프로필 이미지는 1MB 이하로 선택하세요.');return;}
                        const reader = new FileReader();
                        reader.onload = () => {
                          const image = String(reader.result || "");
                          setEdit({ ...edit, avatar: image });
                        };
                        reader.readAsDataURL(file);
                      }}
                    />
                  </label>
                  {edit.avatar && (
                    <Button
                      variant="ghost"
                      className="!px-2 !text-[11px] text-[#A94E30]"
                      onClick={() => setEdit({ ...edit, avatar: "" })}
                    >
                      제거
                    </Button>
                  )}
                </div>
              </FormField>
              <FormField label="시스템 프롬프트">
                <TextArea
                  className="min-h-28"
                  rows={6}
                  value={edit.systemPrompt || ""}
                  onChange={(e) =>
                    setEdit({ ...edit, systemPrompt: e.target.value })
                  }
                />
              </FormField>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={() => setEdit(null)}>
                취소
              </Action>
              <Action onClick={save}>
                <Save size={16} /> 저장
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
    </div>
  );
}


type PersonaProfile={sourceProfile:string;id:string;name:string;description:string;role:string;hasAvatar:boolean;personaPreview:string;exists:boolean;available:boolean;error?:string}
function PersonaImportDialog({agents,onClose,onImported}:{agents:Agent[];onClose:()=>void;onImported:(count:number)=>Promise<void>}){
 const [profiles,setProfiles]=useState<PersonaProfile[]>([]),[selected,setSelected]=useState<string[]>([]),[loading,setLoading]=useState(true),[busy,setBusy]=useState(false),[error,setError]=useState(''),[refreshIdentity,setRefreshIdentity]=useState(false)
 useEffect(()=>{let mounted=true;get<{profiles:PersonaProfile[]}>('/api/personas/import/preview').then(result=>{if(!mounted)return;setProfiles(result.profiles);setSelected([])}).catch(e=>{if(mounted)setError(e.message)}).finally(()=>{if(mounted)setLoading(false)});return()=>{mounted=false}},[])
 const available=profiles.filter(profile=>profile.available&&(!profile.exists||refreshIdentity))
 const apply=async()=>{
  if(busy||!selected.length)return;setBusy(true);setError('')
  try{const result=await post<{imported:number;skipped:number;errors:{sourceProfile:string;error:string}[]}>('/api/personas/import',{apply:true,profileIds:selected,...(refreshIdentity?{refreshIdentity:true}:{})});await onImported(result.imported);if(result.errors.length)setError(result.errors.map(item=>item.sourceProfile+': '+item.error).join(' · '));else onClose()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <DialogBackdrop onDismiss={()=>{if(!busy)onClose()}}><DialogPanel size="wide" aria-label="기존 페르소나 가져오기"><div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small className="text-[10px] tracking-widest text-[#A2937F]">MY LOCAL PERSONAS</small><h2 className="mt-1 font-bold">내 페르소나와 아이콘 가져오기</h2></div><Button size="icon" variant="ghost" aria-label="닫기" disabled={busy} onClick={onClose}><X size={18}/></Button></div><div className="grid gap-4 p-5"><p className="text-[12px] text-[#8F7D65]">로컬에 보관한 내 인격 지침과 아이콘을 확인하고, DOMINION 에이전트로 가져옵니다. 모델 설정은 에이전트 화면에서 선택할 수 있습니다.</p>{loading?<div className="flex items-center justify-center gap-2 py-10 text-[#A2937F]"><LoaderCircle size={17} className="animate-spin"/> 페르소나를 확인하는 중</div>:<><div className="flex items-center justify-between text-[11px] text-[#9A866A]"><span>발견 {profiles.length}명 · 선택 {selected.length}명</span><button className="underline underline-offset-2" disabled={busy||!available.length} onClick={()=>setSelected(selected.length===available.length?[]:available.map(profile=>profile.sourceProfile))}>{selected.length===available.length?'선택 해제':'가능한 참가자 모두 선택'}</button></div><div className="grid max-h-[48vh] gap-3 overflow-auto sm:grid-cols-2">{profiles.map((profile,index)=>{const agent=agents.find(item=>item.id===profile.id);const enabled=profile.available&&(!profile.exists||refreshIdentity);const checked=selected.includes(profile.sourceProfile);return <div className={'rounded-xl border p-3 '+(checked?'border-[#D1B894] bg-[#FBF4E8]':'border-[#E7E0D5] bg-[#FFFCF7]')} key={profile.sourceProfile}><label className="flex cursor-pointer items-center gap-3"><input type="checkbox" className="accent-[#AA8B63]" checked={checked} disabled={busy||!enabled} aria-label={profile.name+' 선택'} onChange={event=>{const next=event.target.checked;setSelected(previous=>next?Array.from(new Set([...previous,profile.sourceProfile])):previous.filter(id=>id!==profile.sourceProfile))}}/><span className="grid size-10 shrink-0 place-items-center overflow-hidden rounded-xl text-[13px] font-semibold text-white" style={{background:agent?.color||accents[index%accents.length]}}>{(agent?.avatar||agent?.avatarUrl)?<img src={String(agent.avatar||agent.avatarUrl)} alt="" className="size-10 object-cover"/>:profile.name.slice(0,1)}</span><span className="min-w-0 flex-1"><strong className="block text-[12px] text-[#776249]">{profile.name}</strong><small className="block text-[10px] text-[#AC987A]">{profile.role||profile.description}</small></span>{profile.exists&&<span className="rounded bg-[#EDE8DE] px-1.5 py-0.5 text-[9px] text-[#9D8D74]">가져옴</span>}</label><div className="mt-3 flex gap-2 text-[9px] text-[#B2A087]"><span>{profile.hasAvatar?'아이콘 포함':'기본 아이콘'}</span><span>·</span><span>내 페르소나</span></div>{profile.personaPreview&&<details className="mt-2 text-[10px] text-[#9F896B]"><summary className="cursor-pointer">인격 지침 미리보기</summary><pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-[#F4EDE1] p-3 font-sans text-[10px] leading-6 text-[#A18C6E]">{profile.personaPreview}</pre></details>}{profile.error&&<p className="mt-2 text-[10px] text-rose-600">{profile.error}</p>}</div>})}</div>{!profiles.length&&<p className="py-7 text-center text-[12px] text-[#A2937F]">가져올 수 있는 로컬 페르소나가 없습니다.</p>}<label className="flex items-start gap-2 text-[11px] text-[#9A866A]"><input type="checkbox" className="mt-1 accent-[#AA8B63]" checked={refreshIdentity} disabled={busy} onChange={e=>{setRefreshIdentity(e.target.checked);if(!e.target.checked)setSelected(previous=>previous.filter(id=>!profiles.find(profile=>profile.sourceProfile===id)?.exists))}}/><span>기존 페르소나·아이콘도 원본에서 다시 읽기<small className="mt-1 block text-[10px] text-[#B3A187]">기존 에이전트의 모델과 제공자 설정은 유지됩니다.</small></span></label></>}<Notice message={error}/></div><div className="flex items-center justify-end gap-2 border-t border-[#EFE9DF] p-5"><Button variant="ghost" disabled={busy} onClick={onClose}>닫기</Button><Button variant="primary" disabled={loading||busy||!selected.length} onClick={()=>void apply()}>{busy?<LoaderCircle size={15} className="animate-spin"/>:<FolderInput size={15}/>} 선택한 {selected.length}명 가져오기</Button></div></DialogPanel></DialogBackdrop>
}
