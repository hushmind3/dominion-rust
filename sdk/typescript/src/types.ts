/** Dominion engine-owned records. Extra fields remain visible as the API evolves. */
export interface DominionRecord {
  id: string;
  [key: string]: unknown;
}
export interface ModelSelection {
  agentId?: string;
  providerId?: string;
  model?: string;
}
export type GoalStatus = "queued" | "running" | "completed" | "needs_review" | "failed" | "cancelled";
export interface Goal extends DominionRecord {
  goal: string;
  status: GoalStatus;
  jobId?: string;
  taskId?: string;
  sessionId?: string;
  phase?: string;
  iterations?: number;
  gaps?: string[];
  evidence?: unknown[];
}
export interface OwnerDirective {
  text:string;
  mode:"append"|"replace";
  requestId:string;
}
export interface GoalCreate {
  goal: string;
  successCriteria?: Record<string, unknown>[];
  executor?: ModelSelection;
  reviewer?: ModelSelection;
  planner?: ModelSelection;
  isolateWorktree?: boolean;
  tokenBudget?: number;
  maxIterations?: number;
  maxReplans?: number;
  replanAfter?: number;
}
export interface Job extends DominionRecord {
  kind: string;
  status: string;
  sessionId?: string;
  input?: Record<string, unknown>;
}
export interface Session extends DominionRecord {
  title: string;
  agentId?: string;
  providerId?: string;
  model?: string;
}
export interface SessionCreate extends ModelSelection {
  title?: string;
}
export interface Message extends DominionRecord {
  role: string;
  content: string;
  session_id: string;
  metadata?: Record<string, unknown>;
}
export interface Approval extends DominionRecord {
  status: string;
  tool?: string;
  sessionId?: string;
  args?: unknown;
}
export interface ServerEvent {
  type: string;
  seq?: number;
  [key: string]: unknown;
}
export interface ChatEvent {
  type: string;
  [key: string]: unknown;
}
export interface ChatRequest extends ModelSelection {
  sessionId: string;
  input: string;
  toolsEnabled?: boolean;
  toolsExecutionEnabled?: boolean;
  autoLearning?: boolean;
  thinking?: boolean;
  reasoningEffort?: string;
  reasoningLanguage?: string;
}
export interface RequestOptions {
  signal?: AbortSignal;
  timeoutMs?: number;
}
export interface WaitOptions<T> extends RequestOptions {
  /** Polling, not another model invocation. Defaults to 750 ms. */
  pollIntervalMs?: number;
  onUpdate?: (record: T) => void;
}
export interface EventWatchOptions {
  /** Last durable event sequence already handled. Defaults to 0. */
  after?: number;
  signal?: AbortSignal;
  reconnect?: boolean;
  /** Defaults to 750 ms. */
  retryDelayMs?: number;
  /** Defaults to unlimited reconnections; abort using signal to stop. */
  maxReconnects?: number;
}
export interface ChatResult {
  finalResponse: string;
  message: Message | null;
  events: ChatEvent[];
}


/** An explicit potentially paid source evolution using Dominion's own engine. */
export type RsiSelectionPolicy = "manual" | "automatic";
export interface RsiEvolutionRequest {
  request: string;
  providerId: string;
  model: string;
  selectionPolicy?: RsiSelectionPolicy;
  limits?: Record<string, unknown>;
  focusFiles?: string[];
  continueFrom?: string;
  methodAblation?: boolean;
}
export interface RsiCampaign extends DominionRecord {
  status: string;
  phase?: string;
  baseCommit?: string;
  headCommit?: string;
  recursiveProof?: Record<string, unknown>;
  capabilityDemonstrated?: boolean;
}
