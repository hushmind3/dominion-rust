import {
  decodeSse, delay, DominionHttpError, DominionProtocolError, DominionStreamInterrupted,
  DominionTransport, parseSseJson,
} from "./transport.js";
import type {TransportOptions} from "./transport.js";
import type {
  Approval, ChatEvent, ChatRequest, ChatResult, DominionRecord, EventWatchOptions,
  Goal, GoalCreate, Job, Message, ModelSelection, OwnerDirective, RequestOptions, ServerEvent,
  Session, SessionCreate, WaitOptions,
} from "./types.js";

const idPart = (id: string) => {
  if (!id || typeof id !== "string") throw new TypeError("A non-empty record ID is required");
  return encodeURIComponent(id);
};
const terminalGoal = (status: string) =>
  ["completed", "failed", "cancelled", "needs_review"].includes(status);
const terminalJob = (status: string) =>
  ["completed", "failed", "cancelled", "needs_review"].includes(status);

/**
 * First-party client for the Dominion Rust engine. Does not embed a vendor
 * agent runtime. No POST request or chat stream is silently retried.
 */
export class DominionClient {
  readonly transport: DominionTransport;

  constructor(options: TransportOptions = {}) {
    this.transport = new DominionTransport(options);
  }

  readonly goals = {
    create: (input: GoalCreate, options?: RequestOptions) =>
      this.transport.json<Goal>("/api/goals", "POST", input, options),
    list: (options?: RequestOptions) =>
      this.transport.json<Goal[]>("/api/goals", "GET", undefined, options),
    get: (id: string, options?: RequestOptions) =>
      this.transport.json<Goal>(`/api/goals/${idPart(id)}`, "GET", undefined, options),
    cancel: (id: string, options?: RequestOptions) =>
      this.transport.json<Goal>(`/api/goals/${idPart(id)}/cancel`, "POST", {}, options),
    /** Changes the owner's plan at the next safe execution boundary. */
    steer: (id: string, plan: string, options?: RequestOptions) =>
      this.transport.json<Goal>(`/api/goals/${idPart(id)}/plan`, "POST", {plan}, options),
    direct: (id: string, input: OwnerDirective, options?: RequestOptions) => {
      if (!input.requestId || !/^[A-Za-z0-9_-]{1,100}$/.test(input.requestId))
        throw new TypeError("Invalid stable owner requestId");
      if (!input.text?.trim() || !["append","replace"].includes(input.mode))
        throw new TypeError("Owner directive requires text and append/replace mode");
      return this.transport.json<Goal>(`/api/goals/${idPart(id)}/steer`,
        "POST", input, options);
    },
    appendInstruction: (id: string, text: string, requestId: string, options?: RequestOptions) =>
      this.goals.direct(id, {text,mode:"append",requestId}, options),
    replacePlan: (id: string, text: string, requestId: string, options?: RequestOptions) =>
      this.goals.direct(id, {text,mode:"replace",requestId}, options),
    /** Does not conceal uncertain external effects: caller must explicitly acknowledge. */
    resume: (id: string, acknowledgeUnknownEffects: true, options?: RequestOptions) =>
      this.transport.json<Goal>(`/api/goals/${idPart(id)}/resume`, "POST", {acknowledgeUnknownEffects}, options),
    changes: (id: string, options?: RequestOptions) =>
      this.transport.json<unknown>(`/api/goals/${idPart(id)}/changes`, "GET", undefined, options),
    wait: (id: string, options?: WaitOptions<Goal>) =>
      this.waitFor(() => this.goals.get(id, {signal: options?.signal}), terminalGoal, options),
  };

  readonly jobs = {
    list: (options?: RequestOptions) =>
      this.transport.json<Job[]>("/api/jobs", "GET", undefined, options),
    get: (id: string, options?: RequestOptions) =>
      this.transport.json<Job>(`/api/jobs/${idPart(id)}`, "GET", undefined, options),
    /** Generic prompt worker; durable across browser closure and engine restart. */
    prompt: (prompt: string, model: ModelSelection = {}, options?: RequestOptions) =>
      this.transport.json<Job>("/api/jobs", "POST", {
        kind: "prompt", input: {prompt, ...model, toolsEnabled: true},
      }, options),
    cancel: (id: string, options?: RequestOptions) =>
      this.transport.json<Job>(`/api/jobs/${idPart(id)}/cancel`, "POST", {}, options),
    wait: (id: string, options?: WaitOptions<Job>) =>
      this.waitFor(() => this.jobs.get(id, {signal: options?.signal}), terminalJob, options),
  };

