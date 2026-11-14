export type JsonObject = Record<string, unknown>;
export type RequestOptions = { signal?: AbortSignal; headers?: Record<string, string> };
export type SdkOptions = { baseUrl?: string; token?: string; headers?: Record<string, string>; fetch?: typeof fetch };
export type StreamEvent = { type: string; text?: string; error?: string; message?: string; [key: string]: unknown };
export type Session = JsonObject & { id: string; agentId?: string; providerId?: string; model?: string };
export type Job = JsonObject & { id: string; kind?: string; status: string; result?: unknown; error?: unknown };
export type Goal = JsonObject & { id: string; jobId?: string; status: string; goal: string };
export type ChatRequest = { sessionId: string; agentId: string; input: string; providerId?: string; model?: string; toolsEnabled?: boolean; toolsExecutionEnabled?: boolean; autoLearning?: boolean; reasoningLanguage?: string; reasoningEffort?: string; thinking?: boolean; [key: string]: unknown };
export class DominionError extends Error {
  readonly status: number;
  readonly method: string;
  readonly path: string;
  readonly details: unknown;
  readonly code: string;
  constructor(message: string, options?: { status?: number; method?: string; path?: string; details?: unknown; code?: string });
}
export class DominionClient {
  baseUrl: string;
  token: string;
  headers: Record<string, string>;
  fetch: typeof fetch;
  constructor(options?: SdkOptions);
  url(path: string): string;
  request<T = unknown>(method: string, path: string, body?: unknown, options?: RequestOptions): Promise<T>;
  stream(path: string, body: unknown, options?: RequestOptions): AsyncGenerator<StreamEvent>;
  runChat(body: ChatRequest, options?: RequestOptions): Promise<{ text: string; reasoning: string; events: StreamEvent[]; done: StreamEvent | null }>;
  waitJob(jobId: string, options?: { intervalMs?: number; signal?: AbortSignal; maxPolls?: number }): Promise<Job>;
  session(sessionId: string): {
    id: string;
    messages(options?: RequestOptions): Promise<JsonObject[]>;
    usage(options?: RequestOptions): Promise<JsonObject>;
    stream(input: string, config?: Omit<ChatRequest, 'sessionId' | 'input'>, options?: RequestOptions): AsyncGenerator<StreamEvent>;
    run(input: string, config?: Omit<ChatRequest, 'sessionId' | 'input'>, options?: RequestOptions): Promise<{ text: string; reasoning: string; events: StreamEvent[]; done: StreamEvent | null }>;
    cancel(options?: RequestOptions): Promise<JsonObject>;
    steer(instruction: string, options?: RequestOptions): Promise<JsonObject>;
  };
  sessions: {
    list(options?: RequestOptions): Promise<Session[]>;
    create(config?: JsonObject, options?: RequestOptions): Promise<Session>;
    get(id: string, options?: RequestOptions): Promise<Session>;
    messages(id: string, options?: RequestOptions): Promise<JsonObject[]>;
    usage(id: string, options?: RequestOptions): Promise<JsonObject>;
    cancel(id: string, options?: RequestOptions): Promise<JsonObject>;
    steer(id: string, instruction: string, options?: RequestOptions): Promise<JsonObject>;
    fork(id: string, options?: RequestOptions): Promise<Session>;
    compact(id: string, config: JsonObject, options?: RequestOptions): Promise<JsonObject>;
  };
  chat: {
    stream(request: ChatRequest, options?: RequestOptions): AsyncGenerator<StreamEvent>;
    run(request: ChatRequest, options?: RequestOptions): Promise<{ text: string; reasoning: string; events: StreamEvent[]; done: StreamEvent | null }>;
  };
  goals: {
    list(options?: RequestOptions): Promise<Goal[]>;
    create(config: JsonObject & { goal: string }, options?: RequestOptions): Promise<Goal>;
    get(id: string, options?: RequestOptions): Promise<Goal>;
    receipt(id: string, options?: RequestOptions): Promise<JsonObject>;
    resume(id: string, config?: JsonObject, options?: RequestOptions): Promise<JsonObject>;
    steer(id: string, config: { expectedRevision: number; plan: string }, options?: RequestOptions): Promise<JsonObject>;
    editPlan(id: string, config: { plan: string }, options?: RequestOptions): Promise<JsonObject>;
    diff(id: string, options?: RequestOptions): Promise<JsonObject>;
    apply(id: string, sha256: string, options?: RequestOptions): Promise<JsonObject>;
    approvals(id: string, options?: RequestOptions): Promise<JsonObject[]>;
    subtasks(id: string, options?: RequestOptions): Promise<JsonObject[]>;
  };
  jobs: {
    list(options?: RequestOptions): Promise<Job[]>;
    get(id: string, options?: RequestOptions): Promise<Job>;
    create(kind: 'prompt' | 'workflow', input: JsonObject, options?: RequestOptions): Promise<Job>;
    prompt(prompt: string, config?: JsonObject, options?: RequestOptions): Promise<Job>;
    cancel(id: string, options?: RequestOptions): Promise<Job>;
    wait(id: string, options?: { intervalMs?: number; signal?: AbortSignal; maxPolls?: number }): Promise<Job>;
  };
  approvals: { list(options?: RequestOptions): Promise<JsonObject[]>; resolve(id: string, allow: boolean, options?: RequestOptions): Promise<JsonObject> };
  agents: { list(options?: RequestOptions): Promise<JsonObject[]> };
  providers: { list(options?: RequestOptions): Promise<JsonObject[]>; models(id: string, options?: RequestOptions): Promise<JsonObject> };
  tools: { list(options?: RequestOptions): Promise<JsonObject[]> };
  mcp: { list(options?: RequestOptions): Promise<JsonObject[]>; register(config: JsonObject & { name: string; transport: 'http' | 'stdio' }, options?: RequestOptions): Promise<JsonObject>; remove(id: string, options?: RequestOptions): Promise<JsonObject>; discover(options?: RequestOptions): Promise<JsonObject[]> };
  rooms: { list(options?: RequestOptions): Promise<JsonObject[]>; run(id: string, input: JsonObject, options?: RequestOptions): Promise<JsonObject>; stop(id: string, options?: RequestOptions): Promise<JsonObject>; messages(id: string, options?: RequestOptions): Promise<JsonObject[]> };
  rsi: { list(options?: RequestOptions): Promise<JsonObject>; start(input: JsonObject, options?: RequestOptions): Promise<JsonObject>; campaigns(options?: RequestOptions): Promise<JsonObject>; campaign(id: string, options?: RequestOptions): Promise<JsonObject> };
}
