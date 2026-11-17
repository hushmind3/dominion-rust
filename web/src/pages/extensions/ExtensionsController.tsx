import { DialogBackdrop, DialogPanel } from "../../components/Dialog";
import { SkillsPanel } from "./SkillsPanel";
import { t, useLocale } from "../../i18n";
import {
  Button,
  TextInput,
  TextArea,
  SelectInput,
} from "../../components/Primitives";
import { useEffect, useRef, useState } from "react";
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
import { del, get, post } from "../../api";
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
} from "../../components/UI";
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
    descriptionEn: "Browse web pages, click, type and capture screenshots",
    icon: Globe2,
    transport: "stdio",
    command: "npx",
    args: ["-y", "@playwright/mcp@latest"],
    url: "https://playwright.dev/mcp/installation",
  },
  {
    name: "Hugging Face",
    description: "모델·데이터셋을 검색하는 원격 MCP",
    descriptionEn: "Remote MCP for searching models and datasets",
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
export type ExtensionTab='market'|'installed'|'plugins'|'skills'
export function useExtensionsController({section,embedded=false}:{section?:ExtensionTab;embedded?:boolean}={}) {
  const loc = useLocale();
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
  const [localTab, setTab] = useState<ExtensionTab>(
      "market",
    ),
    [query, setQuery] = useState(""),
    [catalog, setCatalog] = useState<RegistryItem[]>([]),
    [searching, setSearching] = useState(false),
    [installing, setInstalling] = useState(""),
    [error, setError] = useState(""),
    [message, setMessage] = useState("");
  const tab=section??localTab;
  const [skillQuery, setSkillQuery] = useState(""),
    [skillCategory, setSkillCategory] = useState("all");
  const [pluginQuery, setPluginQuery] = useState(""),
    [pluginCategory, setPluginCategory] = useState("all");
  const [catalogQuery, setCatalogQuery] = useState(""),
    [catalogSource, setCatalogSource] = useState("all"),
    [catalogCategory, setCatalogCategory] = useState("all"),
    [catalogLimit, setCatalogLimit] = useState(30),
    [importing, setImporting] = useState("");
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
        t('ext.confirmRegister'),
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
        t('ext.registeredHint'),
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
      t('ext.installHint', { name: preset.name }),
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
            t('ext.connectedToolsMsg', { name: preset.name, count: own.length }),
          );
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
      setMessage(t('ext.registeredCheckMsg', { name: preset.name }));
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
      setError(t('ext.searchMin'));
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
      setMessage(t('ext.skillAdded', { name: template.name }));
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
      setMessage(t('ext.skillImported', { source: item.source, name: item.name }));
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
      setMessage(t('ext.skillImportedRepo', { repo: r.repo, count: r.added }));
      await skills.reload();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRepoBusy(false);
    }
  };
  useEffect(() => {
    const q = pluginQuery.trim();
    if (tab!=="plugins" || q.length < 2) {
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
  }, [pluginQuery,tab]);
  const discover = async (silent = false) => {
    try {
      setTools(await get("/api/mcp/discover"));
      if (!silent) setMessage(t('ext.toolsFound'));
    } catch (e) {
      if (!silent) setError((e as Error).message);
    }
  };
  useEffect(() => {
    if(tab==="plugins"||tab==="installed")void discover(true);
  }, [tab]);
  const initialCatalogLoaded=useRef(false);
  useEffect(() => {
    if((tab==="market"||tab==="plugins")&&!initialCatalogLoaded.current){
      initialCatalogLoaded.current=true;
      void loadCatalog();
    }
  }, [tab]);
  return {loc,mcp,skills,presets,catalogSkills,repo,setRepo,repoBusy,setRepoBusy,pluginRemote,setPluginRemote,localTab,setTab,query,setQuery,catalog,setCatalog,searching,setSearching,installing,setInstalling,error,setError,message,setMessage,tab,skillQuery,setSkillQuery,skillCategory,setSkillCategory,pluginQuery,setPluginQuery,pluginCategory,setPluginCategory,catalogQuery,setCatalogQuery,catalogSource,setCatalogSource,catalogCategory,setCatalogCategory,catalogLimit,setCatalogLimit,importing,setImporting,templates,skillCategories,filteredSkills,pluginCategories,filteredPresets,catalogItems,catalogSources,catalogCategories,filteredCatalog,visibleCatalog,edit,setEdit,argText,setArgText,skillEdit,setSkillEdit,tools,setTools,install,installPreset,loadCatalog,search,saveMcp,saveSkill,addSkillTemplate,importCatalogSkill,importRepo,discover,embedded,featured,StatusLine};
}
