export type RunStatus = 'queued' | 'running' | 'awaiting_approval' | 'paused' | 'completed' | 'failed' | 'cancelled';
export interface TimelineEvent {
  seq: number; kind: string; message: string; detail: unknown | null; at_ms: number;
}
export interface ToolCall { id: string; name: string; arguments: Record<string, unknown> }
export interface Run {
  id: string; instruction: string; status: RunStatus; plan: string | null; answer: string | null;
  messages: unknown[]; events: TimelineEvent[]; queued_calls: ToolCall[];
  pending_approval: ToolCall | null; approval_decision: boolean | null;
  steps: number; input_tokens: number; output_tokens: number; cost_krw: number;
  budget_krw: number; max_steps: number; created_at_ms: number; updated_at_ms: number;
}
export interface ModelConfig {
  model: string; economy_model: string; configured: boolean; shell_enabled: boolean; max_steps: number;
}
export interface CreateRun {
  instruction: string; budget_krw?: number; max_steps?: number;
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
    this.baseUrl = (options.baseUrl ?? 'http://127.0.0.1:8787').replace(/\/+$/, '');
    this.token = options.token;
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
  }
  async request<T>(method: 'GET'|'POST', path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
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
  get health(): Promise<{ok: boolean; name: string; api: string}> {return this.request('GET','/health');}
  get config(): Promise<ModelConfig> {return this.request('GET','/config');}
  readonly runs = {
    create: (input: CreateRun) => this.request<Run>('POST','/runs',input),
    list: () => this.request<Run[]>('GET','/runs'),
    get: (id: string) => this.request<Run>('GET','/runs/'+encodeURIComponent(id)),
    resume: (id: string) => this.request<{ok:boolean}>('POST','/runs/'+encodeURIComponent(id)+'/resume',{}),
    cancel: (id: string) => this.request<Run>('POST','/runs/'+encodeURIComponent(id)+'/cancel',{}),
    approve: (id: string, allow: boolean) => this.request<{ok:boolean}>('POST','/runs/'+encodeURIComponent(id)+'/approval',{allow}),
    instruct: (id: string, instruction: string) => this.request<Run>('POST','/runs/'+encodeURIComponent(id)+'/directive',{instruction}),
    files: (id: string) => this.request<{files:string[]}>('GET','/runs/'+encodeURIComponent(id)+'/files'),
    file: (id: string, path: string) => this.request<{path:string;content:string}>('GET','/runs/'+encodeURIComponent(id)+'/files/'+path.split('/').map(encodeURIComponent).join('/')),
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
