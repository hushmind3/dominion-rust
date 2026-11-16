export type RunStatus = 'queued' | 'running' | 'awaiting_approval' | 'paused' | 'completed' | 'failed' | 'cancelled';
export interface TimelineEvent {
  seq: number; kind: string; message: string; detail: unknown | null; at_ms: number;
}
export interface ToolCall { id: string; name: string; arguments: Record<string, unknown> }
export interface Run {
  id: string; instruction: string; status: RunStatus; plan: string | null; answer: string | null;
  messages: unknown[]; events: TimelineEvent[]; queued_calls: ToolCall[];
  pending_approval: ToolCall | null; approval_decision: boolean | null; in_flight: ToolCall | null;
  steps: number; input_tokens: number; cached_input_tokens?: number; cache_write_tokens?: number;
  output_tokens: number; cost_krw: number; context_chars_avoided?: number; tool_schema_chars_avoided?: number;
  repository?: string | null; base_commit?: string | null; parent_run_id?: string | null;
  read_only?: boolean; reserved_budget_krw?: number; delegated_spent_krw?: number;
  settled_child_ids?: string[]; waiting_for_children?: boolean; cancel_after_tool?: boolean; read_batch_in_flight?: ToolCall[];
  budget_krw: number; max_steps: number; created_at_ms: number; updated_at_ms: number;
}
export interface ModelConfig {
  model: string; economy_model: string; configured: boolean; shell_enabled: boolean; strategy_learning_enabled?: boolean; max_steps: number; context_max_chars?: number; mcp_server_count: number; max_parallel_reads?: number; denied_tools?: string[]; tool_result_max_chars?: number;
}
export interface CreateRun {
  instruction: string; budget_krw?: number; max_steps?: number;
}
export interface DelegateRun extends CreateRun { read_only?: boolean }
export interface MemoryEntry {
  id: string; scope: string; title: string; content: string; created_at_ms: number; updated_at_ms: number;
}
export interface OwnerProfile { name: string; avatar: string; updated_at_ms: number }
/** Exact 2026 main frontend view schemas backed by Rust state. */
export interface MainAgent {
  id:string; name:string; description:string; systemPrompt:string; providerId:string;
  model:string; color:string; role:string; aliases:string[]; avatarUrl:string;
  reasoningEffort?:string|null; thinking?:boolean|null;
}
export interface MainSession {
  id:string; title:string; agentId:string; providerId:string; model:string;
  createdAt:string; updatedAt:string;
}
export interface MainMessage {
  id:string; session_id:string; role:string; content:string; created_at:string;
  agent_id?:string; metadata?:Record<string,unknown>;
}
export interface MainProvider {
  id:string; name:string; kind:string; model:string; enabled:boolean;
  baseUrl:string; local:boolean; hasKey:boolean;
}
export interface MainExecution {
  id:string; instruction:string; status:string;
  checkpoint?:{rounds:number;toolCalls:number;pendingCall?:ToolCall|null;answer?:string|null};
}

export interface SkillInfo { name: string; path: string }

