import {Action,Empty,Panel,StateTag,Subheading} from '../../components/UI'
import {Button,TextInput} from '../../components/Primitives'
import {ArrowRight,BookOpenText,Check,Plus,Search,Trash2} from 'lucide-react'

type Props={
 repo:string;setRepo:(v:string)=>void;repoBusy:boolean;importRepo:()=>Promise<void>;
 skills:any;addSkillTemplate:(t:any)=>Promise<void>;filteredSkills:any[];
 skillCategories:string[];skillCategory:string;setSkillCategory:(v:string)=>void;
 skillQuery:string;setSkillQuery:(v:string)=>void;setSkillEdit:(v:any)=>void;
 setError:(v:string)=>void;setMessage:(v:string)=>void;
}

export function SkillsPanel(p:Props){
 const {repo,setRepo,repoBusy,importRepo,skills,addSkillTemplate,filteredSkills,skillCategories,skillCategory,setSkillCategory,skillQuery,setSkillQuery,setSkillEdit,setError,setMessage}=p
 const del=(url:string)=>fetch(url,{method:'DELETE'}).then(async r=>{if(!r.ok)throw Error('HTTP '+r.status);return r.json()})
 const post=(url:string,body:any)=>fetch(url,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)}).then(async r=>{if(!r.ok)throw Error('HTTP '+r.status);return r.json()})
 return (
  <>
  <>
    <Panel className="mb-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <TextInput
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          placeholder="GitHub 저장소 (예: anthropics/skills)"
        />
        <Action onClick={() => void importRepo()} disabled={repoBusy}>
          {repoBusy ? "가져오는 중" : "스킬 가져오기"}
        </Action>
      </div>
    </Panel>
    <Subheading
      title="기본 스킬 템플릿"
      extra={
        <span className="text-[11px] text-[#8A8176]">
          {filteredSkills.length}개
        </span>
      }
    />
    <div className="mb-4 flex flex-wrap items-center gap-1.5">
      {skillCategories.map((category) => (
        <Button
          key={category}
          variant={skillCategory === category ? "primary" : "ghost"}
          onClick={() => setSkillCategory(category)}
          className="!px-3 !py-1.5 !text-[12px]"
        >
          {category === "all" ? "전체" : category}
        </Button>
      ))}
    </div>
    <div className="mb-5 flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
      <Search size={17} className="text-[#A79C8E]" />
      <TextInput
        value={skillQuery}
        onChange={(e) => setSkillQuery(e.target.value)}
        placeholder="스킬 검색 (예: 리서치, Git, 보안)"
      />
    </div>
    <div className="mb-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      {filteredSkills.map((template) => {
        const exists = skills.value.some((x:any) => x.name === template.name);
        return (
          <Panel
            key={template.name}
            className="flex min-w-0 flex-col gap-3 p-5"
          >
            <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
              <BookOpenText size={20} />
            </div>
            <h3>{template.name}</h3>
            <p>{template.instructions}</p>
            <div className="mt-auto border-t border-[#EFE9DF] pt-3">
              {exists ? (
                <Button
                  variant="secondary"
                  onClick={async () => {
                    const target = skills.value.find((x: any) => x.name === template.name);
                    if (!target) return;
                    await del("/api/skills/" + target.id);
                    await skills.reload();
                  }}
                >
                  취소
                </Button>
              ) : (
                <Button variant="secondary" onClick={() => void addSkillTemplate(template)}>
                  설치
                </Button>
              )}
            </div>
          </Panel>
        );
      })}
    </div>
    <Subheading
      title="내 스킬"
      extra={
        <Action
          onClick={() =>
            setSkillEdit({ name: "", instructions: "", enabled: true })
          }
        >
          <Plus size={16} /> 스킬 작성
        </Action>
      }
    />
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      {skills.value.map((x:any) => (
        <Panel key={x.id} className="flex min-w-0 flex-col gap-3 p-5">
          <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]">
            <BookOpenText size={21} />
          </div>
          <h3>
            {x.name}
            {String(x.source || "").includes("anthropics") && (
              <span className="ml-2 rounded bg-[#FFF3E8] px-1.5 py-0.5 text-[10px] font-semibold text-[#A94E30]">
                Claude 기준
              </span>
            )}
          </h3>
          <p>{x.instructions.slice(0, 170)}</p>
          <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
            <StateTag online={x.enabled}>
              {x.enabled ? "활성화" : "제안 / 비활성"}
            </StateTag>
            <div className="flex items-center gap-1">
              <Button
                variant={x.enabled ? "ghost" : "primary"}
                onClick={async () => {
                  try {
                    await post("/api/skills", {
                      ...x,
                      enabled: !x.enabled,
                    });
                    skills.reload();
                  } catch (e) {
                    setError((e as Error).message);
                  }
                }}
              >
                {x.enabled ? "비활성화" : "활성화"}
              </Button>
              <a
                href={"/api/skills/" + x.id + "/export"}
                download={x.name + ".md"}
                className="inline-flex items-center gap-1 rounded-lg border border-[#E1D9CC] px-2 py-1 text-[11px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"
              >
                내보내기
              </a>
              <Button onClick={() => setSkillEdit(x)}>
                편집 <ArrowRight size={15} />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                title="삭제"
                onClick={async () => {
                  if (!confirm(x.name + " 스킬을 삭제할까요?")) return;
                  try {
                    await del("/api/skills/" + x.id);
                    await skills.reload();
                    setMessage(x.name + " 삭제됨");
                  } catch (e) {
                    setError((e as Error).message);
                  }
                }}
              >
                <Trash2 size={16} />
              </Button>
            </div>
          </div>
        </Panel>
      ))}
      {!skills.value.length && (
        <Panel>
          <Empty
            icon={BookOpenText}
            title="아직 스킬이 없습니다"
            description="에이전트가 제안한 스킬을 검토하거나 직접 작성할 수 있습니다."
          />
        </Panel>
      )}
    </div>
  </>

  </>
 )
}
