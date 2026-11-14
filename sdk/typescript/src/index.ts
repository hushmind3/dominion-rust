/**
 * DOMINION Agent SDK v0.1: a typed client for DOMINION's own Rust agent runtime.
 * No Copilot/Claude/Codex CLI, SDK, hidden prompt policy, or model runtime dependency.
 * The server owns model routing, tool execution, approvals, persistence and evidence.
 */

export type JsonObject = Record<string, unknown>
export type AgentEvent = {type: string; [field: string]: unknown}
export type Session = JsonObject & {id: string; title?: string; agentId?: string}
export type Message = JsonObject & {id: string; role: string; content: string; session_id: string}
export type Approval = JsonObject & {id: string; tool: string; status: string; sessionId: string}
export type Goal = JsonObject & {id: string; jobId: string; status: string; goal: string}
export type Job = JsonObject & {id: string; status: string; kind: string}
export type Tool = JsonObject & {name: string}
export type MCPServer = JsonObject & {
  id: string
  name: string
  transport: 'http' | 'stdio'
  enabled?: boolean
}
export type MCPServerConfig = {
  id?: string
  name: string
  transport: 'http' | 'stdio'
  url?: string
  command?: string
  args?: string[]
  enabled?: boolean
  /** Saved by the Rust vault; never returned in MCP records. */
  apiKey?: string
}

export type TurnInput = {
  input: string
  agentId?: string
  providerId?: string
  model?: string
  toolsEnabled?: boolean
  toolsExecutionEnabled?: boolean
  autoLearning?: boolean
  reasoningLanguage?: string
  reasoningEffort?: string
  thinking?: boolean
  attachmentIds?: string[]
}
export type SessionOptions = {
  title?: string
  agentId?: string
  providerId?: string
  model?: string
}
export type GoalOptions = {
  goal: string
  successCriteria?: unknown[]
  executor?: JsonObject
  reviewer?: JsonObject
  planner?: JsonObject
  tokenBudget?: number
  maxIterations?: number
  maxReplans?: number
  replanAfter?: number
  isolateWorktree?: boolean
}
export type RunOptions = {
  signal?: AbortSignal
  onEvent?: (event: AgentEvent) => void
}
export type RunResult = {
  sessionId: string
  finalResponse: string
  messageId?: string
  events: AgentEvent[]
}

export type SDKOptions = {
  /** Default: same origin as the browser / local Rust gateway. */
  baseUrl?: string
  /** Server token, sent only through the DOMINION X-Dominion-Token header. */
  token?: string
  /** Inject transport for tests, server-side usage or custom networking. */
  fetch?: typeof fetch
}

export class DominionSDKError extends Error {
  readonly status: number
  constructor(status: number, message: string) {
    super(message)
    this.name = 'DominionSDKError'
    this.status = status
  }
}

function endpoint(path: string): string {
  if (!path.startsWith('/api/')) {
    throw new Error('DOMINION SDK accepts only /api/ endpoints')
  }
  return path
}

function normalizedError(body: string, status: number): string {
  try {
    const parsed: unknown = JSON.parse(body)
    if (parsed && typeof parsed === 'object' && 'error' in parsed) {
      const error = (parsed as JsonObject).error
      if (typeof error === 'string') return error
    }
  } catch { /* Non-JSON error from proxy or gateway. */ }
  return body && !body.trimStart().startsWith('<') ? body.slice(0, 512) : 'HTTP ' + status
}

/** Each frame has any number of SSE fields and one or more data: lines. */
export function parseSSEFrame(frame: string): string | undefined {
  const data: string[] = []
  for (const line of frame.split(/\r?\n/)) {
    if (line === 'data') data.push('')
    else if (line.startsWith('data:')) {
      const value = line.slice(5)
      data.push(value.startsWith(' ') ? value.slice(1) : value)
    }
  }
  return data.length ? data.join('\n') : undefined
}

/**
 * Decoder shared by transient turn events and the durable replayable event bus.
 * For a turn, the Rust done event is mandatory; for the bus, EOF is allowed.
 */
