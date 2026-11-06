import { DialogBackdrop, DialogPanel } from "../components/Dialog";
import { SkillsPanel } from "./extensions/SkillsPanel";
import {
  Button,
  TextInput,
  TextArea,
  SelectInput,
} from "../components/Primitives";
import { useEffect, useState } from "react";
import {
  Activity,
  ArrowDownToLine,
  ArrowRight,
  Blocks,
  BookOpenText,
  Check,
  ChevronRight,
  Code2,
  Compass,
  Download,
  ExternalLink,
  FileCode2,
  Globe2,
  HardDrive,
  LoaderCircle,
  PackagePlus,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
  Trash2,
  X,
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
  refresh,
  useRemote,
} from "../components/UI";
type Mcp = {
  id: string;
  name: string;
  transport: "http" | "stdio";
  url?: string;
  command?: string;
  args?: string[];
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
type RegistryItem = {
  name: string;
  description: string;
  url?: string;
  packageName?: string;
  transport?: string;
};
const featured = [
  {
    name: "Playwright Browser",
    description: "웹 페이지 탐색, 클릭, 입력 및 스크린샷",
    icon: Globe2,
    transport: "stdio",
    command: "npx",
    args: ["-y", "@playwright/mcp@latest"],
    url: "https://playwright.dev/mcp/installation",
  },
  {
    name: "Hugging Face",
    description: "모델·데이터셋을 검색하는 원격 MCP",
    icon: BrainIcon,
    transport: "http",
    command: "",
    args: [],
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
    category: "리서치",
    instructions:
      "검색이 필요한 요청은 web_search를 먼저 호출하고, 서로 다른 출처 2~3개를 비교한 뒤 URL과 함께 요약한다. 확인되지 않은 내용은 추측이라고 명시한다.",
  },
  {
    name: "리서치 노트",
    category: "리서치",
    instructions:
      "조사한 내용을 주장·근거·출처·추가 확인 항목으로 나눠 노트로 정리한다. 출처가 없는 주장은 별도 표시한다.",
  },
  {
    name: "경쟁사 분석",
    category: "리서치",
    instructions:
      "제품, 가격, 타깃, 차별점 기준으로 비교표를 만든다. 확인된 사실과 추정을 구분하고 출처 URL을 남긴다.",
  },
  {
    name: "파일 정리",
    category: "파일",
    instructions:
      "대상 폴더를 file_list로 먼저 확인하고, 이동·이름 변경 전에 현재 구조를 요약한다. 파일을 삭제하기 전에는 항상 사용자에게 확인한다.",
  },
  {
    name: "파일 검색",
    category: "파일",
    instructions:
      "파일 이름과 내용을 함께 검색하고, 일치한 경로·크기·수정 시간을 표로 정리한다. 읽기 전용으로 먼저 확인한다.",
  },
  {
    name: "코드 리뷰",
    category: "코딩",
    instructions:
      "버그, 보안, 성능, 유지보수성 순서로 리뷰한다. 각 지적에는 파일 경로와 근거를 붙이고, 확실하지 않은 내용은 질문으로 남긴다.",
  },
  {
    name: "테스트 작성",
    category: "코딩",
    instructions:
      "기존 테스트 프레임워크를 먼저 확인하고, 정상·경계·실패 경로를 각각 검증하는 테스트를 추가한다. 테스트가 실제로 실패하는지도 확인한다.",
  },
  {
    name: "리팩터링",
    category: "코딩",
    instructions:
      "동작을 바꾸지 않는 범위에서 중복과 결합을 줄인다. 변경 전후 테스트를 실행하고, 공개 API 변경은 별도로 보고한다.",
  },
  {
    name: "Git 작업",
    category: "개발 운영",
    instructions:
      "변경 상태를 git status와 diff로 확인한 뒤 작업한다. 커밋 메시지는 변경 이유를 한 줄로 요약하고, 사용자가 요청하지 않은 push는 하지 않는다.",
  },
  {
    name: "배포 체크리스트",
    category: "개발 운영",
    instructions:
      "빌드, 테스트, 환경변수, 마이그레이션, 롤백 계획을 순서대로 확인한다. 실패한 항목이 있으면 배포 완료라고 말하지 않는다.",
  },
  {
    name: "로그 분석",
    category: "개발 운영",
    instructions:
      "에러 빈도와 시간대, 공통 스택 트레이스, 최초 발생 시점을 분리해 분석한다. 원인과 추정을 구분한다.",
  },
  {
    name: "장애 대응",
    category: "개발 운영",
    instructions:
      "영향 범위, 현재 상태, 즉시 완화 조치, 재발 방지 항목 순서로 정리한다. 확인되지 않은 원인을 단정하지 않는다.",
  },
  {
    name: "보안 점검",
    category: "보안",
    instructions:
      "비밀키 노출, 인증 우회, 입력 검증, 권한 범위를 우선 점검한다. 민감한 값을 출력하지 않고 위치와 위험도만 보고한다.",
  },
  {
    name: "데이터 분석",
    category: "데이터",
    instructions:
      "질문을 지표와 차원으로 나누고, 데이터 범위와 전처리 가정을 먼저 밝힌다. 결과와 한계를 함께 보고한다.",
  },
  {
    name: "SQL 쿼리",
    category: "데이터",
    instructions:
      "스키마를 먼저 확인하고 읽기 전용 쿼리로 검증한다. 대량 변경 전에는 예상 행 수와 롤백 방법을 제시한다.",
  },
  {
    name: "문서 작성",
    category: "글쓰기",
    instructions:
      "독자, 목적, 핵심 메시지를 먼저 정하고 제목·요약·본문·다음 단계 순서로 작성한다. 사실과 의견을 구분한다.",
  },
  {
    name: "회의 요약",
    category: "글쓰기",
    instructions:
      "결정 사항, 담당자, 마감, 미해결 질문을 구분해 요약한다. 원문에 없는 담당자나 날짜를 만들어내지 않는다.",
  },
  {
    name: "번역·현지화",
    category: "글쓰기",
    instructions:
      "원문의 의미와 톤을 유지하고, 고유명사·UI 문자열은 용어집을 따른다. 직역투를 피하고 변경 이유를 간단히 남긴다.",
  },
  {
    name: "API 문서화",
    category: "글쓰기",
    instructions:
      "엔드포인트, 인증, 요청·응답 예시, 오류 코드, 제한 사항을 실제 코드와 대조해 작성한다.",
  },
  {
    name: "이메일·메시지",
    category: "커뮤니케이션",
    instructions:
      "목적, 요청 사항, 기한, 다음 단계가 한 번에 보이게 작성한다. 상대에 따라 정중함과 간결함의 균형을 조정한다.",
  },
  {
    name: "일정 정리",
    category: "생산성",
    instructions:
      "할 일을 중요도·마감·소요 시간으로 나누고 오늘/이번 주/나중으로 묶는다. 일정 충돌을 먼저 확인한다.",
  },
  {
    name: "프롬프트 설계",
    category: "AI",
    instructions:
      "역할, 입력, 제약, 출력 형식, 검증 기준을 명시한 프롬프트를 만든다. 모호한 지시는 예시로 보완한다.",
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
export function ExtensionsPage() {
  const mcp = useRemote<Mcp[]>("/api/mcp", []),
    skills = useRemote<Skill[]>("/api/skills", []),
    presets = useRemote<any[]>("/api/mcp/presets", []);
  const catalogSkills = useRemote<{
    items: any[];
    total: number;
    updatedAt: string;
  }>("/api/skill-catalog", { items: [], total: 0, updatedAt: "" }, false);
  const [repo, setRepo] = useState(""),
    [repoBusy, setRepoBusy] = useState(false),
    [pluginRemote, setPluginRemote] = useState<any[]>([]);
  const [tab, setTab] = useState<"market" | "installed" | "plugins" | "skills">(
      "market",
    ),
    [query, setQuery] = useState(""),
    [catalog, setCatalog] = useState<RegistryItem[]>([]),
    [searching, setSearching] = useState(false),
    [installing, setInstalling] = useState(""),
    [error, setError] = useState(""),
    [message, setMessage] = useState("");
  const [skillQuery, setSkillQuery] = useState(""),
    [skillCategory, setSkillCategory] = useState("all");
  const [pluginQuery, setPluginQuery] = useState(""),
    [pluginCategory, setPluginCategory] = useState("all");
  const [catalogQuery, setCatalogQuery] = useState(""),
    [catalogSource, setCatalogSource] = useState("all"),
    [catalogCategory, setCatalogCategory] = useState("all"),
    [catalogLimit, setCatalogLimit] = useState(30),
    [importing, setImporting] = useState("");
  const skillCategories = [
    "all",
    ...new Set(skillTemplates.map((t) => t.category)),
  ];
  const filteredSkills = skillTemplates.filter(
    (t) =>
      (skillCategory === "all" || t.category === skillCategory) &&
      (t.name + t.instructions + t.category)
        .toLowerCase()
        .includes(skillQuery.toLowerCase()),
  );
  const pluginCategories = [
    "all",
    ...new Set(presets.value.map((p: any) => p.category)),
  ];
  const filteredPresets = presets.value.filter(
    (p: any) =>
      (pluginCategory === "all" || p.category === pluginCategory) &&
      (p.name + p.description + p.category)
        .toLowerCase()
        .includes(pluginQuery.toLowerCase()),
  );
  const catalogItems = catalogSkills.value.items || [];
  const catalogSources = [
    "all",
    ...new Set(catalogItems.map((item: any) => item.source)),
  ];
  const catalogCategories = [
    "all",
    ...new Set(catalogItems.map((item: any) => item.category)),
  ];
  const filteredCatalog = catalogItems.filter(
    (item: any) =>
      (catalogSource === "all" || item.source === catalogSource) &&
      (catalogCategory === "all" || item.category === catalogCategory) &&
      (
        item.name +
        item.description +
        item.category +
        item.source +
        (item.tags || []).join(" ")
      )
        .toLowerCase()
        .includes(catalogQuery.toLowerCase()),
  );
  const visibleCatalog = filteredCatalog.slice(0, catalogLimit);
  const [edit, setEdit] = useState<any | null>(null),
    [argText, setArgText] = useState(""),
    [skillEdit, setSkillEdit] = useState<Partial<Skill> | null>(null),
    [tools, setTools] = useState<any[]>([]);
  const install = async (entry: any) => {
    if (
      !confirm(
        entry.name +
          " MCP를 DOMINION에 등록하시겠습니까?\n로컬 MCP는 실행 시 외부 프로그램을 시작할 수 있습니다.",
      )
    )
      return;
    setError("");
    setMessage("");
    try {
      const payload = {
        name: entry.name,
        transport: entry.transport || "stdio",
        url: entry.endpoint || entry.url || "",
        command: entry.command || "npx",
        args:
          entry.args || (entry.packageName ? ["-y", entry.packageName] : []),
      };
      await post("/api/mcp", payload);
      mcp.reload();
      setMessage(
        entry.name + " 등록됨 · 실행 연결은 설치 목록에서 확인할 수 있습니다.",
      );
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const installPreset = async (preset: any) => {
    if (installing) return;
    setInstalling(preset.id);
    setError("");
    setMessage(
      preset.name +
        " 설치 중 · 첫 실행은 패키지 다운로드로 시간이 걸릴 수 있습니다.",
    );
    try {
      const saved = await post<any>("/api/mcp", {
        name: preset.name,
        transport: "stdio",
        command: "npx",
        args: ["-y", preset.package, ...(preset.args || [])],
        enabled: true,
      });
      await mcp.reload();
      for (let attempt = 0; attempt < 30; attempt++) {
        const found = await get<any[]>("/api/mcp/discover");
        setTools(found);
        const own = found.filter((t) => t.mcpId === saved.id && !t.error);
        if (own.length) {
          setMessage(
            preset.name +
              " 연결 완료 · 도구 " +
              own.length +
              "개를 사용할 수 있습니다.",
          );
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
      setMessage(preset.name + " 등록됨 · 연결 확인이 필요합니다.");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setInstalling("");
    }
  };
  const loadCatalog = async (term = "") => {
    setSearching(true);
    setError("");
    try {
      setCatalog(
        await get(
          "/api/mcp/catalog" + (term ? "?q=" + encodeURIComponent(term) : ""),
        ),
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSearching(false);
    }
  };
  const search = async () => {
    if (query.trim() && query.trim().length < 3) {
      setError("3자 이상 검색해주세요.");
      return;
    }
    await loadCatalog(query.trim());
  };
  const saveMcp = async () => {
    if (!edit) return;
    try {
      await post("/api/mcp", {
        ...edit,
        args: argText
          .split("\n")
          .map((x) => x.trim())
          .filter(Boolean),
      });
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
      setMessage(template.name + " 스킬을 추가했습니다.");
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const importCatalogSkill = async (item: any) => {
    if (importing) return;
    setImporting(item.id);
    setError("");
    setMessage("");
    try {
      await post("/api/skill-catalog/import", { id: item.id });
      await skills.reload();
      setMessage(item.source + "의 " + item.name + " 스킬을 가져왔습니다.");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setImporting("");
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
      setMessage(r.repo + "에서 " + r.added + "개 스킬을 가져왔습니다");
      await skills.reload();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRepoBusy(false);
    }
  };
  useEffect(() => {
    const q = pluginQuery.trim();
    if (q.length < 2) {
      setPluginRemote([]);
      return;
    }
    let alive = true;
    const timer = setTimeout(() => {
      get<any[]>("/api/mcp/catalog?q=" + encodeURIComponent(q))
        .then((rows) => {
          if (alive) setPluginRemote(rows || []);
        })
        .catch(() => {
          if (alive) setPluginRemote([]);
        });
    }, 350);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [pluginQuery]);
  const discover = async (silent = false) => {
    try {
      setTools(await get("/api/mcp/discover"));
      if (!silent) setMessage("MCP 도구 검색 완료");
    } catch (e) {
      if (!silent) setError((e as Error).message);
    }
  };
  useEffect(() => {
    void discover(true);
  }, [tab]);
  useEffect(() => {
    void loadCatalog();
  }, []);
  return (
    <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
      <Headline
        eyebrow="CAPABILITY MARKETPLACE"
        title="필요한 능력만, 자유롭게."
        description="MCP 도구와 지식을 찾아 설치하고 관리하세요. 에이전트가 실행 중 연결된 도구를 발견합니다."
        actions={
          <Action
            variant="secondary"
            onClick={() => {
              setEdit({
                name: "",
                transport: "stdio",
                command: "npx",
                url: "",
                enabled: true,
              });
              setArgText("");
            }}
          >
            <Plus size={17} /> MCP 직접 추가
          </Action>
        }
      />
      <div className="mb-4 flex flex-wrap items-center gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-4 py-3 text-[11px] text-[#5C544A]">
        <StatusLine label="MCP 서버" value={mcp.value.length} />
        <StatusLine
          label="연결된 도구"
          value={tools.filter((t) => !t.error).length}
        />
        <StatusLine
          label="스킬"
          value={
            skills.value.filter((s) => s.enabled).length +
            "/" +
            skills.value.length
          }
        />
      </div>
      <div className="mb-6 flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3">
        <Button
          variant={tab === "market" ? "primary" : "ghost"}
          onClick={() => setTab("market")}
        >
          <Blocks size={17} /> MCP
        </Button>
        <Button
          variant={tab === "plugins" ? "primary" : "ghost"}
          onClick={() => setTab("plugins")}
        >
          <PackagePlus size={17} /> 플러그인
        </Button>
        <Button
          variant={tab === "installed" ? "primary" : "ghost"}
          onClick={() => setTab("installed")}
        >
          설치됨 <b>{mcp.value.length}</b>
        </Button>
        <Button
          variant={tab === "skills" ? "primary" : "ghost"}
          onClick={() => setTab("skills")}
        >
          <BookOpenText size={17} /> 스킬{" "}
          <b>
            {skills.value.filter((s) => s.enabled).length}/{skills.value.length}
          </b>
        </Button>
      </div>
      {error && <Notice message={error} />}
      {message && (
        <div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700">
          <Check size={16} />
          {message}
        </div>
      )}
      {tab === "market" && (
        <>
          <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
            <Subheading
              title="공식 MCP Registry"
              extra={
                <div className="flex items-center gap-2">
                  <span className="text-[11px] text-[#8A8176]">
                    실시간 · {catalog.length}개
                  </span>
                  <Button
                    variant="secondary"
                    onClick={() => void loadCatalog(query.trim())}
                    disabled={searching}
                  >
                    <RefreshCw size={14} /> 새로고침
                  </Button>
                </div>
              }
            />
          </div>
          <Panel className="min-w-0 p-5">
            <div className="flex min-w-0 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
              <Search size={18} />
              <TextInput
                value={query}
                placeholder="MCP 서버 검색 (예: GitHub, browser, filesystem)"
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && search()}
              />
              <Action onClick={search} disabled={searching}>
                {searching ? "검색 중" : "검색"}
              </Action>
            </div>
            <div className="rounded-xl bg-[#FAF7F1] p-3 text-xs leading-5 text-[#7C7469]">
              공식 Registry에서 실시간으로 가져옵니다. 서버 설치 전 명령과 배포
              출처를 확인하세요.
            </div>
            <div className="registry-results">
              {catalog.map((x) => (
                <div
                  className="flex min-w-0 items-start gap-3 rounded-xl border border-[#E7E0D5] bg-white p-4 hover:border-[#E6C9B8]"
                  key={x.name}
                >
                  <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-[#F7E8E1] text-[#C96442]">
                    <Server size={19} />
                  </span>
                  <div>
                    <strong>{x.name}</strong>
                    <p>{x.description || "MCP server"}</p>
                    <small>{x.packageName || x.url || "설치 정보 없음"}</small>
                  </div>
                  {(x.packageName || x.url) &&
                    (mcp.value.some((r: any) => r.name === x.name) ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          const rec = mcp.value.find((r: any) => r.name === x.name);
                          if (!rec) return;
                          await del("/api/mcp/" + rec.id);
                          await mcp.reload();
                        }}
                      >
                        취소
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        onClick={() =>
                          install({
                            name: x.name,
                            transport: x.packageName ? "stdio" : "http",
                            command: "npx",
                            args: x.packageName ? ["-y", x.packageName] : [],
                            endpoint: x.url,
                          })
                        }
                      >
                        설치
                      </Button>
                    ))}
                </div>
              ))}
              {!catalog.length && (
                <Empty
                  icon={Compass}
                  title="MCP를 검색해보세요"
                  description="검색된 서버의 설치 가능 패키지나 원격 URL을 보여줍니다."
                />
              )}
            </div>
          </Panel>
        </>
      )}
      {tab === "plugins" && (
        <>
          <div className="mb-3 flex flex-wrap items-end justify-between gap-3">
            <div>
              <h3 className="m-0 text-[15px] font-bold text-[#24211D]">
                OpenClaw 플러그인
              </h3>
              <p className="mt-1 text-[11px] text-[#8A8176]">
                OpenClaw 플러그인과 DOMINION 스킬 폴더의 SKILL.md를 읽습니다.{" "}
                {catalogSkills.value.total}개 발견 ·{" "}
                {catalogSkills.value.updatedAt
                  ? new Date(catalogSkills.value.updatedAt).toLocaleTimeString()
                  : "읽는 중"}
              </p>
            </div>
            <Button
              variant="secondary"
              onClick={() => void catalogSkills.reload()}
            >
              <RefreshCw size={14} /> 새로고침
            </Button>
          </div>
          <div className="mb-3 flex flex-wrap items-center gap-1.5">
            {catalogSources.map((source) => (
              <Button
                key={source}
                variant={catalogSource === source ? "primary" : "ghost"}
                onClick={() => setCatalogSource(source)}
                className="!px-3 !py-1.5 !text-[12px]"
              >
                {source === "all" ? "전체 소스" : source}
              </Button>
            ))}
          </div>
          <div className="mb-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_220px]">
            <div className="flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
              <Search size={17} className="text-[#A79C8E]" />
              <TextInput
                value={catalogQuery}
                onChange={(e) => setCatalogQuery(e.target.value)}
                placeholder="플러그인 검색 (이름, 카테고리)"
              />
            </div>
            <SelectInput
              value={catalogCategory}
              onChange={(e) => setCatalogCategory(e.target.value)}
            >
              <option value="all">전체 카테고리</option>
              {catalogCategories
                .filter((category) => category !== "all")
                .map((category) => (
                  <option key={category} value={category}>
                    {category}
                  </option>
                ))}
            </SelectInput>
          </div>
          <div className="mb-3 max-h-[440px] overflow-y-auto rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9]">
            {visibleCatalog.map((item) => {
              const installed = skills.value.some(
                (skill) => skill.name === item.name,
              );
              return (
                <div
                  key={item.id}
                  className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EFE9DF] px-4 py-3 last:border-0"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <strong className="text-[12px] text-[#3A342D]">
                        {item.name}
                      </strong>
                      <span className="rounded bg-[#F3EEE5] px-1.5 py-0.5 text-[10px] font-semibold text-[#7C7469]">
                        {item.source}
                      </span>
                      <span className="text-[10px] text-[#A79C8E]">
                        {item.category}
                      </span>
                    </div>
                    <p className="mt-1 line-clamp-2 text-[11px] leading-5 text-[#7C7469]">
                      {item.description}
                    </p>
                    {!item.available && (
                      <small className="mt-1 block text-[10px] text-amber-700">
                        실행 파일 필요: {(item.requires || []).join(", ")}
                      </small>
                    )}
                  </div>
                  <div className="shrink-0">
                    {installed ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          const target = skills.value.find(
                            (x) => x.name === item.name,
                          );
                          if (!target) return;
                          try {
                            await del("/api/skills/" + target.id);
                            await skills.reload();
                            setMessage(item.name + " 취소됨");
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        취소
                      </Button>
                    ) : !item.available ? (
                      <span className="rounded-md bg-amber-50 px-2 py-1 text-[10px] font-semibold text-amber-700">
                        도구 필요
                      </span>
                    ) : (
                      <Button
                        variant="secondary"
                        disabled={importing === item.id}
                        onClick={() => void importCatalogSkill(item)}
                      >
                        {importing === item.id ? "설치 중" : "설치"}
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
            {!visibleCatalog.length && (
              <Empty
                icon={BookOpenText}
                title="카탈로그가 비어 있습니다"
                description="OpenClaw 또는 Hermes 스킬 폴더를 확인하세요."
              />
            )}
          </div>
          <div className="mb-6 flex flex-wrap items-center justify-between gap-2 text-[11px] text-[#8A8176]">
            <span>
              {filteredCatalog.length}개 중 {visibleCatalog.length}개 표시
            </span>
            {visibleCatalog.length < filteredCatalog.length && (
              <Button
                variant="secondary"
                onClick={() => setCatalogLimit((limit) => limit + 30)}
              >
                더 보기
              </Button>
            )}
          </div>
          <div className="mb-4">
            <Subheading
              title="플러그인 검색 결과"
              extra={
                <span className="text-[11px] text-[#8A8176]">
                  {pluginRemote.length}개
                </span>
              }
            />
            {pluginRemote.length ? (
              <div className="mt-2 grid gap-2">
                {pluginRemote.map((x: any) => (
                  <div
                    key={x.name}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[#E7E0D5] bg-white p-3"
                  >
                    <div className="min-w-0">
                      <strong className="block truncate text-[12px] text-[#3A342D]">
                        {x.name}
                      </strong>
                      <small className="text-[10px] text-[#8A8176]">
                        {(x.description || "").slice(0, 90)}
                      </small>
                    </div>
                    {mcp.value.some((r: any) => r.name === x.name) ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          const rec = mcp.value.find((r: any) => r.name === x.name);
                          if (!rec) return;
                          await del("/api/mcp/" + rec.id);
                          await mcp.reload();
                        }}
                      >
                        취소
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        onClick={() =>
                          void install({
                            name: x.name,
                            transport: x.packageName ? "stdio" : "http",
                            command: "npx",
                            args: x.packageName ? ["-y", x.packageName] : [],
                            endpoint: x.url,
                          })
                        }
                      >
                        설치
                      </Button>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <p className="mt-2 text-[11px] text-[#8A8176]">
                검색어를 2자 이상 입력하면 레지스트리에서도 찾습니다.
              </p>
            )}
          </div>
          <div className="mb-4 flex flex-wrap items-center gap-1.5">
            {pluginCategories.map((category) => (
              <Button
                key={category}
                variant={pluginCategory === category ? "primary" : "ghost"}
                onClick={() => setPluginCategory(category)}
                className="!px-3 !py-1.5 !text-[12px]"
              >
                {category === "all" ? "전체" : category}
              </Button>
            ))}
          </div>
          <div className="mb-5 flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
            <Search size={17} className="text-[#A79C8E]" />
            <TextInput
              value={pluginQuery}
              onChange={(e) => setPluginQuery(e.target.value)}
              placeholder="플러그인 검색 (예: Git, SQLite, browser)"
            />
          </div>
          <Subheading
            title="추천 플러그인"
            extra={
              <span className="text-[11px] text-[#8A8176]">
                {filteredPresets.length}개
              </span>
            }
          />
          <div className="mb-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {filteredPresets.map((preset) => {
              const record = mcp.value.find((x) => x.name === preset.name);
              const connected = tools.filter(
                (t) => t.mcpId === record?.id && !t.error,
              ).length;
              return (
                <Panel
                  key={preset.id}
                  className="flex min-w-0 flex-col gap-3 p-5"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
                      <PackagePlus size={21} />
                    </div>
                    <StateTag online={Boolean(record)}>
                      {record
                        ? connected
                          ? "연결됨 · 도구 " + connected + "개"
                          : "등록됨 · 확인 필요"
                        : "미설치"}
                    </StateTag>
                  </div>
                  <h3>{preset.name}</h3>
                  <p>{preset.description}</p>
                  <small className="text-[10px] font-semibold uppercase tracking-wider text-[#9C9388]">
                    {preset.category}
                  </small>
                  <div className="mt-auto flex items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
                    <code className="truncate text-[10px] text-[#9C9388]">
                      {preset.package}
                    </code>
                    {record ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          try {
                            await del("/api/mcp/" + record.id);
                            await mcp.reload();
                            setMessage(preset.name + " 취소됨");
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        취소
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        disabled={installing === preset.id}
                        onClick={() => void installPreset(preset)}
                      >
                        {installing === preset.id ? "설치 중" : "설치"}
                      </Button>
                    )}
                  </div>
                </Panel>
              );
            })}
          </div>
          <Subheading
            title="추천 도구"
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
              const connected = tools.filter(
                (t) => t.mcpId === record?.id && !t.error,
              ).length;
              return (
                <Panel key={x.name} className="flex min-w-0 flex-col gap-3 p-5">
                  <div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
                    <Icon size={24} />
                  </div>
                  <h3>{x.name}</h3>
                  <p>{x.description}</p>
                  <div className="mt-auto flex items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3">
                    <a href={x.url} target="_blank" rel="noreferrer">
                      공식 안내 <ExternalLink size={13} />
                    </a>
                    {record ? (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={async () => {
                          try {
                            await del("/api/mcp/" + record.id);
                            await mcp.reload();
                            setMessage(x.name + " 취소됨");
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        취소
                      </Button>
                    ) : (
                      <Button
                        variant="secondary"
                        className="shrink-0 whitespace-nowrap"
                        onClick={() => install(x)}
                      >
                        설치
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
            title="연결된 MCP 서버"
            extra={
              <Action variant="secondary" onClick={discover}>
                <Activity size={16} /> 도구 검색
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
                    {x.transport === "stdio"
                      ? x.command + " " + (x.args || []).join(" ")
                      : x.url}
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
                    if (ok) return "도구 " + ok + "개 연결됨";
                    if (own.some((t) => t.error)) return "연결 실패";
                    return tools.length ? "연결 확인 필요" : "확인 중";
                  })()}
                </StateTag>
                <Button
                  size="icon"
                  variant="ghost"
                  title="설정"
                  onClick={() => {
                    setEdit(x);
                    setArgText((x.args || []).join("\n"));
                  }}
                >
                  <Settings2 size={17} />
                </Button>
                <Button
                  size="icon"
                  variant="ghost"
                  title="제거"
                  onClick={async () => {
                    if (confirm(x.name + " 연결을 제거할까요?")) {
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
                  title="MCP 서버가 없습니다"
                  description="탐색 탭에서 필요한 기능을 등록하세요."
                />
              </Panel>
            )}
          </div>
          {tools.length > 0 && (
            <Panel className="grid gap-3">
              <Subheading
                title="검색된 실제 도구"
                extra={<span>{tools.filter((t) => !t.error).length}개</span>}
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
                <small>MCP CONNECTION</small>
                <h2>서버 등록</h2>
              </div>
              <Button size="icon" variant="ghost" onClick={() => setEdit(null)}>
                <X size={18} />
              </Button>
            </div>
            <div className="grid gap-4 p-5">
              <FormField label="표시 이름">
                <TextInput
                  value={edit.name}
                  onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                />
              </FormField>
              <FormField label="연결 방식">
                <SelectInput
                  value={edit.transport}
                  onChange={(e) =>
                    setEdit({ ...edit, transport: e.target.value })
                  }
                >
                  <option value="stdio">stdio · 로컬 실행</option>
                  <option value="http">HTTP · 원격 서버</option>
                </SelectInput>
              </FormField>
              {edit.transport === "http" ? (
                <>
                  <FormField label="MCP 서버 URL">
                    <TextInput
                      value={edit.url || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, url: e.target.value })
                      }
                    />
                  </FormField>
                  <FormField label="인증 Bearer Token">
                    <TextInput
                      type="password"
                      value={edit.token || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, token: e.target.value })
                      }
                      placeholder="선택사항"
                    />
                  </FormField>
                </>
              ) : (
                <>
                  <FormField label="실행 파일">
                    <TextInput
                      value={edit.command || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, command: e.target.value })
                      }
                    />
                  </FormField>
                  <FormField label="인수 (한 줄에 하나씩)">
                    <TextArea
                      rows={4}
                      className="min-h-28"
                      value={argText}
                      onChange={(e) => setArgText(e.target.value)}
                    />
                  </FormField>
                </>
              )}
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={() => setEdit(null)}>
                취소
              </Action>
              <Action onClick={saveMcp}>
                <Check size={16} /> 저장
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
                <small>SKILL EDITOR</small>
                <h2>스킬 작성 · 검토</h2>
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
              <FormField label="이름">
                <TextInput
                  value={skillEdit.name || ""}
                  onChange={(e) =>
                    setSkillEdit({ ...skillEdit, name: e.target.value })
                  }
                />
              </FormField>
              <FormField label="실행 지침">
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
                에이전트 시스템 지침에 적용
              </label>
            </div>
            <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
              <Action variant="secondary" onClick={() => setSkillEdit(null)}>
                취소
              </Action>
              <Action onClick={saveSkill}>
                <Check size={16} /> 스킬 저장
              </Action>
            </div>
          </DialogPanel>
        </DialogBackdrop>
      )}
    </div>
  );
}