export interface Participant { name: string; role: string; economy: boolean }
export interface Utterance { speaker: string; content: string; at_ms: number; model: string | null }
export interface GroupRoom {
  id: string; topic: string; participants: Participant[]; messages: Utterance[];
  status: 'open' | 'finished'; rounds: number; max_rounds: number;
  cost_krw: number; budget_krw: number; created_at_ms: number; updated_at_ms: number;
}
export interface CreateGroup {
  topic: string; participants: Participant[]; max_rounds?: number; budget_krw?: number;
}
export interface StrategyEvidence {
  source_id: string; source: 'agent'; repository: string;
  goal: string; principle: string; when_to_use: string; failure_mode: string;
  files: string[]; outcome: 'verified';
  baseline_score: number | null; resulting_score: number | null;
  attempts: number; cost_krw: number; updated_at_ms: number;
}
export interface LegacySqlitePreview {
  sourceAgents:number;sourceSessions:number;sourceMessages:number;
  importableAgents:number;importableSessions:number;
  alreadyPresentAgents:number;alreadyPresentSessions:number;
  skippedBoundSessions:number;skippedArchivedSessions:number;note:string;
}
export interface LegacySqliteImport {
  importedAgents:number;importedSessions:number;importedMessages:number;
  existingAgents:number;existingSessions:number;
  skippedBoundSessions:number;skippedArchivedSessions:number;
}
export interface ClientOptions {
  baseUrl?: string; token?: string; fetch?: typeof globalThis.fetch;
}
export class DominionError extends Error {
  constructor(public readonly status: number, message: string) { super(message); this.name = 'DominionError'; }
}
export class Dominion {
  readonly baseUrl: string;
  private readonly token?: string;
  private readonly fetcher: typeof fetch;
  constructor(options: ClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? 'http://127.0.0.1:4317').replace(/\/+$/, '');
    this.token = options.token;
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
  }
  async request<T>(method: 'GET'|'POST'|'DELETE', path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    const headers: Record<string, string> = {};
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (this.token) headers.Authorization = 'Bearer ' + this.token;
    const response = await this.fetcher(this.baseUrl + '/api/v1' + path, {
      method, headers, signal, body: body === undefined ? undefined : JSON.stringify(body)
    });
    const payload = await response.json().catch(() => ({})) as { error?: string };
    if (!response.ok) throw new DominionError(response.status, payload.error ?? response.statusText);
    return payload as T;
  }

  /** The unchanged main React UI's /api contract, not a second agent runtime. */
  async frontendRequest<T>(method:'GET'|'POST'|'DELETE',path:string,body?:unknown):Promise<T>{
    const headers:Record<string,string>={};
    if(body!==undefined)headers['Content-Type']='application/json';
    if(this.token)headers.Authorization='Bearer '+this.token;
    const response=await this.fetcher(this.baseUrl+'/api'+path,{
      method,headers,body:body===undefined?undefined:JSON.stringify(body)
    });
    const value=await response.json().catch(()=>({})) as {error?:string};
    if(!response.ok)throw new DominionError(response.status,value.error??response.statusText);
    return value as T;
  }
  /** Active main-frontend functionality, mapped onto actual native Rust services. */
  readonly frontend={
    agents:{
      list:()=>this.frontendRequest<MainAgent[]>('GET','/agents'),
      save:(record:Partial<MainAgent>&{name:string})=>this.frontendRequest<MainAgent>('POST','/agents',record),
      remove:(id:string)=>this.frontendRequest<{ok:boolean}>('DELETE','/agents/'+encodeURIComponent(id)),
    },
    sessions:{
      list:()=>this.frontendRequest<MainSession[]>('GET','/sessions'),
      create:(input:{title?:string;agentId?:string;providerId?:string;model?:string})=>
        this.frontendRequest<MainSession>('POST','/sessions',input),
      messages:(id:string)=>this.frontendRequest<MainMessage[]>('GET','/sessions/'+encodeURIComponent(id)+'/messages'),
      rename:(id:string,title:string)=>this.frontendRequest<MainSession>('POST','/sessions/'+encodeURIComponent(id)+'/rename',{title}),
      clear:(id:string)=>this.frontendRequest<{ok:boolean}>('POST','/sessions/'+encodeURIComponent(id)+'/clear',{}),
      fork:(id:string)=>this.frontendRequest<MainSession>('POST','/sessions/'+encodeURIComponent(id)+'/fork',{}),
      cancel:(id:string)=>this.frontendRequest<{cancelled:boolean}>('POST','/sessions/'+encodeURIComponent(id)+'/cancel',{}),
    },
    providers:{
      list:()=>this.frontendRequest<MainProvider[]>('GET','/providers'),
      models:(id:string)=>this.frontendRequest<{id:string}[]>('GET','/providers/'+encodeURIComponent(id)+'/models'),
    },
    executions:{
      list:()=>this.frontendRequest<MainExecution[]>('GET','/sdk/runs'),
      create:(input:{instruction:string;providerId?:string;model?:string;maxRounds?:number})=>
        this.frontendRequest<MainExecution>('POST','/sdk/runs',input),
      get:(id:string)=>this.frontendRequest<MainExecution>('GET','/sdk/runs/'+encodeURIComponent(id)),
      cancel:(id:string)=>this.frontendRequest<MainExecution>('POST','/sdk/runs/'+encodeURIComponent(id)+'/cancel',{}),
      resume:(id:string)=>this.frontendRequest<MainExecution>('POST','/sdk/runs/'+encodeURIComponent(id)+'/resume',{}),
    },
    legacySqlite:{
      preview:(path:string)=>this.frontendRequest<LegacySqlitePreview>('POST','/system/legacy-sqlite/preview',{path}),
      import:(path:string)=>this.frontendRequest<LegacySqliteImport>('POST','/system/legacy-sqlite/import',{path,confirm:true}),
    },
    status:()=>this.frontendRequest<{agents:number;providers:number;jobs:number;executions:number}>('GET','/status'),
  };
  get health(): Promise<{ok: boolean; name: string; api: string}> {return this.request('GET','/health');}
  readonly owner = {
    get: () => this.request<OwnerProfile>('GET','/profile'),
    save: (profile:{name:string;avatar:string}) => this.request<OwnerProfile>('POST','/profile',profile),
  };
  get config(): Promise<ModelConfig> {return this.request('GET','/config');}
  readonly runs = {
    create: (input: CreateRun) => this.request<Run>('POST','/runs',input),
    fromRepository: (input: CreateRun & {repository:string}) => this.request<Run>('POST','/runs/from-repository',input),
    list: () => this.request<Run[]>('GET','/runs'),
    get: (id: string) => this.request<Run>('GET','/runs/'+encodeURIComponent(id)),
    resume: (id: string) => this.request<{ok:boolean}>('POST','/runs/'+encodeURIComponent(id)+'/resume',{}),
    cancel: (id: string) => this.request<Run>('POST','/runs/'+encodeURIComponent(id)+'/cancel',{}),
    approve: (id: string, allow: boolean) => this.request<{ok:boolean}>('POST','/runs/'+encodeURIComponent(id)+'/approval',{allow}),
    instruct: (id: string, instruction: string) => this.request<Run>('POST','/runs/'+encodeURIComponent(id)+'/directive',{instruction}),
    reconcile: (id: string, input: {succeeded: boolean; result: string}) => this.request<Run>('POST','/runs/'+encodeURIComponent(id)+'/reconcile',input),
    subagents: (id: string) => this.request<Run[]>('GET','/runs/'+encodeURIComponent(id)+'/subagents'),
    delegate: (id: string, input: DelegateRun) => this.request<Run>('POST','/runs/'+encodeURIComponent(id)+'/subagents',input),
    skills: (id: string) => this.request<SkillInfo[]>('GET','/runs/'+encodeURIComponent(id)+'/skills'),
    skill: (id: string,name: string) => this.request<{name:string;content:string}>('GET','/runs/'+encodeURIComponent(id)+'/skills/'+encodeURIComponent(name)),
    files: (id: string) => this.request<{files:string[]}>('GET','/runs/'+encodeURIComponent(id)+'/files'),
    file: (id: string, path: string) => this.request<{path:string;content:string}>('GET','/runs/'+encodeURIComponent(id)+'/files/'+path.split('/').map(encodeURIComponent).join('/')),
  };

  /** Persistent, project-scoped non-weight notes. */
  readonly memory = {
    search: (scope: string, query = '') =>
      this.request<MemoryEntry[]>('GET','/memory?scope='+encodeURIComponent(scope)+'&query='+encodeURIComponent(query)),
    store: (input: {scope:string;title:string;content:string}) =>
      this.request<MemoryEntry>('POST','/memory',input),
    list: (scope='global') =>
      this.request<MemoryEntry[]>('GET','/memory/all?scope='+encodeURIComponent(scope)),
    scopes: () => this.request<string[]>('GET','/memory/scopes'),
    revise: (id:string,input:{scope:string;title:string;content:string}) =>
      this.request<MemoryEntry>('POST','/memory/'+encodeURIComponent(id),input),
    forget: (id:string,scope:string) =>
      this.request<{deleted:boolean;id:string}>('DELETE','/memory/'+encodeURIComponent(id)+'?scope='+encodeURIComponent(scope)),
  };

  readonly groups = {
    create: (input: CreateGroup) => this.request<GroupRoom>('POST','/groups',input),
    list: () => this.request<GroupRoom[]>('GET','/groups'),
    get: (id: string) => this.request<GroupRoom>('GET','/groups/'+encodeURIComponent(id)),
    say: (id: string, content: string) => this.request<GroupRoom>('POST','/groups/'+encodeURIComponent(id)+'/messages',{content}),
    next: (id: string) => this.request<GroupRoom>('POST','/groups/'+encodeURIComponent(id)+'/next',{}),
  };
  /** Durable, evidence-ranked cross-task strategy retrieval owned by Rust. */
  readonly strategies = {
    search: (repository:string, query:string, limit=6) => this.request<StrategyEvidence[]>(
      'GET','/strategies?repository='+encodeURIComponent(repository)+
      '&query='+encodeURIComponent(query)+'&limit='+Math.min(6,Math.max(1,limit))
    ),
  };
  /** HTTP SSE streaming: yields replayable sequence-numbered events until the run is terminal. */
  async *events(id: string, options: {since?: number; signal?: AbortSignal} = {}): AsyncGenerator<TimelineEvent, void, unknown> {
    const headers: Record<string, string> = {'Accept':'text/event-stream'};
    if (this.token) headers.Authorization = 'Bearer ' + this.token;
    let cursor = options.since ?? 0;
    const url = this.baseUrl + '/api/v1/runs/' + encodeURIComponent(id) + '/events?since=' + cursor;
    const response = await this.fetcher(url, { headers, signal:options.signal });
    if (!response.ok) throw new DominionError(response.status, await response.text());
    if (!response.body) throw new DominionError(500,'SSE stream has no body');
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    try {
      while (true) {
        const {done, value} = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, {stream:true}).replace(/\r/g,'');
        let boundary: number;
        while ((boundary = buffer.indexOf('\n\n')) >= 0) {
          const frame = buffer.slice(0,boundary);
          buffer = buffer.slice(boundary+2);
          const lines = frame.split('\n');
          const data = lines.filter(line=>line.startsWith('data:')).map(line=>line.slice(5).trimStart()).join('\n');
          if (!data) continue;
          const event = JSON.parse(data) as TimelineEvent;
          if (event.seq > cursor) { cursor=event.seq; yield event; }
        }
      }
    } finally { await reader.cancel().catch(()=>undefined); reader.releaseLock(); }
  }
  /** Polls durable server state, not browser-local state; safe after a tab or process restarts. */
  async wait(id: string, options: {intervalMs?: number; timeoutMs?: number; signal?: AbortSignal} = {}): Promise<Run> {
    const interval = Math.max(100,options.intervalMs ?? 1000);
    const timeout = options.timeoutMs ?? 0;
    const deadline = timeout > 0 ? Date.now()+timeout : Infinity;
    while (true) {
      options.signal?.throwIfAborted();
      const run=await this.runs.get(id);
      if (['completed','failed','cancelled'].includes(run.status)) return run;
      if (Date.now() >= deadline) throw new DominionError(408,'execution wait timed out');
      await new Promise<void>((resolve,reject)=>{
        const timer = setTimeout(()=>{options.signal?.removeEventListener('abort',abort);resolve();},Math.min(interval,deadline-Date.now()));
        const abort=()=>{clearTimeout(timer);reject(options.signal?.reason ?? new Error('aborted'));};
        options.signal?.addEventListener('abort',abort,{once:true});
      });
    }
  }
}
export default Dominion;
