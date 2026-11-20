import { DialogBackdrop, DialogPanel } from "../components/Dialog";
import { SkillsPanel } from "./extensions/SkillsPanel";
import { t, useLocale } from "../i18n";
import {
  Button,
  TextInput,
  TextArea,
} from "../components/Primitives";
import { useEffect, useState } from "react";
import {
  Activity, Blocks, BookOpenText, Check, Code2, ExternalLink,
  Plus, Search, Settings2, Sparkles, Trash2, X,
} from "lucide-react";
import { del, get, post } from "../api";
import {
  Action,
  Empty,
  FormField,
  Headline,
  Notice,
  Panel,
  StateTag,
  Subheading,
  useRemote,
} from "../components/UI";
type Mcp = {
  id: string;
  name: string;
  url?: string;
  enabled: boolean;
};
type Skill = {
  id: string;
  name: string;
  instructions: string;
  enabled: boolean;
  status?: string;
  source?: string;
};
const featured = [
  {
    name: "Hugging Face",
    description: "모델·데이터셋을 검색하는 원격 MCP",
    descriptionEn: "Remote MCP for searching models and datasets",
    icon: BrainIcon,
    endpoint: "https://huggingface.co/mcp",
    url: "https://huggingface.co",
  },
];
function BrainIcon({ size = 22 }: { size?: number }) {
  return <Sparkles size={size} />;
}
const skillTemplates = [
  {
    name: "웹 리서치",
    nameEn: "Web research",
    category: "리서치",
    categoryEn: "Research",
    instructions:
      "검색이 필요한 요청은 web_search를 먼저 호출하고, 서로 다른 출처 2~3개를 비교한 뒤 URL과 함께 요약한다. 확인되지 않은 내용은 추측이라고 명시한다.",
    instructionsEn:
      "For requests that need search, call web_search first, compare two or three independent sources, and summarize with URLs. State clearly when something is unverified and is an inference.",
  },
  {
    name: "리서치 노트",
    nameEn: "Research notes",
    category: "리서치",
    categoryEn: "Research",
    instructions:
      "조사한 내용을 주장·근거·출처·추가 확인 항목으로 나눠 노트로 정리한다. 출처가 없는 주장은 별도 표시한다.",
    instructionsEn:
      "Organize findings into claims, evidence, sources and open questions. Mark claims without a source separately.",
  },
  {
    name: "경쟁사 분석",
    nameEn: "Competitor analysis",
    category: "리서치",
    categoryEn: "Research",
    instructions:
      "제품, 가격, 타깃, 차별점 기준으로 비교표를 만든다. 확인된 사실과 추정을 구분하고 출처 URL을 남긴다.",
    instructionsEn:
      "Build a comparison table of product, pricing, target and differentiators. Separate confirmed facts from estimates and keep source URLs.",
  },
  {
    name: "파일 정리",
    nameEn: "File organization",
    category: "파일",
    categoryEn: "Files",
    instructions:
      "대상 폴더를 file_list로 먼저 확인하고, 이동·이름 변경 전에 현재 구조를 요약한다. 파일을 삭제하기 전에는 항상 사용자에게 확인한다.",
    instructionsEn:
      "Inspect the target folder with file_list first and summarize its structure before moving or renaming anything. Always confirm with the user before deleting files.",
  },
  {
    name: "파일 검색",
    nameEn: "File search",
    category: "파일",
    categoryEn: "Files",
    instructions:
      "파일 이름과 내용을 함께 검색하고, 일치한 경로·크기·수정 시간을 표로 정리한다. 읽기 전용으로 먼저 확인한다.",
    instructionsEn:
      "Search file names and contents together, then report matching paths, sizes and modification times in a table. Start read-only.",
  },
  {
    name: "코드 리뷰",
    nameEn: "Code review",
    category: "코딩",
    categoryEn: "Coding",
    instructions:
      "버그, 보안, 성능, 유지보수성 순서로 리뷰한다. 각 지적에는 파일 경로와 근거를 붙이고, 확실하지 않은 내용은 질문으로 남긴다.",
    instructionsEn:
      "Review in the order of bugs, security, performance and maintainability. Attach a file path and evidence to each finding and leave uncertain points as questions.",
  },
  {
    name: "테스트 작성",
    nameEn: "Write tests",
    category: "코딩",
    categoryEn: "Coding",
    instructions:
      "기존 테스트 프레임워크를 먼저 확인하고, 정상·경계·실패 경로를 각각 검증하는 테스트를 추가한다. 테스트가 실제로 실패하는지도 확인한다.",
    instructionsEn:
      "Check the existing test framework first, then add tests covering success, boundary and failure paths. Confirm the tests actually fail without the fix.",
  },
  {
    name: "리팩터링",
    nameEn: "Refactoring",
    category: "코딩",
    categoryEn: "Coding",
    instructions:
      "동작을 바꾸지 않는 범위에서 중복과 결합을 줄인다. 변경 전후 테스트를 실행하고, 공개 API 변경은 별도로 보고한다.",
    instructionsEn:
      "Reduce duplication and coupling without changing behavior. Run tests before and after, and report public API changes separately.",
  },
  {
    name: "Git 작업",
    nameEn: "Git work",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "변경 상태를 git status와 diff로 확인한 뒤 작업한다. 커밋 메시지는 변경 이유를 한 줄로 요약하고, 사용자가 요청하지 않은 push는 하지 않는다.",
    instructionsEn:
      "Check git status and diff before working. Summarize the reason for the change in one commit line and never push unless the user asks.",
  },
  {
    name: "배포 체크리스트",
    nameEn: "Release checklist",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "빌드, 테스트, 환경변수, 마이그레이션, 롤백 계획을 순서대로 확인한다. 실패한 항목이 있으면 배포 완료라고 말하지 않는다.",
    instructionsEn:
      "Check build, tests, environment variables, migrations and rollback plan in order. If anything fails, do not call the release complete.",
  },
  {
    name: "로그 분석",
    nameEn: "Log analysis",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "에러 빈도와 시간대, 공통 스택 트레이스, 최초 발생 시점을 분리해 분석한다. 원인과 추정을 구분한다.",
    instructionsEn:
      "Separate error frequency and timing, shared stack traces and first occurrence. Distinguish cause from inference.",
  },
  {
    name: "장애 대응",
    nameEn: "Incident response",
    category: "개발 운영",
    categoryEn: "DevOps",
    instructions:
      "영향 범위, 현재 상태, 즉시 완화 조치, 재발 방지 항목 순서로 정리한다. 확인되지 않은 원인을 단정하지 않는다.",
    instructionsEn:
      "Report impact, current state, immediate mitigation and prevention items in that order. Never assert an unverified cause.",
  },
  {
    name: "보안 점검",
    nameEn: "Security review",
    category: "보안",
    categoryEn: "Security",
    instructions:
      "비밀키 노출, 인증 우회, 입력 검증, 권한 범위를 우선 점검한다. 민감한 값을 출력하지 않고 위치와 위험도만 보고한다.",
    instructionsEn:
      "Check secret exposure, auth bypass, input validation and permission scope first. Report locations and risk only, never the sensitive values.",
  },
  {
    name: "데이터 분석",
    nameEn: "Data analysis",
    category: "데이터",
    categoryEn: "Data",
    instructions:
      "질문을 지표와 차원으로 나누고, 데이터 범위와 전처리 가정을 먼저 밝힌다. 결과와 한계를 함께 보고한다.",
    instructionsEn:
      "Split the question into metrics and dimensions, state the data range and preprocessing assumptions, and report results with limitations.",
  },
  {
    name: "SQL 쿼리",
    nameEn: "SQL queries",
    category: "데이터",
    categoryEn: "Data",
    instructions:
      "스키마를 먼저 확인하고 읽기 전용 쿼리로 검증한다. 대량 변경 전에는 예상 행 수와 롤백 방법을 제시한다.",
    instructionsEn:
      "Inspect the schema first and verify with read-only queries. Before bulk changes, state the expected row count and rollback method.",
  },
  {
    name: "문서 작성",
    nameEn: "Writing documents",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "독자, 목적, 핵심 메시지를 먼저 정하고 제목·요약·본문·다음 단계 순서로 작성한다. 사실과 의견을 구분한다.",
    instructionsEn:
      "Define the audience, purpose and key message first, then write title, summary, body and next steps. Separate facts from opinions.",
  },
  {
    name: "회의 요약",
    nameEn: "Meeting summary",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "결정 사항, 담당자, 마감, 미해결 질문을 구분해 요약한다. 원문에 없는 담당자나 날짜를 만들어내지 않는다.",
    instructionsEn:
      "Summarize decisions, owners, deadlines and open questions separately. Never invent owners or dates not in the source.",
  },
  {
    name: "번역·현지화",
    nameEn: "Translation and localization",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "원문의 의미와 톤을 유지하고, 고유명사·UI 문자열은 용어집을 따른다. 직역투를 피하고 변경 이유를 간단히 남긴다.",
    instructionsEn:
      "Keep the meaning and tone of the source, follow the glossary for proper nouns and UI strings, avoid literal phrasing, and note why a change was made.",
  },
  {
    name: "API 문서화",
    nameEn: "API documentation",
    category: "글쓰기",
    categoryEn: "Writing",
    instructions:
      "엔드포인트, 인증, 요청·응답 예시, 오류 코드, 제한 사항을 실제 코드와 대조해 작성한다.",
    instructionsEn:
      "Document endpoints, authentication, request and response examples, error codes and limits against the real code.",
  },
  {
    name: "이메일·메시지",
    nameEn: "Email and messages",
    category: "커뮤니케이션",
    categoryEn: "Communication",
    instructions:
      "목적, 요청 사항, 기한, 다음 단계가 한 번에 보이게 작성한다. 상대에 따라 정중함과 간결함의 균형을 조정한다.",
    instructionsEn:
      "Show purpose, request, deadline and next step at a glance. Adjust the balance of politeness and brevity for the recipient.",
  },
  {
    name: "일정 정리",
    nameEn: "Schedule planning",
    category: "생산성",
    categoryEn: "Productivity",
    instructions:
      "할 일을 중요도·마감·소요 시간으로 나누고 오늘/이번 주/나중으로 묶는다. 일정 충돌을 먼저 확인한다.",
    instructionsEn:
      "Split tasks by importance, deadline and effort, then group them into today, this week and later. Check for conflicts first.",
  },
  {
    name: "프롬프트 설계",
    nameEn: "Prompt design",
    category: "AI",
    categoryEn: "AI",
    instructions:
      "역할, 입력, 제약, 출력 형식, 검증 기준을 명시한 프롬프트를 만든다. 모호한 지시는 예시로 보완한다.",
    instructionsEn:
      "Write prompts that specify role, input, constraints, output format and validation criteria. Clarify vague instructions with examples.",
  },
];
function StatusLine({
  label,
  value,
}: {
  label: string;
  value: string | number;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <i className="size-1.5 rounded-full bg-emerald-500" />
      <span className="text-[#9C9388]">{label}</span>
      <strong className="text-[#4A443C]">{value}</strong>
    </span>
  );
}
export type ExtensionTab='market'|'installed'|'skills'
export function ExtensionsPage({section,embedded=false}:{section?:ExtensionTab;embedded?:boolean}={}) {
  const loc = useLocale();
  const mcp = useRemote<Mcp[]>("/api/mcp", []),
    skills = useRemote<Skill[]>("/api/skills", []);
  const [repo, setRepo] = useState(""),
    [repoBusy, setRepoBusy] = useState(false);
  const [localTab,setTab]=useState<ExtensionTab>("market"),
    [error,setError]=useState(""),[message,setMessage]=useState("");
  const tab=section??localTab;
  const [skillQuery, setSkillQuery] = useState(""),
    [skillCategory, setSkillCategory] = useState("all");
  const templates = skillTemplates.map((item) =>
    loc === "en"
      ? {
          ...item,
          name: item.nameEn,
          category: item.categoryEn,
          instructions: item.instructionsEn,
        }
      : item,
  );
  const skillCategories = [
    "all",
    ...new Set(templates.map((item) => item.category)),
  ];
  const filteredSkills = templates.filter(
    (item) =>
      (skillCategory === "all" || item.category === skillCategory) &&
      (item.name + item.instructions + item.category)
        .toLowerCase()
        .includes(skillQuery.toLowerCase()),
  );
  const [edit, setEdit] = useState<any | null>(null),
    [skillEdit, setSkillEdit] = useState<Partial<Skill> | null>(null),
    [tools, setTools] = useState<any[]>([]);
  const install = async (entry: any) => {
    if (!confirm(t('ext.confirmRegister'))) return;
    setError("");setMessage("");
    try {
      await post("/api/mcp", {name:entry.name,
        url:entry.endpoint || entry.url || "",enabled:true});
      await mcp.reload();setMessage(t('ext.registeredHint'));
    } catch (cause) {setError((cause as Error).message)}
  };
  const saveMcp = async () => {
    if (!edit) return;
    try {
      await post("/api/mcp", edit);
      setEdit(null);
      mcp.reload();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const saveSkill = async () => {
    if (!skillEdit?.name) return;
    try {
      await post("/api/skills", skillEdit);
      setSkillEdit(null);
      skills.reload();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const addSkillTemplate = async (template: any) => {
    if (skills.value.some((x) => x.name === template.name)) return;
    try {
      await post("/api/skills", { ...template, enabled: true });
      await skills.reload();
      setMessage(t('ext.skillAdded', { name: template.name }));
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const importRepo = async () => {
    if (!repo.trim()) return;
    setRepoBusy(true);
    setError("");
    setMessage("");
    try {
      const r = await post<any>("/api/skills/import-repo", {
        repo: repo.trim(),
      });
      setMessage(t('ext.skillImportedRepo', { repo: r.repo, count: r.added }));
      await skills.reload();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRepoBusy(false);
    }
  };
  const discover = async (silent = false) => {
    try {
      setTools(await get("/api/mcp/discover"));
      if (!silent) setMessage(t('ext.toolsFound'));
    } catch (e) {
      if (!silent) setError((e as Error).message);
    }
  };
  useEffect(() => {
    void discover(true);
  }, [tab]);
  return (
    <div className={embedded?"min-w-0":"mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8"}>
      <Headline
        eyebrow={t('ui.eyebrow.marketplace')}
        title={t('ext.title')}
        description={t('ext.description')}
        actions={
          <Action
            variant="secondary"
            onClick={() => {
              setEdit({name:"",url:"",enabled:true});
            }}
          >
            <Plus size={17} /> {t('ext.addMcp')}
          </Action>
        }
      />
      <div className="mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-4 py-3 text-[11px] text-[#5C544A]">
        <StatusLine label={t('ext.tab.servers')} value={mcp.value.length} />
        <StatusLine
          label={t('ext.tab.tools')}
          value={tools.filter((t) => !t.error).length}
        />
        <StatusLine
          label={t('ext.tab.skills')}
          value={
            skills.value.filter((s) => s.enabled).length +
            "/" +
            skills.value.length
          }
        />
      </div>
      {!embedded&&<div className="mb-6 flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3">
        <Button
          variant={tab === "market" ? "primary" : "ghost"}
          onClick={() => setTab("market")}
        >
          <Blocks size={17} /> MCP
        </Button>
        <Button
          variant={tab === "installed" ? "primary" : "ghost"}
          onClick={() => setTab("installed")}
        >
          {t('ext.tab.installed')} <b>{mcp.value.length}</b>
        </Button>
        <Button
          variant={tab === "skills" ? "primary" : "ghost"}
          onClick={() => setTab("skills")}
        >
          <BookOpenText size={17} /> {t('ext.tab.skills')}{" "}
          <b>
            {skills.value.filter((s) => s.enabled).length}/{skills.value.length}
          </b>
        </Button>
      </div>}
      {error && <Notice message={error} />}
      {message && (
        <div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700">
          <Check size={16} />
          {message}
        </div>
      )}
      {tab === "market" && (
        <>
          <Subheading
            title={t('ext.recommendedTools')}
            extra={
              <span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">
                VERIFIED SOURCES
              </span>
            }
          />
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {featured.map((x) => {
              const Icon = x.icon;
              const record = mcp.value.find((y) => y.name === x.name);
              return (
                <Panel key={x.name} className="flex min-w-0 flex-col gap-3 p-5">
                  <div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
                    <Icon size={24} />
                  </div>
                  <h3>{x.name}</h3>
                  <p>{loc === "en" ? x.descriptionEn || x.description : x.description}</p>
                  <div className="mt-auto flex items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3">
                    <a href={x.url} target="_blank" rel="noreferrer">
                      {t('ext.officialGuide')} <ExternalLink size={13} />
                    </a>
                    {record ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          try {
                            await del("/api/mcp/" + record.id);
                            await mcp.reload();
                            setMessage(t('ext.cancelled', { name: x.name }));
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        {t('ext.cancel')}
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={() => install(x)}
                      >
                        {t('ext.install')}
                      </Button>
                    )}
                  </div>
                </Panel>
              );
            })}
          </div>
        </>
      )}
      {tab === "installed" && (
        <>
          <Subheading
            title={t('ext.connectedServers')}
            extra={
              <Action variant="secondary" onClick={discover}>
                <Activity size={16} /> {t('ext.toolSearch')}
              </Action>
            }
          />
          <div className="flex min-w-0 flex-col gap-3">
            {mcp.value.map((x) => (
              <Panel
                key={x.id}
                className="flex min-w-0 flex-wrap items-center justify-between gap-3 rounded-xl border border-[#E7E0D5] p-4"
              >
                <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
                  <Blocks size={21} />
                </div>
                <div>
                  <strong>{x.name}</strong>
                  <p>
                    {x.url}
                  </p>
                </div>
                <StateTag
                  online={Boolean(
                    tools.find((t) => t.mcpId === x.id && !t.error),
                  )}
                >
                  {(() => {
                    const own = tools.filter((t) => t.mcpId === x.id);
                    const ok = own.filter((t) => !t.error).length;
                    if (ok) return t('ext.connectedTools', { count: ok });
                    if (own.some((t) => t.error)) return t('ext.connectionFailed');
                    return tools.length ? t('ext.checkConnection') : t('ext.checking');
                  })()}
                </StateTag>
                <Button
                  size="icon"
                  variant="ghost"
                  title={t('ext.settings')}
                  onClick={() => {
                    setEdit(x);
                  }}
                >
                  <Settings2 size={17} />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  title={t('ext.remove')}
                  onClick={async () => {
                    if (confirm(t('ext.confirmRemove'))) {
                      await del("/api/mcp/" + x.id);
                      mcp.reload();
                    }
                  }}
                >
                  <Trash2 size={17} />
                </Button>
              </Panel>
            ))}
            {!mcp.value.length && (
              <Panel>
                <Empty
                  icon={Blocks}
                  title={t('ext.noServers')}
                  description={t('ext.noServersDesc')}
                />
              </Panel>
            )}
          </div>
          {tools.length > 0 && (
            <Panel className="grid gap-3">
              <Subheading
                title={t('ext.realTools')}
                extra={<span>{t('ext.count', { count: tools.filter((item) => !item.error).length })}</span>}
              />
              {tools.map((t) => (
                <div
                  key={t.name + t.mcpId}
                  className="overflow-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4 text-xs leading-6"
                >
                  <Code2 size={16} />
                  <strong>{t.name}</strong>
                  <span>{t.error || t.description}</span>
                </div>
              ))}
            </Panel>
          )}
        </>
      )}
      {tab === "skills" && (
        <SkillsPanel
          repo={repo} setRepo={setRepo} repoBusy={repoBusy} importRepo={importRepo}
          skills={skills} addSkillTemplate={addSkillTemplate} filteredSkills={filteredSkills}
          skillCategories={skillCategories} skillCategory={skillCategory} setSkillCategory={setSkillCategory}
          skillQuery={skillQuery} setSkillQuery={setSkillQuery} setSkillEdit={setSkillEdit}
          setError={setError} setMessage={setMessage}
        />
      )}
      {edit && (
        <DialogBackdrop onDismiss={() => setEdit(null)}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
              <div>
                <small>{t('ui.label.mcpConnection')}</small>
                <h2>{t('ext.register')}</h2>
              </div>
              <Button size="icon" variant="ghost" onClick={() => setEdit(null)}>
                <X size={18} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <FormField label={t('ext.displayName')}>
                <TextInput
                  value={edit.name}
                  onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                />
              </FormField>
              <FormField label={t('ext.url')}>
                <TextInput value={edit.url || ""} onChange={(e)=>setEdit({...edit,url:e.target.value})}/>
              </FormField>
              <FormField label={t('ext.bearer')}>
                <TextInput type="password" value={edit.bearerToken || ""} onChange={(e)=>setEdit({...edit,bearerToken:e.target.value})} placeholder={t('ext.optional')}/>
              </FormField>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={() => setEdit(null)}>
                {t('ext.cancel')}
              </Action>
              <Action onClick={saveMcp}>
                <Check size={16} /> {t('ext.save')}
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
      {skillEdit && (
        <DialogBackdrop onDismiss={() => setSkillEdit(null)}>
          <DialogPanel size="wide">
            <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
              <div>
                <small>{t('ui.label.skillEditor')}</small>
                <h2>{t('ext.skillWrite')}</h2>
              </div>
              <Button
                size="icon"
                variant="ghost"
                onClick={() => setSkillEdit(null)}
              >
                <X size={18} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <FormField label={t('ext.skillName')}>
                <TextInput
                  value={skillEdit.name || ""}
                  onChange={(e) =>
                    setSkillEdit({ ...skillEdit, name: e.target.value })
                  }
                />
              </FormField>
              <FormField label={t('ext.skillInstructions')}>
                <TextArea
                  className="min-h-28"
                  rows={10}
                  value={skillEdit.instructions || ""}
                  onChange={(e) =>
                    setSkillEdit({ ...skillEdit, instructions: e.target.value })
                  }
                />
              </FormField>
              <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]">
                <TextInput
                  type="checkbox"
                  checked={Boolean(skillEdit.enabled)}
                  onChange={(e) =>
                    setSkillEdit({
                      ...skillEdit,
                      enabled: e.target.checked,
                      status: e.target.checked ? "approved" : "proposed",
                    })
                  }
                />{" "}
                {t('ext.applySystem')}
              </label>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={() => setSkillEdit(null)}>
                {t('ext.cancel')}
              </Action>
              <Action onClick={saveSkill}>
                <Check size={16} /> {t('ext.saveSkill')}
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
    </div>
  );
}
