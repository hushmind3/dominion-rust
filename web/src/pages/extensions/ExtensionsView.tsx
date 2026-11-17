//! This file contains only the extensions UI. Data loading and mutations are
//! owned by useExtensionsController, shared by all four tabs.
import { DialogBackdrop, DialogPanel } from "../components/Dialog";
import { SkillsPanel } from "./extensions/SkillsPanel";
import { t, useLocale } from "../i18n";
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
import type {useExtensionsController} from './ExtensionsController';

export function ExtensionsView(state:ReturnType<typeof useExtensionsController>){
  const {loc,mcp,skills,presets,catalogSkills,repo,setRepo,repoBusy,setRepoBusy,pluginRemote,setPluginRemote,localTab,setTab,query,setQuery,catalog,setCatalog,searching,setSearching,installing,setInstalling,error,setError,message,setMessage,tab,skillQuery,setSkillQuery,skillCategory,setSkillCategory,pluginQuery,setPluginQuery,pluginCategory,setPluginCategory,catalogQuery,setCatalogQuery,catalogSource,setCatalogSource,catalogCategory,setCatalogCategory,catalogLimit,setCatalogLimit,importing,setImporting,templates,skillCategories,filteredSkills,pluginCategories,filteredPresets,catalogItems,catalogSources,catalogCategories,filteredCatalog,visibleCatalog,edit,setEdit,argText,setArgText,skillEdit,setSkillEdit,tools,setTools,install,installPreset,loadCatalog,search,saveMcp,saveSkill,addSkillTemplate,importCatalogSkill,importRepo,discover,embedded,featured,StatusLine}=state;
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
          variant={tab === "plugins" ? "primary" : "ghost"}
          onClick={() => setTab("plugins")}
        >
          <PackagePlus size={17} /> {t('ext.tab.plugins')}
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
          <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
            <Subheading
              title={t('ext.registry')}
              extra={
                <div className="flex items-center gap-2">
                  <span className="text-[11px] text-[#8A8176]">
                    {t('ext.realtime', { count: catalog.length })}
                  </span>
                  <Button
                    variant="secondary"
                    onClick={() => void loadCatalog(query.trim())}
                    disabled={searching}
                  >
                    <RefreshCw size={14} /> {t('ext.refresh')}
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
                placeholder={t('ext.searchPlaceholder')}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && search()}
              />
              <Action onClick={search} disabled={searching}>
                {searching ? t('ext.searching') : t('ext.search')}
              </Action>
            </div>
            <div className="rounded-xl bg-[#FAF7F1] p-3 text-xs leading-5 text-[#7C7469]">
              {t('ext.registryNote')}
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
                    <small>{x.packageName || x.url || t('ext.noInstallInfo')}</small>
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
                        {t('ext.cancel')}
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
                        {t('ext.install')}
                      </Button>
                    ))}
                </div>
              ))}
              {!catalog.length && (
                <Empty
                  icon={Compass}
                  title={t('ext.emptySearch')}
                  description={t('ext.emptySearchDesc')}
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
                {t('ext.pluginSkillsTitle')}
              </h3>
              <p className="mt-1 text-[11px] text-[#8A8176]">
                {t('ext.pluginSkillsDesc')}{" "}
                {t('ext.foundCount', { count: catalogSkills.value.total })} ·{" "}
                {catalogSkills.value.updatedAt
                  ? new Date(catalogSkills.value.updatedAt).toLocaleTimeString(loc === 'en' ? 'en-US' : 'ko-KR')
                  : t('ext.reading')}
              </p>
            </div>
            <Button
              variant="secondary"
              onClick={() => void catalogSkills.reload()}
            >
              <RefreshCw size={14} /> {t('ext.refresh')}
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
                {source === "all" ? t('ext.allSources') : source}
              </Button>
            ))}
          </div>
          <div className="mb-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_220px]">
            <div className="flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
              <Search size={17} className="text-[#A79C8E]" />
              <TextInput
                value={catalogQuery}
                onChange={(e) => setCatalogQuery(e.target.value)}
                placeholder={t('ext.pluginSearch')}
              />
            </div>
            <SelectInput
              value={catalogCategory}
              onChange={(e) => setCatalogCategory(e.target.value)}
            >
              <option value="all">{t('ext.allCategories')}</option>
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
                        {t('ext.requiresBinary', { list: (item.requires || []).join(', ') })}
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
                            setMessage(t('ext.cancelled', { name: item.name }));
                          } catch (e) {
                            setError((e as Error).message);
                          }
                        }}
                      >
                        {t('ext.cancel')}
                      </Button>
                    ) : !item.available ? (
                      <span className="rounded-md bg-amber-50 px-2 py-1 text-[10px] font-semibold text-amber-700">
                        {t('ext.requiredTool')}
                      </span>
                    ) : (
                      <Button
                        variant="secondary"
                        disabled={importing === item.id}
                        onClick={() => void importCatalogSkill(item)}
                      >
                        {importing === item.id ? t('ext.installing') : t('ext.install')}
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
            {!visibleCatalog.length && (
              <Empty
                icon={BookOpenText}
                title={t('ext.catalogEmpty')}
                description={t('ext.catalogHint')}
              />
            )}
          </div>
          <div className="mb-6 flex flex-wrap items-center justify-between gap-2 text-[11px] text-[#8A8176]">
            <span>
              {t('ext.showing', { visible: visibleCatalog.length, total: filteredCatalog.length })}
            </span>
            {visibleCatalog.length < filteredCatalog.length && (
              <Button
                variant="secondary"
                onClick={() => setCatalogLimit((limit) => limit + 30)}
              >
                {t('ext.more')}
              </Button>
            )}
          </div>
          <div className="mb-4">
            <Subheading
              title={t('ext.pluginResults')}
              extra={
                <span className="text-[11px] text-[#8A8176]">
                  {t('ext.count', { count: pluginRemote.length })}
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
                        {t('ext.cancel')}
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
                        {t('ext.install')}
                      </Button>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <p className="mt-2 text-[11px] text-[#8A8176]">
                {t('ext.remoteHint')}
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
                {category === "all" ? t('ext.all') : category}
              </Button>
            ))}
          </div>
          <div className="mb-5 flex items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] px-3 py-2">
            <Search size={17} className="text-[#A79C8E]" />
            <TextInput
              value={pluginQuery}
              onChange={(e) => setPluginQuery(e.target.value)}
              placeholder={t('ext.pluginSearchExample')}
            />
          </div>
          <Subheading
            title={t('ext.recommendedPlugins')}
            extra={
              <span className="text-[11px] text-[#8A8176]">
                {t('ext.count', { count: filteredPresets.length })}
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
                          ? t('ext.connectedTools', { count: connected })
                          : t('ext.registeredCheck')
                        : t('ext.notInstalled')}
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
                            setMessage(t('ext.cancelled', { name: preset.name }));
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
                        disabled={installing === preset.id}
                        onClick={() => void installPreset(preset)}
                      >
                        {installing === preset.id ? t('ext.installing') : t('ext.install')}
                      </Button>
                    )}
                  </div>
                </Panel>
              );
            })}
          </div>
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
              const connected = tools.filter(
                (t) => t.mcpId === record?.id && !t.error,
              ).length;
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
                    setArgText((x.args || []).join("\n"));
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
              <FormField label={t('ext.transport')}>
                <SelectInput
                  value={edit.transport}
                  onChange={(e) =>
                    setEdit({ ...edit, transport: e.target.value })
                  }
                >
                  <option value="stdio">{t('ext.stdio')}</option>
                  <option value="http">{t('ext.http')}</option>
                </SelectInput>
              </FormField>
              {edit.transport === "http" ? (
                <>
                  <FormField label={t('ext.url')}>
                    <TextInput
                      value={edit.url || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, url: e.target.value })
                      }
                    />
                  </FormField>
                  <FormField label={t('ext.bearer')}>
                    <TextInput
                      type="password"
                      value={edit.token || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, token: e.target.value })
                      }
                      placeholder={t('ext.optional')}
                    />
                  </FormField>
                </>
              ) : (
                <>
                  <FormField label={t('ext.executable')}>
                    <TextInput
                      value={edit.command || ""}
                      onChange={(e) =>
                        setEdit({ ...edit, command: e.target.value })
                      }
                    />
                  </FormField>
                  <FormField label={t('ext.args')}>
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
