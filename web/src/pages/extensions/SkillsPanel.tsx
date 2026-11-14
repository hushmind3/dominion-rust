import {del,post} from '../../api'
import {Action,Empty,Panel,StateTag,Subheading} from '../../components/UI'
import {Button,TextInput} from '../../components/Primitives'
import {ArrowRight,BookOpenText,Check,Plus,Search,Trash2} from 'lucide-react'
import {t,useLocale} from '../../i18n'

type Props={
 repo:string;setRepo:(v:string)=>void;repoBusy:boolean;importRepo:()=>Promise<void>;
 skills:any;addSkillTemplate:(t:any)=>Promise<void>;filteredSkills:any[];
 skillCategories:string[];skillCategory:string;setSkillCategory:(v:string)=>void;
 skillQuery:string;setSkillQuery:(v:string)=>void;setSkillEdit:(v:any)=>void;
 setError:(v:string)=>void;setMessage:(v:string)=>void;
}

export function SkillsPanel(p:Props){
 useLocale()
 const {repo,setRepo,repoBusy,importRepo,skills,addSkillTemplate,filteredSkills,skillCategories,skillCategory,setSkillCategory,skillQuery,setSkillQuery,setSkillEdit,setError,setMessage}=p
 return (
  <>
  <>
    <Panel className="mb-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <TextInput
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          placeholder={t('sp.repoPlaceholder')}
        />
        <Action onClick={() => void importRepo()} disabled={repoBusy}>
          {repoBusy ? t('sp.importing') : t('sp.import')}
        </Action>
      </div>
    </Panel>
    <Subheading
      title={t('sp.templates')}
      extra={
        <span className="text-[11px] text-[#8A8176]">
          {t('sp.count', { count: filteredSkills.length })}
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
          {category === "all" ? t('sp.all') : category}
        </Button>
      ))}
    </div>
    <div className="mb-5 flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
      <Search size={17} className="text-[#A79C8E]" />
      <TextInput
        value={skillQuery}
        onChange={(e) => setSkillQuery(e.target.value)}
        placeholder={t('sp.searchPlaceholder')}
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
                  {t('sp.cancel')}
                </Button>
              ) : (
                <Button variant="secondary" onClick={() => void addSkillTemplate(template)}>
                  {t('sp.install')}
                </Button>
              )}
            </div>
          </Panel>
        );
      })}
    </div>
    <Subheading
      title={t('sp.mine')}
      extra={
        <Action
          onClick={() =>
            setSkillEdit({ name: "", instructions: "", enabled: true })
          }
        >
          <Plus size={16} /> {t('sp.write')}
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
                {t('sp.claudeBasis')}
              </span>
            )}
          </h3>
          <p>{x.instructions.slice(0, 170)}</p>
          <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
            <StateTag online={x.enabled}>
              {x.enabled ? t('sp.active') : t('sp.suggested')}
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
                {x.enabled ? t('sp.disable') : t('sp.enable')}
              </Button>
              <a
                href={"/api/skills/" + x.id + "/export"}
                download={x.name + ".md"}
                className="inline-flex items-center gap-1 rounded-lg border border-[#E1D9CC] px-2 py-1 text-[11px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]"
              >
                {t('sp.export')}
              </a>
              {x.enabled&&x.status!=="draft"&&<a
                href={"#/agents?skill="+encodeURIComponent(x.id)}
                className="inline-flex items-center gap-1 rounded-lg border border-[#D8C8AF] px-2 py-1 text-[11px] font-semibold text-[#A94E30] hover:bg-[#F7F2EA]"
              >에이전트에 배정 <ArrowRight size={13}/></a>}
              <Button onClick={() => setSkillEdit(x)}>
                {t('sp.edit')} <ArrowRight size={15} />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                title={t('sp.delete')}
                onClick={async () => {
                  if (!confirm(t('sp.confirmDelete', { name: x.name }))) return;
                  try {
                    await del("/api/skills/" + x.id);
                    await skills.reload();
                    setMessage(t('ext.skillDeleted', { name: x.name }));
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
            title={t('sp.empty')}
            description={t('sp.emptyDesc')}
          />
        </Panel>
      )}
    </div>
  </>

  </>
 )
}