async function* decodeEventStream(response: Response, requireDone: boolean): AsyncGenerator<AgentEvent> {
  if (!response.ok) {
    throw new DominionSDKError(response.status, normalizedError(await response.text(), response.status))
  }
  if (!response.body || !response.headers.get('Content-Type')?.startsWith('text/event-stream')) {
    throw new DominionSDKError(response.status, 'Expected a DOMINION event stream')
  }
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  try {
    while (true) {
      const {done, value} = await reader.read()
      buffer += decoder.decode(value || new Uint8Array(), {stream: !done})
      let match: RegExpExecArray | null
      while ((match = /\r?\n\r?\n/.exec(buffer)) !== null) {
        const frame = buffer.slice(0, match.index)
        buffer = buffer.slice(match.index + match[0].length)
        if (frame.length > 4_194_304) {
          throw new DominionSDKError(0, 'DOMINION event frame exceeds 4 MiB')
        }
        const data = parseSSEFrame(frame)
        if (data === undefined || data.trim() === '') continue
        let event: AgentEvent
        try {
          event = JSON.parse(data) as AgentEvent
        } catch {
          throw new DominionSDKError(0, 'Malformed DOMINION event JSON')
        }
        if (!event || typeof event !== 'object' || typeof event.type !== 'string') {
          throw new DominionSDKError(0, 'Malformed DOMINION event envelope')
        }
        yield event
        if (requireDone && event.type === 'done') return
      }
      if (buffer.length > 4_194_304) {
        throw new DominionSDKError(0, 'DOMINION event frame exceeds 4 MiB')
      }
      if (done) {
        if (requireDone) throw new DominionSDKError(0, 'DOMINION stream interrupted before done')
        if (buffer.trim() && !buffer.trimStart().startsWith(':')) {
          throw new DominionSDKError(0, 'DOMINION event stream ended during a frame')
        }
        return
      }
    }
  } finally {
    await reader.cancel().catch(() => {})
    reader.releaseLock()
  }
}

/** HTTP is transport only; the autonomous agent loop lives in DOMINION Rust. */
export class DominionSDK {
  private readonly baseUrl: string
  private readonly transport: typeof fetch
  private token: string

  constructor(options: SDKOptions = {}) {
    this.baseUrl = (options.baseUrl || '').replace(/\/+$/, '')
    this.token = options.token || ''
    this.transport = options.fetch || globalThis.fetch.bind(globalThis)
  }

  setToken(value: string): void {
    this.token = value
  }

