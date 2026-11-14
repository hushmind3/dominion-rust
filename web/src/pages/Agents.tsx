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
import { ModelPicker } from "../components/ModelPicker";
import { ReasoningSelect } from "../components/ReasoningSelect";
import { useProviderModels } from "../hooks/useProviderModels";
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
import { t, useLocale } from "../i18n";
const accents = [
  "#5979e8",
  "#9067d9",
  "#15aaa1",
  "#f2934e",
  "#e86e86",
  "#6d8b9f",
];
export function AgentsPage() {
  useLocale();
  const agents = useRemote<Agent[]>("/api/agents", []),
    providers = useRemote<Provider[]>("/api/providers", []),
    skills = useRemote<{id:string;name:string;enabled:boolean;status?:string}[]>("/api/skills", []),
    jobs = useRemote<{kind:string;status:string;input?:{definition?:{steps?:{agentId?:string}[]}};result?:{deliverableId?:string};queuedAt?:string}[]>("/api/jobs", []);
  const [edit, setEdit] = useState<Partial<Agent> | null>(null),
    [error, setError] = useState(""), [saving, setSaving] = useState(false);
  const [showPersonaImport,setShowPersonaImport]=useState(false);
  const [bulk, setBulk] = useState(false),
    [selected, setSelected] = useState<string[]>([]),
    [bulkProvider, setBulkProvider] = useState(""),
    [bulkModel, setBulkModel] = useState(""),
    [bulkEffort, setBulkEffort] = useState(""),
    [bulkBusy, setBulkBusy] = useState(false),
    [bulkNotice, setBulkNotice] = useState("");
  const enabledProviders = providers.value.filter(p => p.enabled !== false);
  const editProvider = enabledProviders.find(p => p.id === edit?.providerId);
  const editModels = useProviderModels(edit?.providerId || "");
  const bulkModels = useProviderModels(bulk ? bulkProvider : "");
  const closeEditor = () => { if (!saving) { setEdit(null); setError(""); } };
  const save = async () => {
    if (!edit?.name?.trim() || saving) return;
    const model = edit.model?.trim() || "";
    if (edit.providerId && (!editProvider || (!model && !editProvider.model))) {
      setError(t('agents.modelHint'));
      return;
    }
    setSaving(true);
    setError("");
    try {
      await post("/api/agents", {
        ...edit,
        name: edit.name.trim(),
        aliases: edit.aliases?.map(alias => alias.trim()).filter(Boolean),
        model: edit.providerId ? model : "",
      });
      setEdit(null);
      await agents.reload();
      refresh();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
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
        reasoningEffort: bulkEffort,
      });
      setBulkNotice(t('agents.bulkDone', { count: r.count }));
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
        eyebrow={t('ui.eyebrow.agents')}
        title={t('agents.title')}
        description={t('agents.description')}
        actions={
          <>
            <Action variant="secondary" onClick={()=>setShowPersonaImport(true)}><FolderInput size={16}/> {t('agents.import')}</Action>
            <Action
              variant="secondary"
              onClick={() => {
                setBulk((x) => !x);
                setSelected([]);
                setError("");
                setBulkNotice("");
              }}
            >
              <Settings2 size={16} /> {t('agents.bulk')}
            </Action>
            <Action
              onClick={() => {
                setError("");
                setEdit({
                  name: t('agents.new'),
                  description: "",
                  aliases: [],
                  systemPrompt:
                    t('agents.defaultPrompt'),
                  providerId: "",
                  model: "",
                  color: accents[agents.value.length % accents.length],
                });
              }}
            >
              <Plus size={17} /> {t('agents.create')}
            </Action>
          </>
        }
      />
      <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5">
        <div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]">
          <BrainCircuit size={22} />
        </div>
        <div>
          <strong>{t('agents.modelLater')}</strong>
          <p>
            {t('agents.modelLater.desc')}
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
            title={t('agents.bulk.title')}
            extra={
              <span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">
                {t('agents.selectedCount', { total: agents.value.length, selected: selected.length })}
              </span>
            }
          />
          <p>
            {t('agents.bulk.desc')}
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
              {t('agents.selectAll')}
            </label>
            <FormField label={t('agents.api')}>
              <SelectInput
                value={bulkProvider}
                onChange={(e) => {
                  setBulkProvider(e.target.value);
                  setBulkModel("");
                  setBulkEffort("");
                }}
              >
                <option value="">{t('agents.select')}</option>
                {enabledProviders.map((p) => (
                  <option value={p.id} key={p.id}>
                    {p.name}
                  </option>
                ))}
              </SelectInput>
            </FormField>
            <FormField label={t('agents.model')}>
              <ModelPicker
                providerId={bulkProvider}
                catalogue={bulkModels}
                value={bulkModel}
                onChange={model=>{setBulkModel(model);setBulkEffort("")}}
                placeholder={t('agents.modelPick')}
                className="w-full"
                ariaLabel={t('agents.model')}
              />
            </FormField>
            <FormField label={t('chat.reasoningEffort')}>
              <ReasoningSelect value={bulkEffort} onChange={setBulkEffort} model={bulkModels.value.find(item=>item.id===bulkModel)} requireCatalog={enabledProviders.find(item=>item.id===bulkProvider)?.kind==='codex'} disabled={bulkBusy||!bulkModel}/>
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
              {bulkBusy ? t('agents.applying') : t('agents.apply', { count: selected.length })}
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
                      aria-label={a.name + ' ' + t('agents.select')}
                      checked={selected.includes(a.id)}
                      onChange={(e) =>
                        setSelected((prev) =>
                          e.target.checked
                            ? [...prev, a.id]
                            : prev.filter((id) => id !== a.id),
                        )
                      }
                    />
                  </label>
                )}
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={a.name + ' ' + t('agents.editShort')}
                  onClick={() => { setError(""); setEdit({ ...a }); }}
                >
                  <Edit3 size={16} />
                </Button>
                {a.id !== "general" && (
                  <Button
                    size="icon"
                    variant="ghost"
                    onClick={async () => {
                      if (confirm(a.name + ' ' + t('agents.confirmDelete'))) {
                        try {
                          await del("/api/agents/" + encodeURIComponent(a.id));
                          setSelected(previous => previous.filter(id => id !== a.id));
                          await agents.reload();
                        } catch (e) {
                          setError((e as Error).message);
                        }
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
            {a.preferredSkillIds?.length>0&&<p className="text-[11px] text-[#6C6155]">
              연결된 스킬: {(a.preferredSkillIds||[]).map(id=>skills.value.find(sk=>sk.id===id)?.name||'삭제된 스킬').join(' · ')}
            </p>}
            <p className="line-clamp-2" title={a.description || ""}>{a.description || t('agents.noDescription')}</p>
            <div className="my-2 h-px bg-[#F3EEE5]" />
            <div className="mt-auto flex flex-col gap-1 border-t border-[#EFE9DF] pt-3 text-xs text-[#7C7469]">
              <span>
                <Sparkles size={15} />{" "}
                {providers.value.find((p) => p.id === a.providerId)?.name ||
                  (a.providerId ? t('agents.providerMissing') : t('agents.modelInChat'))}
              </span>
              {a.providerId&&<span>{a.model || enabledProviders.find(p=>p.id===a.providerId)?.model || t('agents.modelNeeded')}</span>}
              <a href={"#/workflows?agent="+encodeURIComponent(a.id)} className="mt-1 inline-flex items-center gap-1 font-semibold text-[#4F6C96] hover:underline">이 에이전트로 다단계 작업 구성 <ArrowRight size={13}/></a>
              {jobs.value.filter(j=>j.kind==='workflow'&&j.input?.definition?.steps?.some(step=>step.agentId===a.id)).length>0&&
               <span className="text-[11px] text-[#7C7469]">
                실제 참여 작업 {jobs.value.filter(j=>j.kind==='workflow'&&j.input?.definition?.steps?.some(step=>step.agentId===a.id)).length}건 ·
                산출물 {jobs.value.filter(j=>j.kind==='workflow'&&j.result?.deliverableId&&j.input?.definition?.steps?.some(step=>step.agentId===a.id)).length}건
               </span>}
              <a href={"#/chat?agent="+encodeURIComponent(a.id)} className="mt-1 inline-flex items-center gap-1 font-semibold text-[#C96442] hover:underline">{t('agents.chatWith')} <ArrowRight size={13}/></a>
            </div>
          </Panel>
        ))}
      </div>
      {!edit && <Notice message={error || agents.error || providers.error} />}
      {showPersonaImport&&<PersonaImportDialog agents={agents.value} onClose={()=>setShowPersonaImport(false)} onImported={async(count)=>{await agents.reload();refresh();setBulkNotice(t('agents.personaImported', { count }))}}/>}
      {edit && (
        <DialogBackdrop onDismiss={closeEditor}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
              <div>
                <small>{t('agents.roleAndModel')}</small>
                <h2>{edit.id ? t('agents.edit') : t('agents.new')}</h2>
              </div>
              <Button size="icon" variant="ghost" onClick={closeEditor} disabled={saving}>
                <X size={19} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <FormField label={t('agents.name')}>
                  <TextInput
                    value={edit.name || ""}
                    onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                  />
                </FormField>
                <FormField label={t('agents.color')}>
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
              <FormField label={t('agents.role')}>
                <TextInput
                  value={edit.description || ""}
                  onChange={(e) =>
                    setEdit({ ...edit, description: e.target.value })
                  }
                  placeholder={t('agents.rolePlaceholder')}
                />
              </FormField>
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <FormField label={t('agents.api')}>
                  <SelectInput
                    value={edit.providerId || ""}
                    onChange={(e) =>
                      setEdit({
                        ...edit,
                        providerId: e.target.value,
                        model: "",
                        reasoningEffort: "",
                      })
                    }
                  >
                    <option value="">{t('agents.selectInChat')}</option>
                    {edit.providerId&&!editProvider&&<option value={edit.providerId} disabled>{t('agents.previousMissing')}</option>}
                    {enabledProviders.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </SelectInput>
                </FormField>
                <FormField label={t('agents.useModel')} hint={edit.providerId ? t('agents.modelIdHint') : t('agents.modelLaterHint')}>
                  <ModelPicker
                    providerId={editProvider?.id || ""}
                    catalogue={editModels}
                    value={edit.model || ""}
                    onChange={(model) => setEdit({ ...edit, model, reasoningEffort: "" })}
                    placeholder={editProvider?.model ? t('agents.defaultModel') : editProvider ? t('agents.pickModel') : t('agents.pickProviderFirst')}
                    className="w-full"
                    ariaLabel={t('agents.agentModel')}
                  />
                </FormField>
              </div>
              <FormField label={t('chat.reasoningEffort')} hint="이 에이전트의 기본 추론 강도입니다. 대화나 회의실에서 지정한 값이 있으면 해당 값이 우선합니다.">
                <ReasoningSelect value={edit.reasoningEffort||""} onChange={reasoningEffort=>setEdit({...edit,reasoningEffort,thinking:undefined})} model={editModels.value.find(item=>item.id===(edit.model||editProvider?.model))} requireCatalog={editProvider?.kind==='codex'} disabled={saving}/>
              </FormField>
              <FormField
                label={t('agents.alias')}
                hint={t('agents.aliasHint')}
              >
                <TextInput
                  value={(edit.aliases || []).join(", ")}
                  onChange={(e) =>
                    setEdit({ ...edit, aliases: e.target.value.split(",") })
                  }
                  placeholder={t('agents.aliasPlaceholder')}
                />
              </FormField>
              <FormField label={t('agents.avatar')} hint={t('agents.imageHint')}>
                <div className="flex items-center gap-3">
                  <div className="grid size-14 shrink-0 place-items-center overflow-hidden rounded-xl border border-[#E7E0D5] bg-[#FAF7F1]">
                    {(edit.avatar || edit.avatarUrl) ? (
                      <img src={String(edit.avatar || edit.avatarUrl)} alt="" className="size-14 object-cover" />
                    ) : (
                      <Bot size={22} className="text-[#C96442]" />
                    )}
                  </div>
                  <label className="cursor-pointer rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
                    {t('agents.imagePick')}
                    <input
                      type="file"
                      className="hidden"
                      accept="image/png,image/jpeg,image/webp"
                      onChange={(e) => {
                        const file = e.target.files?.[0];
                        if (!file) return;
                        if(!['image/png','image/jpeg','image/webp'].includes(file.type)){setError(t('agents.imageTypeError'));return;}
                        if(file.size>1024*1024){setError(t('agents.imageSizeError'));return;}
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
                      {t('agents.remove')}
                    </Button>
                  )}
                </div>
              </FormField>
              <FormField label="실행에 우선 사용할 설치된 스킬" hint="선택 스킬은 실제 채팅·그룹대화·워크플로의 에이전트 컨텍스트에서 우선 주입됩니다. 최대 8개, 도구 실행 권한은 기존 승인 정책을 따릅니다.">
                <div className="grid max-h-36 grid-cols-1 gap-2 overflow-auto rounded-xl border border-[#E7E0D5] p-3 sm:grid-cols-2">
                  {skills.value.filter(sk=>sk.enabled&&sk.status!=='draft').map(sk=><label key={sk.id} className="flex items-center gap-2 text-xs text-[#554C42]">
                    <input type="checkbox" checked={(edit.preferredSkillIds||[]).includes(sk.id)}
                     disabled={saving||(!(edit.preferredSkillIds||[]).includes(sk.id)&&(edit.preferredSkillIds||[]).length>=8)}
                     onChange={e=>setEdit({...edit,preferredSkillIds:e.target.checked?[...(edit.preferredSkillIds||[]),sk.id]:(edit.preferredSkillIds||[]).filter(id=>id!==sk.id)})}/>
                    {sk.name}
                  </label>)}
                  {!skills.value.some(sk=>sk.enabled&&sk.status!=='draft')&&<p className="text-xs text-[#8F8174]">설치된 스킬이 없습니다. 연결 메뉴에서 먼저 스킬을 설치하세요.</p>}
                </div>
              </FormField>
              <FormField label={t('agents.instructions')} hint={t('agents.instructions.hint')}>
                <TextArea
                  className="min-h-28"
                  rows={6}
                  value={edit.systemPrompt || ""}
                  onChange={(e) =>
                    setEdit({ ...edit, systemPrompt: e.target.value })
                  }
                />
              </FormField>
              <Notice message={error}/>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={closeEditor} disabled={saving}>
                {t('agents.cancel')}
              </Action>
              <Action onClick={save} disabled={saving||!edit.name?.trim()}>
                <Save size={16} /> {saving ? t('agents.saving') : t('agents.save')}
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
 useEffect(()=>{let mounted=true;get<{profiles:PersonaProfile[]}>('/api/personas/import/preview').then(result=>{if(!mounted)return;setProfiles(result.profiles);setSelected(result.profiles.filter(profile=>profile.available&&!profile.exists).map(profile=>profile.sourceProfile))}).catch(e=>{if(mounted)setError(e.message)}).finally(()=>{if(mounted)setLoading(false)});return()=>{mounted=false}},[])
 const available=profiles.filter(profile=>profile.available&&(!profile.exists||refreshIdentity))
 const apply=async()=>{
  if(busy||!selected.length)return;setBusy(true);setError('')
  try{const result=await post<{imported:number;skipped:number;errors:{sourceProfile:string;error:string}[]}>('/api/personas/import',{apply:true,profileIds:selected,...(refreshIdentity?{refreshIdentity:true}:{})});await onImported(result.imported);if(result.errors.length)setError(result.errors.map(item=>item.sourceProfile+': '+item.error).join(' · '));else onClose()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <DialogBackdrop onDismiss={()=>{if(!busy)onClose()}}><DialogPanel size="wide" aria-label={t('agents.import')}><div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small className="text-[10px] tracking-widest text-[#A2937F]">{t('ui.label.personas')}</small><h2 className="mt-1 font-bold">{t('agents.persona.title')}</h2></div><Button size="icon" variant="ghost" aria-label={t('agents.close')} disabled={busy} onClick={onClose}><X size={18}/></Button></div><div className="grid gap-4 p-5"><p className="text-[12px] text-[#8F7D65]">{t('agents.persona.desc')}</p>{loading?<div className="flex items-center justify-center gap-2 py-10 text-[#A2937F]"><LoaderCircle size={17} className="animate-spin"/> {t('agents.persona.loading')}</div>:<><div className="flex items-center justify-between text-[11px] text-[#9A866A]"><span>{t('agents.persona.found', { count: profiles.length, selected: selected.length })}</span><button className="underline underline-offset-2" disabled={busy||!available.length} onClick={()=>setSelected(selected.length===available.length?[]:available.map(profile=>profile.sourceProfile))}>{selected.length===available.length?t('agents.persona.clear'):t('agents.persona.selectAll')}</button></div><div className="grid max-h-[48vh] gap-3 overflow-auto sm:grid-cols-2">{profiles.map((profile,index)=>{const agent=agents.find(item=>item.id===profile.id);const enabled=profile.available&&(!profile.exists||refreshIdentity);const checked=selected.includes(profile.sourceProfile);return <div className={'rounded-xl border p-3 '+(checked?'border-[#D1B894] bg-[#FBF4E8]':'border-[#E7E0D5] bg-[#FFFCF7]')} key={profile.sourceProfile}><label className="flex cursor-pointer items-center gap-3"><input type="checkbox" className="accent-[#AA8B63]" checked={checked} disabled={busy||!enabled} onChange={()=>setSelected(previous=>checked?previous.filter(id=>id!==profile.sourceProfile):[...previous,profile.sourceProfile])}/><span className="grid size-10 shrink-0 place-items-center overflow-hidden rounded-xl text-[13px] font-semibold text-white" style={{background:agent?.color||accents[index%accents.length]}}>{(agent?.avatar||agent?.avatarUrl)?<img src={String(agent.avatar||agent.avatarUrl)} alt="" className="size-10 object-cover"/>:profile.name.slice(0,1)}</span><span className="min-w-0 flex-1"><strong className="block text-[12px] text-[#776249]">{profile.name}</strong><small className="block text-[10px] text-[#AC987A]">{profile.role||profile.description}</small></span>{profile.exists&&<span className="rounded bg-[#EDE8DE] px-1.5 py-0.5 text-[9px] text-[#9D8D74]">{t('agents.persona.imported')}</span>}</label><div className="mt-3 flex gap-2 text-[9px] text-[#B2A087]"><span>{profile.hasAvatar?t('agents.persona.avatar'):t('agents.persona.defaultAvatar')}</span><span>·</span><span>{t('agents.persona.mine')}</span></div>{profile.personaPreview&&<details className="mt-2 text-[10px] text-[#9F896B]"><summary className="cursor-pointer">{t('agents.persona.preview')}</summary><pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-[#F4EDE1] p-3 font-sans text-[10px] leading-6 text-[#A18C6E]">{profile.personaPreview}</pre></details>}{profile.error&&<p className="mt-2 text-[10px] text-rose-600">{profile.error}</p>}</div>})}</div>{!profiles.length&&<p className="py-7 text-center text-[12px] text-[#A2937F]">{t('agents.persona.none')}</p>}<label className="flex items-start gap-2 text-[11px] text-[#9A866A]"><input type="checkbox" className="mt-1 accent-[#AA8B63]" checked={refreshIdentity} disabled={busy} onChange={e=>{setRefreshIdentity(e.target.checked);if(!e.target.checked)setSelected(previous=>previous.filter(id=>!profiles.find(profile=>profile.sourceProfile===id)?.exists))}}/><span>{t('agents.persona.refresh')}<small className="mt-1 block text-[10px] text-[#B3A187]">{t('agents.persona.refreshHint')}</small></span></label></>}<Notice message={error}/></div><div className="flex items-center justify-end gap-2 border-t border-[#EFE9DF] p-5"><Button variant="ghost" disabled={busy} onClick={onClose}>{t('agents.close')}</Button><Button variant="primary" disabled={loading||busy||!selected.length} onClick={()=>void apply()}>{busy?<LoaderCircle size={15} className="animate-spin"/>:<FolderInput size={15}/>} {t('agents.persona.apply', { count: selected.length })}</Button></div></DialogPanel></DialogBackdrop>
}
