export type RecordData = Record<string, unknown>;
export interface Options {signal?: AbortSignal}
export interface WaitOptions extends Options {
  timeoutMs?: number;
  pollIntervalMs?: number;
  onUpdate?: (record: RecordData) => void;
}
export interface WatchOptions extends Options {after?: number; reconnect?: boolean; reconnectDelayMs?: number}
export interface ClientOptions {baseUrl?: string; token?: string; fetch?: typeof fetch; headers?: Record<string, string>}
export interface Agent extends RecordData {id: string; name: string; systemPrompt?: string; providerId?: string; model?: string}
export interface Session extends RecordData {id: string; title: string; agentId?: string}
export interface ChatMessage extends RecordData {id: string; content: string; role: string; session_id: string}
export interface Goal extends RecordData {id: string; jobId: string; goal?: string; status: string}
export interface Job extends RecordData {id: string; kind: string; status: string}
export interface ChatInput {
  input: string; sessionId?: string; agentId?: string; providerId?: string;
  model?: string; toolsEnabled?: boolean; toolsExecutionEnabled?: boolean;
  autoLearning?: boolean; thinking?: boolean; reasoningEffort?: string;
  reasoningLanguage?: string; attachmentIds?: string[];
}
export interface GoalInput {
  goal: string; successCriteria?: RecordData[]; isolateWorktree?: boolean;
  publishPr?: boolean; verifyCommands?: string[]; executor?: RecordData;
  planner?: RecordData; reviewer?: RecordData; maxIterations?: number;
  maxReplans?: number; replanAfter?: number; tokenBudget?: number;
}
export interface SDKEvent extends RecordData {type: string; seq?: number; sessionId?: string; messageId?: string; message?: string}
export class DominionError extends Error {
  readonly status: number; readonly details: unknown;
  constructor(message: string, status?: number, details?: unknown);
}
export function decodeSSE(body: ReadableStream<Uint8Array>): AsyncGenerator<{id?: string; data: string}>;
export class DominionClient {
  constructor(options?: ClientOptions);
  readonly baseUrl: URL;
  readonly token: string;
  agents: {
    list(options?: Options): Promise<Agent[]>;
    save(input: {name: string; [key: string]: unknown}, options?: Options): Promise<Agent>;
  };
  sessions: {
    list(options?: Options): Promise<Session[]>;
    create(input?: RecordData, options?: Options): Promise<Session>;
    get(id: string, options?: Options): Promise<Session>;
    messages(id: string, options?: Options): Promise<ChatMessage[]>;
    usage(id: string, options?: Options): Promise<RecordData>;
    cancel(id: string, options?: Options): Promise<RecordData>;
    /** Apply new owner instructions at the next running Rust agent decision. */
    steer(id: string, input: string, options?: Options): Promise<RecordData>;
  };
  approvals: {
    pending(options?: Options): Promise<RecordData[]>;
    resolve(id: string, allow: boolean, options?: Options): Promise<RecordData>;
  };
  jobs: {
    create(input: {kind: 'prompt' | 'workflow'; input: RecordData}, options?: Options): Promise<Job>;
    get(id: string, options?: Options): Promise<Job>;
    cancel(id: string, options?: Options): Promise<Job>;
    wait(id: string, options?: WaitOptions): Promise<Job>;
  };
  goals: {
    create(input: GoalInput, options?: Options): Promise<Goal>;
    get(id: string, options?: Options): Promise<Goal>;
    wait(id: string, options?: WaitOptions): Promise<Goal>;
    run(input: GoalInput, options?: WaitOptions): Promise<Goal>;
    resume(id: string, options?: Options): Promise<Goal>;
    changes(id: string, options?: Options): Promise<RecordData>;
    applyReviewed(id: string, patchSha256: string, options?: Options): Promise<RecordData>;
    rollback(id: string, options?: Options): Promise<RecordData>;
  };
  chat: {
    stream(input: ChatInput, options?: Options): AsyncGenerator<SDKEvent>;
    run(input: ChatInput, options?: Options & {onEvent?: (event: SDKEvent) => void}):
      Promise<{sessionId?: string; message: ChatMessage | null}>;
  };
  events: {watch(options?: WatchOptions): AsyncGenerator<SDKEvent>};
  request<T>(method: string, path: string, body?: unknown, options?: Options): Promise<T>;
  streamChat(input: ChatInput, options?: Options): AsyncGenerator<SDKEvent>;
  runChat(input: ChatInput, options?: Options & {onEvent?: (event: SDKEvent) => void}):
    Promise<{sessionId?: string; message: ChatMessage | null}>;
  watchEvents(options?: WatchOptions): AsyncGenerator<SDKEvent>;
}