  private async send(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers)
    if (typeof init.body === 'string' && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json')
    }
    if (this.token) headers.set('X-Dominion-Token', this.token)
    return this.transport(this.baseUrl + endpoint(path), {
      ...init,
      headers,
      cache: 'no-store',
    })
  }

  async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const response = await this.send(path, init)
    const body = await response.text()
    if (!response.ok) throw new DominionSDKError(response.status, normalizedError(body, response.status))
    try {
      return JSON.parse(body) as T
    } catch {
      throw new DominionSDKError(response.status, 'DOMINION response was not JSON')
    }
  }

  get<T>(path: string, signal?: AbortSignal): Promise<T> {
    return this.request<T>(path, {signal})
  }

  post<T>(path: string, data: unknown, signal?: AbortSignal): Promise<T> {
    return this.request<T>(path, {method: 'POST', body: JSON.stringify(data), signal})
  }

  async startThread(options: SessionOptions = {}): Promise<DominionThread> {
    const session = await this.post<Session>('/api/sessions', options)
    if (!session?.id) throw new DominionSDKError(200, 'Session id missing from DOMINION response')
    return new DominionThread(this, session.id)
  }

  /** Resume an existing persisted session. No new agent runtime is created. */
  resumeThread(sessionId: string): DominionThread {
    if (!sessionId) throw new Error('sessionId is required')
    return new DominionThread(this, sessionId)
  }

  sessions(signal?: AbortSignal): Promise<Session[]> {
    return this.get('/api/sessions', signal)
  }

  session(sessionId: string, signal?: AbortSignal): Promise<Session> {
    return this.get('/api/sessions/' + encodeURIComponent(sessionId), signal)
  }

  messages(sessionId: string, signal?: AbortSignal): Promise<Message[]> {
    return this.get('/api/sessions/' + encodeURIComponent(sessionId) + '/messages', signal)
  }

  tools(signal?: AbortSignal): Promise<Tool[]> {
    return this.get('/api/tools', signal)
  }

  approvals(signal?: AbortSignal): Promise<Approval[]> {
    return this.get('/api/approvals', signal)
  }

  /** MCP tools remain executed by the native Rust engine, not this JS process. */
  mcpServers(signal?: AbortSignal): Promise<MCPServer[]> {
    return this.get('/api/mcp', signal)
  }

  discoverMcpTools(signal?: AbortSignal): Promise<Tool[]> {
    return this.get('/api/mcp/discover', signal)
  }

  registerMcpServer(config: MCPServerConfig): Promise<MCPServer> {
    if (!config.name.trim()) throw new Error('MCP server name is required')
    if (config.transport === 'http' && !config.url) throw new Error('HTTP MCP server URL is required')
    if (config.transport === 'stdio' && !config.command) throw new Error('Stdio MCP command is required')
    return this.post('/api/mcp', config)
  }

  removeMcpServer(id: string): Promise<JsonObject> {
    return this.request('/api/mcp/' + encodeURIComponent(id), {method: 'DELETE'})
  }

  async resolveApproval(id: string, allow: boolean): Promise<JsonObject> {
    const decision = await this.post<JsonObject>('/api/approvals', {id, allow})
    if (decision.resolved !== true) {
      throw new DominionSDKError(409, typeof decision.error === 'string' ? decision.error : 'Approval already resolved')
    }
    return decision
  }

  async startGoal(options: GoalOptions): Promise<DominionGoal> {
    const goal = await this.post<Goal>('/api/goals', options)
    if (!goal?.id || !goal?.jobId) {
      throw new DominionSDKError(200, 'Goal/job id missing from DOMINION response')
    }
    return new DominionGoal(this, goal.id)
  }

  resumeGoal(id: string): DominionGoal {
    if (!id) throw new Error('goalId is required')
    return new DominionGoal(this, id)
  }

  goal(id: string, signal?: AbortSignal): Promise<Goal> {
    return this.get('/api/goals/' + encodeURIComponent(id), signal)
  }

  job(id: string, signal?: AbortSignal): Promise<Job> {
    return this.get('/api/jobs/' + encodeURIComponent(id), signal)
  }

  /**
   * Parses DOMINION's existing data: JSON SSE frames, including UTF-8 and
   * boundary splits. Does not retry an agent turn after an interrupted stream:
   * replay could duplicate real file/network effects.
   */
  async *streamChat(input: TurnInput & {sessionId: string}, signal?: AbortSignal): AsyncGenerator<AgentEvent> {
    const response = await this.send('/api/chat', {
      method: 'POST',
      body: JSON.stringify(input),
      signal,
      headers: {Accept: 'text/event-stream'},
    })
    yield* decodeEventStream(response, true)
  }

  /**
   * Durable record/approval/job updates from Rust /api/events.
   * After disconnect, pass the greatest observed event.seq as 'after'.
   * No transparent retries or automatic repeated actions.
   */
  async *watchEvents(options: {after?: number; signal?: AbortSignal} = {}): AsyncGenerator<AgentEvent> {
    const after = options.after ?? 0
    if (!Number.isSafeInteger(after) || after < 0) throw new Error('after must be a nonnegative integer')
    const response = await this.send('/api/events?after=' + after, {
      signal: options.signal,
      headers: {Accept: 'text/event-stream'},
    })
    yield* decodeEventStream(response, false)
  }

  /**
   * Reconnects the read-only durable event feed, never a side-effecting chat
   * turn. Uses Rust's exclusive 'after' cursor, skips overlap duplicates and
   * surfaces 'replay_gap' when server retention has pruned older messages.
   * A caller can cancel an indefinitely running watch with AbortController.
   */
  async *watchEventsResilient(options: {
    after?: number
    signal?: AbortSignal
    reconnectDelayMs?: number
    maxReconnects?: number
  } = {}): AsyncGenerator<AgentEvent> {
    let cursor = options.after ?? 0
    if (!Number.isSafeInteger(cursor) || cursor < 0) {
      throw new Error('after must be a nonnegative integer')
    }
    const initialDelay = options.reconnectDelayMs ?? 400
    const limit = options.maxReconnects ?? Number.POSITIVE_INFINITY
    if (!Number.isFinite(initialDelay) || initialDelay < 0 || initialDelay > 30_000) {
      throw new Error('reconnectDelayMs must be within 0..30000')
    }
    if (!Number.isSafeInteger(limit) && limit !== Number.POSITIVE_INFINITY) {
      throw new Error('maxReconnects must be a nonnegative integer or Infinity')
    }
    if (limit < 0) throw new Error('maxReconnects must be nonnegative')
    let failures = 0
    while (!options.signal?.aborted) {
      let received = false
      try {
        for await (const event of this.watchEvents({after: cursor, signal: options.signal})) {
          if (options.signal?.aborted) return
          if (event.type === 'replay_gap') {
            yield event
            continue
          }
          const seq = event.seq
          if (typeof seq !== 'number' || !Number.isSafeInteger(seq) || seq < 1) {
            throw new DominionSDKError(0, 'Durable event lacks a valid seq')
          }
          if (seq <= cursor) continue
          cursor = seq
          received = true
          yield event
        }
      } catch (error) {
        if (options.signal?.aborted) return
        if (error instanceof DominionSDKError &&
          error.status >= 400 && error.status < 500) throw error
      }
      if (options.signal?.aborted) return
      failures = received ? 0 : failures + 1
      if (failures > limit) {
        throw new DominionSDKError(0, 'DOMINION durable event watch retry limit reached')
      }
      const delay = Math.min(5000, initialDelay * 2 ** Math.min(failures, 5))
      if (delay > 0) {
        await new Promise<void>(resolve => {
          let timer: ReturnType<typeof setTimeout>
          const done = () => {
            clearTimeout(timer)
            options.signal?.removeEventListener('abort', done)
            resolve()
          }
          timer = setTimeout(done, delay)
          options.signal?.addEventListener('abort', done, {once: true})
          if (options.signal?.aborted) done()
        })
      }
    }
  }
}

