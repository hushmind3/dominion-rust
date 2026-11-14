/** DOMINION native SDK: Rust owns the agent loop, tools, memory and approvals. */
export class DominionError extends Error {
  constructor(message, { status = 0, method = '', path = '', details = null, code = 'DOMINION_ERROR' } = {}) {
    super(message);
    this.name = 'DominionError';
    this.status = status;
    this.method = method;
    this.path = path;
    this.details = details;
    this.code = code;
  }
}
const id = value => encodeURIComponent(String(value));
const terminal = new Set(['completed', 'failed', 'cancelled', 'needs_review']);
const reason = details => typeof details?.error === 'string' ? details.error :
  typeof details?.message === 'string' ? details.message : null;

function frames(buffer, final = false) {
  const events = [];
  while (true) {
    const match = /\r?\n\r?\n/.exec(buffer);
    if (!match) break;
    events.push(buffer.slice(0, match.index));
    buffer = buffer.slice(match.index + match[0].length);
  }
  if (final && buffer.trim()) events.push(buffer);
  return [events, buffer];
}

function parseFrame(frame) {
  const data = frame.split(/\r?\n/)
    .filter(line => line.startsWith('data:'))
    .map(line => line.slice(5).replace(/^ /, '')).join('\n');
  if (!data) return null;
  if (data === '[DONE]') return { type: 'done' };
  try {
    const event = JSON.parse(data);
    if (event && typeof event === 'object' && !Array.isArray(event)) return event;
  } catch { /* malformed events are protocol errors */ }
  throw new DominionError('Invalid DOMINION streaming event', { code: 'STREAM_PROTOCOL' });
}

