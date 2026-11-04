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
            <Action
              variant="secondary"
              onClick={() => {
                setBulk((x) => !x);
                setSelected(agents.value.map((x) => x.id));
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
                {a.avatar ? (
                  <img src={a.avatar} alt="" className="size-12 object-cover" />
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
              <FormField label="프로필 이미지" hint="PNG/JPEG/WebP · 카드에 표시됩니다">
                <div className="flex items-center gap-3">
                  <div className="grid size-14 shrink-0 place-items-center overflow-hidden rounded-xl border border-[#E7E0D5] bg-[#FAF7F1]">
                    {edit.avatar ? (
                      <img src={edit.avatar} alt="" className="size-14 object-cover" />
                    ) : (
                      <Bot size={22} className="text-[#C96442]" />
                    )}
                  </div>
                  <label className="cursor-pointer rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
                    이미지 선택
                    <input
                      type="file"
                      className="hidden"
                      accept="image/*"
                      onChange={(e) => {
                        const file = e.target.files?.[0];
                        if (!file) return;
                        const reader = new FileReader();
                        reader.onload = () => {
                          const image = String(reader.result || "");
                          if (!edit.id) {
                            setEdit({ ...edit, avatar: image });
                            return;
                          }
                          void post("/api/agents/" + edit.id + "/avatar", { image })
                            .then((r: any) => setEdit({ ...edit, avatar: r.avatar }))
                            .catch((err: any) => setError(err.message));
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
export function RoomsPage({
  openSession,
}: {
  openSession: (id: string) => void;
}) {
  const rooms = useRemote<Room[]>("/api/rooms", []),
    agents = useRemote<Agent[]>("/api/agents", []),
    jobs = useRemote<any[]>("/api/jobs", []);
  const [edit, setEdit] = useState<Partial<Room> | null>(null),
    [goal, setGoal] = useState<Record<string, string>>({}),
    [error, setError] = useState(""),
    [pending, setPending] = useState("");
  const save = async () => {
    if (!edit) return;
    try {
      await post("/api/rooms", edit);
      setEdit(null);
      rooms.reload();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const run = async (r: Room) => {
    if (!goal[r.id]?.trim()) return;
    setPending(r.id);
    setError("");
    try {
      await post("/api/rooms/" + r.id + "/run", { goal: goal[r.id] });
      jobs.reload();
      setGoal((x) => ({ ...x, [r.id]: "" }));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setPending("");
    }
  };
  return (
    <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
      <Headline
        eyebrow="MULTI-AGENT COLLABORATION"
        title="생각이 모이면, 실행이 됩니다."
        description="에이전트 수와 순서를 직접 선택하는 독립적인 회의실. 토론 기록은 세션에 저장됩니다."
        actions={
          <Action
            onClick={() =>
              setEdit({
                name: "새 회의실",
                agentIds: [],
                rounds: 1,
                maxTurns: 12,
                description: "",
              })
            }
          >
            <Plus size={17} /> 회의실 만들기
          </Action>
        }
      />
      <div className="mb-6 flex flex-wrap items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-4">
        <span className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#A94E30]">
          <UsersRound size={26} />
        </span>
        <div>
          <strong>AI Roundtable</strong>
          <p>
            각 에이전트가 자신의 모델·지침으로 이전 발언을 참고해 순서대로
            논의합니다. 인원 제한을 9명으로 고정하지 않습니다.
          </p>
        </div>
        <span className="inline-flex rounded-full bg-white px-3 py-1 text-xs font-semibold text-[#A94E30]">
          DYNAMIC TEAMS
        </span>
      </div>
      {error && <Notice message={error} />}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {rooms.value.map((r) => {
          const matching = jobs.value.filter(
            (j) => j.kind === "meeting" && j.input?.roomId === r.id,
          )[0];
          return (
            <Panel key={r.id} className="flex min-w-0 flex-col gap-4 p-5">
              <div className="flex min-w-0 items-center gap-3">
                <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
                  <UsersRound size={22} />
                </div>
                <div>
                  <h3>{r.name}</h3>
                  <p>{r.description || "공동 연구 · 문제 해결 · 토론"}</p>
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  onClick={() => setEdit({ ...r })}
                >
                  <Settings2 size={18} />
                </Button>
              </div>
              <div className="flex items-center -space-x-2">
                {r.agentIds.map((aid, i) => {
                  const a = agents.value.find((x) => x.id === aid);
                  return (
                    <span
                      key={aid}
                      title={a?.name || aid}
                      style={{
                        background: a?.color || accents[i % accents.length],
                      }}
                    >
                      {(a?.name || "?").slice(0, 1)}
                    </span>
                  );
                })}
                <small>
                  {r.agentIds.length} participants · {r.rounds} round
                  {r.rounds > 1 ? "s" : ""}
                </small>
              </div>
              <div className="flex items-center gap-2">
                <TextInput
                  placeholder="회의 의제 입력..."
                  value={goal[r.id] || ""}
                  onChange={(e) =>
                    setGoal((x) => ({ ...x, [r.id]: e.target.value }))
                  }
                  onKeyDown={(e) => {
                    if (e.key === "Enter") run(r);
                  }}
                />
                <Button
                  title="회의 시작"
                  onClick={() => run(r)}
                  disabled={!goal[r.id]?.trim() || pending === r.id}
                >
                  <Play size={18} fill="currentColor" />
                </Button>
              </div>
              <div className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3">
                {matching && (
                  <StatusPill status={matching.status}>
                    {matching.status}
                  </StatusPill>
                )}
                {r.sessionId && (
                  <Button onClick={() => openSession(r.sessionId!)}>
                    대화 기록 열기 <ArrowRight size={15} />
                  </Button>
                )}
              </div>
            </Panel>
          );
        })}
        {!rooms.value.length && (
          <Panel>
            <Empty
              icon={UsersRound}
              title="회의실이 없습니다"
              description="2명 이상도, 1명도, 필요한 만큼 자유롭게 구성하세요."
            />
          </Panel>
        )}
      </div>
      {edit && (
        <DialogBackdrop onDismiss={() => setEdit(null)}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
              <div>
                <small>ROOM BUILDER</small>
                <h2>회의실 구성</h2>
              </div>
              <Button size="icon" variant="ghost" onClick={() => setEdit(null)}>
                <X size={19} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <FormField label="회의실 이름">
                <TextInput
                  value={edit.name || ""}
                  onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                />
              </FormField>
              <FormField label="설명">
                <TextInput
                  value={edit.description || ""}
                  onChange={(e) =>
                    setEdit({ ...edit, description: e.target.value })
                  }
                />
              </FormField>
              <FormField label="토론 라운드">
                <SelectInput
                  value={edit.rounds || 1}
                  onChange={(e) =>
                    setEdit({ ...edit, rounds: Number(e.target.value) })
                  }
                >
                  {[1, 2, 3, 4].map((i) => (
                    <option value={i} key={i}>
                      {i}회
                    </option>
                  ))}
                </SelectInput>
              </FormField>
              <FormField label="최대 총 발언 수">
                <TextInput
                  type="number"
                  min={1}
                  max={60}
                  value={edit.maxTurns || 12}
                  onChange={(e) =>
                    setEdit({ ...edit, maxTurns: Number(e.target.value) })
                  }
                />
              </FormField>
              <div className="mb-3 text-sm font-bold text-[#3A342D]">
                참여할 에이전트 <strong>{edit.agentIds?.length || 0}명</strong>
              </div>
              <div className="grid max-h-64 grid-cols-1 gap-2 overflow-auto sm:grid-cols-2">
                {agents.value.map((a) => (
                  <label
                    key={a.id}
                    className="flex cursor-pointer items-center gap-3 rounded-xl border border-[#E7E0D5] bg-white p-3 text-sm hover:border-blue-300"
                  >
                    <TextInput
                      type="checkbox"
                      checked={edit.agentIds?.includes(a.id) || false}
                      onChange={(e) =>
                        setEdit({
                          ...edit,
                          agentIds: e.target.checked
                            ? [...(edit.agentIds || []), a.id]
                            : (edit.agentIds || []).filter((x) => x !== a.id),
                        })
                      }
                    />
                    <span style={{ background: a.color || "#5979e8" }}>
                      {a.name.slice(0, 1)}
                    </span>
                    <strong>{a.name}</strong>
                    <small>{a.model || "모델 미설정"}</small>
                  </label>
                ))}
              </div>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={() => setEdit(null)}>
                취소
              </Action>
              <Action
                onClick={save}
                disabled={!edit.name || !edit.agentIds?.length}
              >
                <Check size={16} /> 저장
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
    </div>
  );
}