  readonly sessions = {
    create: (input: SessionCreate = {}, options?: RequestOptions) =>
      this.transport.json<Session>("/api/sessions", "POST", input, options),
    list: (options?: RequestOptions) =>
      this.transport.json<Session[]>("/api/sessions", "GET", undefined, options),
    get: (id: string, options?: RequestOptions) =>
      this.transport.json<Session>(`/api/sessions/${idPart(id)}`, "GET", undefined, options),
    messages: (id: string, options?: RequestOptions) =>
      this.transport.json<Message[]>(`/api/sessions/${idPart(id)}/messages`, "GET", undefined, options),
    fork: (id: string, options?: RequestOptions) =>
      this.transport.json<Session>(`/api/sessions/${idPart(id)}/fork`, "POST", {}, options),
    cancel: (id: string, options?: RequestOptions) =>
      this.transport.json<unknown>(`/api/sessions/${idPart(id)}/cancel`, "POST", {}, options),
  };

  readonly executions = {
    list: (limit = 200, offset = 0, options?: RequestOptions) =>
      this.transport.json<DominionRecord[]>(`/api/executions?limit=${limit}&offset=${offset}`, "GET", undefined, options),
    get: (id: string, options?: RequestOptions) =>
      this.transport.json<DominionRecord>(`/api/executions/${idPart(id)}`, "GET", undefined, options),
    operations: (id: string, options?: RequestOptions) =>
      this.transport.json<DominionRecord[]>(`/api/executions/${idPart(id)}/operations`, "GET", undefined, options),
    cancel: (id: string, options?: RequestOptions) =>
      this.transport.json<DominionRecord>(`/api/executions/${idPart(id)}/cancel`, "POST", {}, options),
  };

  readonly approvals = {
    pending: (options?: RequestOptions) =>
      this.transport.json<Approval[]>("/api/approvals", "GET", undefined, options),
    resolve: (id: string, allow: boolean, options?: RequestOptions) =>
      this.transport.json<{ok: boolean; resolved: boolean; status: string}>(
        "/api/approvals", "POST", {id, allow}, options),
  };

  readonly catalog = {
    agents: (options?: RequestOptions) =>
      this.transport.json<DominionRecord[]>("/api/agents", "GET", undefined, options),
    providers: (options?: RequestOptions) =>
      this.transport.json<DominionRecord[]>("/api/providers", "GET", undefined, options),
    tools: (options?: RequestOptions) =>
      this.transport.json<unknown>("/api/tools", "GET", undefined, options),
    capabilities: (options?: RequestOptions) =>
      this.transport.json<unknown>("/api/capabilities", "GET", undefined, options),
  };

  /** A completed SSE chat stream is one turn, not an automatically resumable POST. */
  async *streamChat(input: ChatRequest, options: RequestOptions = {}): AsyncGenerator<ChatEvent> {
    const body = {
      agentId: input.agentId ?? "",
      ...input,
      toolsEnabled: input.toolsEnabled ?? true,
      toolsExecutionEnabled: input.toolsExecutionEnabled ?? true,
      autoLearning: input.autoLearning ?? false,
    };
    const source = await this.transport.sse("/api/chat", "POST", body, options);
    for await (const frame of decodeSse(source)) {
      const event = parseSseJson<ChatEvent>(frame);
      if (event.type === "error") {
        throw new DominionProtocolError(typeof event.error === "string" ? event.error : "Chat execution failed");
      }
      yield event;
      if (event.type === "done") return;
    }
    throw new DominionStreamInterrupted();
  }