export class DominionClient {
  constructor({ baseUrl = 'http://127.0.0.1:4317', token = '', headers = {}, fetch: fetcher = globalThis.fetch } = {}) {
    if (typeof fetcher !== 'function') throw new TypeError('A fetch implementation is required');
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    this.token = token;
    this.headers = headers;
    this.fetch = fetcher;
    this.sessions = {
      list: options => this.request('GET', '/api/sessions', undefined, options),
      create: (config = {}, options) => this.request('POST', '/api/sessions', config, options),
      get: (sessionId, options) => this.request('GET', `/api/sessions/${id(sessionId)}`, undefined, options),
      messages: (sessionId, options) => this.request('GET', `/api/sessions/${id(sessionId)}/messages`, undefined, options),
      usage: (sessionId, options) => this.request('GET', `/api/sessions/${id(sessionId)}/usage`, undefined, options),
      cancel: (sessionId, options) => this.request('POST', `/api/sessions/${id(sessionId)}/cancel`, {}, options),
      fork: (sessionId, options) => this.request('POST', `/api/sessions/${id(sessionId)}/fork`, {}, options),
      compact: (sessionId, config, options) => this.request('POST', `/api/sessions/${id(sessionId)}/compact`, config, options),
    };
    this.chat = {
      stream: (request, options) => this.stream('/api/chat', request, options),
      run: (request, options) => this.runChat(request, options),
    };
    this.goals = {
      list: options => this.request('GET', '/api/goals', undefined, options),
      create: (goal, options) => this.request('POST', '/api/goals', goal, options),
      get: (goalId, options) => this.request('GET', `/api/goals/${id(goalId)}`, undefined, options),
      receipt: (goalId, options) => this.request('GET', `/api/goals/${id(goalId)}/receipt`, undefined, options),
      resume: (goalId, body = {}, options) => this.request('POST', `/api/goals/${id(goalId)}/resume`, body, options),
      steer: (goalId, body, options) => this.request('POST', `/api/goals/${id(goalId)}/steer`, body, options),
      editPlan: (goalId, body, options) => this.request('POST', `/api/goals/${id(goalId)}/plan`, body, options),
      diff: (goalId, options) => this.request('GET', `/api/goals/${id(goalId)}/diff`, undefined, options),
      apply: (goalId, digest, options) => this.request('POST', `/api/goals/${id(goalId)}/apply`, { expectedPatchSha256: digest }, options),
      approvals: (goalId, options) => this.request('GET', `/api/goals/${id(goalId)}/approvals`, undefined, options),
      subtasks: (goalId, options) => this.request('GET', `/api/goals/${id(goalId)}/subtasks`, undefined, options),
    };
    this.jobs = {
      list: options => this.request('GET', '/api/jobs', undefined, options),
      get: (jobId, options) => this.request('GET', `/api/jobs/${id(jobId)}`, undefined, options),
      create: (kind, input, options) => this.request('POST', '/api/jobs', { kind, input }, options),
      prompt: (prompt, config = {}, options) => this.request('POST', '/api/jobs', { kind: 'prompt', input: { ...config, prompt } }, options),
      cancel: (jobId, options) => this.request('POST', `/api/jobs/${id(jobId)}/cancel`, {}, options),
      wait: (jobId, options) => this.waitJob(jobId, options),
    };
    this.approvals = {
      list: options => this.request('GET', '/api/approvals', undefined, options),
      resolve: (approvalId, allow, options) => this.request('POST', '/api/approvals', { id: approvalId, allow }, options),
    };
    this.agents = { list: options => this.request('GET', '/api/agents', undefined, options) };
    this.providers = {
      list: options => this.request('GET', '/api/providers', undefined, options),
      models: (providerId, options) => this.request('GET', `/api/providers/${id(providerId)}/models`, undefined, options),
    };
    this.tools = { list: options => this.request('GET', '/api/tools', undefined, options) };
    this.rooms = {
      list: options => this.request('GET', '/api/rooms', undefined, options),
      run: (roomId, input, options) => this.request('POST', `/api/rooms/${id(roomId)}/run`, input, options),
      stop: (roomId, options) => this.request('POST', `/api/rooms/${id(roomId)}/stop`, {}, options),
      messages: (roomId, options) => this.request('GET', `/api/rooms/${id(roomId)}/messages`, undefined, options),
    };
    this.rsi = {
      list: options => this.request('GET', '/api/rsi', undefined, options),
      start: (body, options) => this.request('POST', '/api/rsi/start', body, options),
      campaigns: options => this.request('GET', '/api/rsi/campaigns', undefined, options),
      campaign: (campaignId, options) => this.request('GET', `/api/rsi/campaigns/${id(campaignId)}`, undefined, options),
    };
  }
  url(path) { return `${this.baseUrl}${path}`; }
  async dispatch(method, path, body, { signal, headers = {} } = {}, accept = 'application/json') {
    const requestHeaders = { Accept: accept, ...this.headers, ...headers };
    if (this.token) requestHeaders['X-Dominion-Token'] = this.token;
    if (body !== undefined) requestHeaders['Content-Type'] = 'application/json';
    let response;
    try {
      response = await this.fetch(this.url(path), {
        method, headers: requestHeaders, body: body === undefined ? undefined : JSON.stringify(body),
        signal, cache: 'no-store',
      });
    } catch (cause) {
      if (signal?.aborted) throw cause;
      throw new DominionError(`DOMINION request failed: ${cause.message || cause}`, {
        method, path, code: 'NETWORK_ERROR',
      });
    }
    if (!response.ok) {
      const raw = await response.text();
      let details = null;
      try { details = JSON.parse(raw); } catch { /* preserve server text */ }
      throw new DominionError(reason(details) || (raw.trimStart().startsWith('<') ? `HTTP ${response.status}` : raw.slice(0, 800)) || `HTTP ${response.status}`, {
        status: response.status, method, path, details, code: 'HTTP_ERROR',
      });
    }
    return response;
  }
  async request(method, path, body, options) {
    const response = await this.dispatch(method, path, body, options);
    if (response.status === 204) return null;
    try { return await response.json(); }
    catch { throw new DominionError('Invalid DOMINION JSON response', { status: response.status, method, path, code: 'JSON_PROTOCOL' }); }
  }
  async *stream(path, body, { signal, headers = {} } = {}) {
    const response = await this.dispatch('POST', path, body, { signal, headers }, 'text/event-stream');
    if (!response.body || !response.headers.get('content-type')?.toLowerCase().includes('text/event-stream')) {
      throw new DominionError('DOMINION did not return a streaming response', { status: response.status, path, code: 'STREAM_PROTOCOL' });
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let done = false;
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) break;
        buffer += decoder.decode(next.value, { stream: true });
        const [chunks, remaining] = frames(buffer);
        buffer = remaining;
        for (const chunk of chunks) {
          const event = parseFrame(chunk);
          if (!event) continue;
          yield event;
          if (event.type === 'done') { done = true; return; }
        }
      }
      buffer += decoder.decode();
      const [chunks] = frames(buffer, true);
      for (const chunk of chunks) {
        const event = parseFrame(chunk);
        if (!event) continue;
        yield event;
        if (event.type === 'done') { done = true; return; }
      }
    } finally {
      await reader.cancel().catch(() => {});
    }
    if (!done) throw new DominionError('DOMINION stream ended without a done event', { path, code: 'STREAM_INTERRUPTED' });
  }
  async runChat(body, options) {
    const result = { text: '', reasoning: '', events: [], done: null };
    for await (const event of this.chat.stream(body, options)) {
      result.events.push(event);
      if (event.type === 'text') result.text += event.text || '';
      if (event.type === 'reasoning') result.reasoning += event.text || '';
      if (event.type === 'done') result.done = event;
      if (event.type === 'error') throw new DominionError(event.error || event.message || 'Agent run failed', { code: 'AGENT_ERROR', details: event });
    }
    return result;
  }
  /** Server owns the session transcript. SDK never replays duplicated history. */
  session(sessionId) {
    return {
      id: sessionId,
      messages: options => this.sessions.messages(sessionId, options),
      usage: options => this.sessions.usage(sessionId, options),
      stream: (input, config = {}, options) => this.chat.stream({ ...config, sessionId, input }, options),
      run: (input, config = {}, options) => this.chat.run({ ...config, sessionId, input }, options),
      cancel: options => this.sessions.cancel(sessionId, options),
    };
  }
  async waitJob(jobId, { intervalMs = 400, signal, maxPolls = 300 } = {}) {
    if (!Number.isInteger(intervalMs) || intervalMs < 10 || !Number.isInteger(maxPolls) || maxPolls < 1)
      throw new RangeError('Invalid job polling limits');
    for (let attempt = 0; attempt < maxPolls; attempt++) {
      if (signal?.aborted) throw signal.reason || new Error('Aborted');
      const job = await this.jobs.get(jobId, { signal });
      if (terminal.has(job.status)) return job;
      if (attempt + 1 === maxPolls) break;
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => { signal?.removeEventListener('abort', abort); resolve(); }, intervalMs);
        const abort = () => { clearTimeout(timer); reject(signal.reason || new Error('Aborted')); };
        signal?.addEventListener('abort', abort, { once: true });
        if (signal?.aborted) abort();
      });
    }
    throw new DominionError(`Job ${jobId} did not reach a terminal state`, { code: 'JOB_TIMEOUT' });
  }
}