export class DominionThread {
  readonly sdk: DominionSDK
  readonly id: string
  constructor(sdk: DominionSDK, id: string) {
    this.sdk = sdk
    this.id = id
  }

  runStreamed(input: string | TurnInput, options: RunOptions = {}): AsyncGenerator<AgentEvent> {
    const turn = typeof input === 'string' ? {input} : input
    return this.sdk.streamChat({...turn, sessionId: this.id}, options.signal)
  }

  async run(input: string | TurnInput, options: RunOptions = {}): Promise<RunResult> {
    const events: AgentEvent[] = []
    let streamedText = ''
    let messageId: string | undefined
    for await (const event of this.runStreamed(input, options)) {
      events.push(event)
      options.onEvent?.(event)
      if (event.type === 'text') {
        const piece = event.delta ?? event.text ?? event.content
        if (typeof piece === 'string') streamedText += piece
      }
      if (event.type === 'assistant_saved' && typeof event.messageId === 'string') {
        messageId = event.messageId
      }
      if (event.type === 'error') {
        throw new DominionSDKError(0, typeof event.message === 'string' ? event.message : 'Agent turn failed')
      }
    }
    // Source of truth: saved final message, not arbitrary intermediate deltas.
    let finalResponse = streamedText
    if (messageId) {
      const messages = await this.sdk.messages(this.id, options.signal)
      const saved = messages.find(message => message.id === messageId)
      if (saved && typeof saved.content === 'string') finalResponse = saved.content
    }
    return {sessionId: this.id, finalResponse, messageId, events}
  }

  history(signal?: AbortSignal): Promise<Message[]> {
    return this.sdk.messages(this.id, signal)
  }

  cancel(): Promise<JsonObject> {
    return this.sdk.post('/api/sessions/' + encodeURIComponent(this.id) + '/cancel', {})
  }

  fork(): Promise<Session> {
    return this.sdk.post('/api/sessions/' + encodeURIComponent(this.id) + '/fork', {})
  }
}

export class DominionGoal {
  readonly sdk: DominionSDK
  readonly id: string
  constructor(sdk: DominionSDK, id: string) {
    this.sdk = sdk
    this.id = id
  }

  status(signal?: AbortSignal): Promise<Goal> {
    return this.sdk.goal(this.id, signal)
  }

  cancel(): Promise<Goal> {
    return this.sdk.post('/api/goals/' + encodeURIComponent(this.id) + '/cancel', {})
  }

  /** The server requires a deliberate owner acknowledgement of uncertain external effects. */
  resume(options: {acknowledgeUnknownEffects: true}): Promise<Goal> {
    if (options?.acknowledgeUnknownEffects !== true) {
      throw new Error('resume requires acknowledgeUnknownEffects: true')
    }
    return this.sdk.post('/api/goals/' + encodeURIComponent(this.id) + '/resume', options)
  }

  steer(plan: string, expectedRevision: number): Promise<Goal> {
    return this.sdk.post('/api/goals/' + encodeURIComponent(this.id) + '/steer', {plan, expectedRevision})
  }
  /** Direct owner instruction, distinct from a model-generated work plan. */
  steerInstruction(instruction: string, expectedRevision: number): Promise<Goal> {
    if (!instruction || !instruction.trim()) {
      throw new Error('owner instruction cannot be empty')
    }
    return this.sdk.post('/api/goals/' + encodeURIComponent(this.id) + '/steer', {
      instruction, expectedRevision,
    })
  }

  changes(): Promise<JsonObject> {
    return this.sdk.get('/api/goals/' + encodeURIComponent(this.id) + '/changes')
  }

  /** Apply exactly the diff the owner inspected via changes(), not an unreviewed checkout. */
  applyChanges(reviewDigest: string): Promise<JsonObject> {
    if (!reviewDigest || !reviewDigest.trim()) {
      throw new Error('applyChanges requires the reviewed change digest')
    }
    return this.sdk.post('/api/goals/' + encodeURIComponent(this.id) + '/apply', {
      reviewDigest,
      acknowledgeReview: true,
    })
  }

  discardChanges(): Promise<JsonObject> {
    return this.sdk.post('/api/goals/' + encodeURIComponent(this.id) + '/discard', {})
  }
}