  /**
   * Reads the durable server event log and reconnects with ?after=<last seq>.
   * Unlike chat, this GET may be reissued without repeating an agent action.
   */
  async *watchEvents(options: EventWatchOptions = {}): AsyncGenerator<ServerEvent> {
    let cursor = options.after ?? 0;
    if (!Number.isSafeInteger(cursor) || cursor < 0) throw new TypeError("after must be a non-negative event sequence");
    const reconnect = options.reconnect ?? true;
    const maxReconnects = options.maxReconnects ?? Number.POSITIVE_INFINITY;
    let reconnects = 0;
    while (!options.signal?.aborted) {
      try {
        const stream = await this.transport.sse(
          `/api/events?after=${cursor}`, "GET", undefined, {signal: options.signal},
        );
        for await (const frame of decodeSse(stream)) {
          const event = parseSseJson<ServerEvent>(frame);
          const seq = event.seq ?? (frame.id ? Number(frame.id) : NaN);
          if (!Number.isSafeInteger(seq) || seq < 1)
            throw new DominionProtocolError("Durable SSE event has an invalid sequence");
          if (seq <= cursor) continue; // duplicated frame after replay
          cursor = seq;
          reconnects = 0;
          yield event;
        }
      } catch (error) {
        if (options.signal?.aborted) return;
        if (error instanceof DominionHttpError && error.status >= 400 && error.status < 500) throw error;
        if (error instanceof DominionProtocolError) throw error;
        if (!reconnect || reconnects >= maxReconnects) throw error;
      }
      if (!reconnect || options.signal?.aborted) return;
      if (reconnects >= maxReconnects) throw new DominionStreamInterrupted("Durable event stream reconnect limit reached");
      reconnects++;
      try { await delay(Math.min(30_000, (options.retryDelayMs ?? 750) * 2 ** Math.min(6, reconnects - 1)), options.signal); }
      catch { if (options.signal?.aborted) return; throw new DominionStreamInterrupted("Durable event reconnect interrupted"); }
    }
  }

  async startThread(input: SessionCreate = {}, options?: RequestOptions): Promise<DominionThread> {
    const session = await this.sessions.create(input, options);
    return new DominionThread(this, session.id, input);
  }

  resumeThread(sessionId: string, model: ModelSelection = {}): DominionThread {
    return new DominionThread(this, sessionId, model);
  }

  private async waitFor<T extends {status: string}>(
    get: () => Promise<T>, complete: (status: string) => boolean,
    options: WaitOptions<T> = {},
  ): Promise<T> {
    const started = Date.now();
    const maxMs = options.timeoutMs ?? 600_000;
    const interval = Math.max(100, options.pollIntervalMs ?? 750);
    let last: string | undefined;
    while (true) {
      if (options.signal?.aborted) throw options.signal.reason ?? new Error("Aborted");
      const current = await get();
      const signature = JSON.stringify(current);
      if (signature !== last) { options.onUpdate?.(current); last = signature; }
      if (complete(current.status)) return current;
      if (Date.now() - started >= maxMs) {
        throw new DominionStreamInterrupted(`Polling timed out after ${maxMs}ms; remote task was NOT cancelled`);
      }
      await delay(Math.min(interval, Math.max(1, maxMs - (Date.now() - started))), options.signal);
    }
  }
}

/** A durable Dominion conversation handle, backed by Rust/SQLite session IDs. */
export class DominionThread {
  constructor(
    private readonly client: DominionClient,
    readonly id: string,
    private readonly defaults: ModelSelection = {},
  ) { idPart(id); }

  messages(options?: RequestOptions) { return this.client.sessions.messages(this.id, options); }
  cancel(options?: RequestOptions) { return this.client.sessions.cancel(this.id, options); }

  async fork(options?: RequestOptions): Promise<DominionThread> {
    const copied = await this.client.sessions.fork(this.id, options);
    return new DominionThread(this.client, copied.id, this.defaults);
  }

  runStreamed(input: string, options: Omit<ChatRequest, "sessionId" | "input"> & RequestOptions = {}) {
    const {signal, timeoutMs, ...model} = options;
    return this.client.streamChat({
      sessionId: this.id, input, ...this.defaults, ...model,
    }, {signal, timeoutMs});
  }

  /** Returns only newly persisted assistant content, not the previous turn's answer. */
  async run(input: string, options: Omit<ChatRequest, "sessionId" | "input"> & RequestOptions = {}): Promise<ChatResult> {
    const previous = await this.messages({signal: options.signal});
    const ids = new Set(previous.map(message => message.id));
    const events: ChatEvent[] = [];
    for await (const event of this.runStreamed(input, options)) events.push(event);
    const latest = await this.messages({signal: options.signal});
    const message = latest.filter(m => m.role === "assistant" && !ids.has(m.id)).at(-1) ?? null;
    return {finalResponse: message?.content ?? "", message, events};
  }
}
