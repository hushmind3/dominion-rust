/** DOMINION first-party SDK: Rust owns agents, tools, state, approvals and plans. */
export class DominionError extends Error {
  constructor(message, status = 0, details = null) {
    super(message); this.name = 'DominionError'; this.status = status; this.details = details;
  }
}
const enc = value => encodeURIComponent(String(value));
const TERMINAL = new Set(['completed', 'failed', 'cancelled', 'needs_review']);
const checkAbort = signal => {
  if (signal?.aborted) throw signal.reason ?? new Error('Aborted');
};
function sleep(ms, signal) {
  checkAbort(signal);
  return new Promise((resolve, reject) => {
    const done = () => { signal?.removeEventListener('abort', abort); resolve(); };
    const timer = setTimeout(done, ms);
    const abort = () => { clearTimeout(timer); signal.removeEventListener('abort', abort); reject(signal.reason ?? new Error('Aborted')); };
    signal?.addEventListener('abort', abort, {once: true});
  });
}

/** Incremental SSE parser: split UTF-8, split CRLF, comments, multi-line data. */
export async function* decodeSSE(body) {
  if (!body?.getReader) throw new DominionError('Missing SSE response body');
  const reader = body.getReader(), decoder = new TextDecoder();
  let buffer = '';
  try {
    for (;;) {
      const {done, value} = await reader.read();
      if (done) return; // An incomplete final frame is not a committed event.
      buffer += decoder.decode(value, {stream: true});
      let boundary;
      while ((boundary = /\r\n\r\n|\n\n|\r\r/.exec(buffer))) {
        const frame = buffer.slice(0, boundary.index);
        buffer = buffer.slice(boundary.index + boundary[0].length);
        const data = [];
        let id;
        for (const line of frame.split(/\r\n|\n|\r/)) {
          if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
          else if (line.startsWith('id:')) id = line.slice(3).trim();
        }
        if (data.length) yield {id, data: data.join('\n')};
      }
      if (buffer.length > 2000000) throw new DominionError('Oversized SSE frame');
    }
  } finally {
    await reader.cancel().catch(() => {});
  }
}

export class DominionClient {
  constructor({baseUrl = 'http://127.0.0.1:4317/', token = '', fetch: transport = globalThis.fetch, headers = {}} = {}) {
    this.baseUrl = new URL(baseUrl);
    if (!['http:', 'https:'].includes(this.baseUrl.protocol) || this.baseUrl.username ||
        this.baseUrl.password || this.baseUrl.search || this.baseUrl.hash) {
      throw new TypeError('baseUrl must be an http(s) URL without credentials, query or fragment');
    }
    this.baseUrl.pathname = this.baseUrl.pathname.replace(/\/+$/, '') + '/';
    if (typeof transport !== 'function') throw new TypeError('fetch implementation required');
    this.transport = transport; this.token = token; this.headers = {...headers};
    this.agents = {
      list: options => this.request('GET', 'api/agents', undefined, options),
      save: (agent, options) => this.request('POST', 'api/agents', agent, options)
    };
    this.sessions = {
      list: options => this.request('GET', 'api/sessions', undefined, options),
      create: (body = {}, options) => this.request('POST', 'api/sessions', body, options),
      get: (id, options) => this.request('GET', 'api/sessions/' + enc(id), undefined, options),
      messages: (id, options) => this.request('GET', 'api/sessions/' + enc(id) + '/messages', undefined, options),
      usage: (id, options) => this.request('GET', 'api/sessions/' + enc(id) + '/usage', undefined, options),
      cancel: (id, options) => this.request('POST', 'api/sessions/' + enc(id) + '/cancel', {}, options)
    };
    this.approvals = {
      pending: options => this.request('GET', 'api/approvals', undefined, options),
      resolve: (id, allow, options) => this.request('POST', 'api/approvals', {id, allow: Boolean(allow)}, options)
    };
    this.jobs = {
      create: (body, options) => this.request('POST', 'api/jobs', body, options),
      get: (id, options) => this.request('GET', 'api/jobs/' + enc(id), undefined, options),
      cancel: (id, options) => this.request('POST', 'api/jobs/' + enc(id) + '/cancel', {}, options),
      wait: (id, options) => this.wait('api/jobs', id, options)
    };
    this.goals = {
      create: (body, options) => this.request('POST', 'api/goals', body, options),
      get: (id, options) => this.request('GET', 'api/goals/' + enc(id), undefined, options),
      wait: (id, options) => this.wait('api/goals', id, options),
      run: async (body, options) => {
        const goal = await this.goals.create(body, options);
        if (!goal?.id) throw new DominionError('Goal creation did not return an id');
        return this.goals.wait(goal.id, options);
      },
      resume: (id, options) => this.request('POST', 'api/goals/' + enc(id) + '/resume', {acknowledgeUnknownEffects: true}, options),
      changes: (id, options) => this.request('GET', 'api/goals/' + enc(id) + '/changes', undefined, options),
      applyReviewed: (id, sha256, options) => this.request('POST', 'api/goals/' + enc(id) + '/apply', {expectedPatchSha256: sha256}, options),
      rollback: (id, options) => this.request('POST', 'api/goals/' + enc(id) + '/rollback', {}, options)
    };
    this.chat = {stream: (body, options) => this.streamChat(body, options),
                 run: (body, options) => this.runChat(body, options)};
    this.events = {watch: options => this.watchEvents(options)};
  }

  async raw(method, path, body, {signal} = {}) {
    checkAbort(signal);
    const targetUrl = new URL(path, this.baseUrl);
    if (targetUrl.origin !== this.baseUrl.origin) {
      throw new TypeError('DOMINION SDK requests cannot cross the configured server origin');
    }
    const headers = new Headers(this.headers);
    if (this.token) headers.set('X-Dominion-Token', this.token);
    if (body !== undefined) headers.set('Content-Type', 'application/json');
    const response = await this.transport(targetUrl.toString(), {
      method, headers, body: body === undefined ? undefined : JSON.stringify(body),
      signal, cache: 'no-store', redirect: 'error'
    });
    if (response.ok) return response;
    const text = await response.text().catch(() => '');
    let details = text;
    try { details = JSON.parse(text); } catch {}
    throw new DominionError(
      typeof details?.error === 'string' ? details.error : 'DOMINION HTTP ' + response.status,
      response.status, details
    );
  }
  async request(method, path, body, options) {
    const response = await this.raw(method, path, body, options);
    try { return await response.json(); }
    catch { throw new DominionError('Invalid JSON response at ' + path, response.status); }
  }

  /** A chat turn runs the preexisting native Rust multi-tool agent loop. */
  async *streamChat(body, {signal} = {}) {
    if (typeof body?.input !== 'string') throw new TypeError('A chat input string is required');
    const response = await this.raw('POST', 'api/chat', body, {signal});
    if (!response.headers.get('content-type')?.includes('text/event-stream')) {
      throw new DominionError('Expected text/event-stream from /api/chat', response.status);
    }
    let completed = false;
    for await (const frame of decodeSSE(response.body)) {
      checkAbort(signal);
      let event;
      try { event = JSON.parse(frame.data); }
      catch { throw new DominionError('Malformed chat SSE JSON'); }
      yield event;
      if (event.type === 'done') { completed = true; break; }
    }
    if (!completed) throw new DominionError('Chat stream ended without a done event');
  }
  async runChat(body, {signal, onEvent} = {}) {
    let messageId, error, sessionId = body.sessionId;
    for await (const event of this.streamChat(body, {signal})) {
      onEvent?.(event);
      if (event.type === 'assistant_saved') {
        messageId = event.messageId; sessionId = event.sessionId ?? sessionId;
      }
      if (event.type === 'done') sessionId = event.sessionId ?? sessionId;
      if (event.type === 'error') error = event.message || 'Agent execution failed';
    }
    if (error) throw new DominionError(error);
    if (!messageId || !sessionId) return {sessionId, message: null};
    const records = await this.sessions.messages(sessionId, {signal});
    return {sessionId, message: records.find(row => row.id === messageId) ?? null};
  }

  /** Cancellation here stops the SDK waiter, NOT the durable Rust job. */
  async wait(prefix, id, {signal, timeoutMs = 0, pollIntervalMs = 750, onUpdate} = {}) {
    if (!Number.isFinite(pollIntervalMs) || pollIntervalMs < 1) throw new RangeError('pollIntervalMs must be >= 1');
    const start = Date.now();
    let previous;
    for (;;) {
      checkAbort(signal);
      const record = await this.request('GET', prefix + '/' + enc(id), undefined, {signal});
      const revision = JSON.stringify(record);
      if (revision !== previous) onUpdate?.(record);
      previous = revision;
      if (TERMINAL.has(record.status)) return record;
      const remaining = timeoutMs > 0 ? timeoutMs - (Date.now() - start) : Infinity;
      if (remaining <= 0) throw new DominionError('Wait timed out: ' + prefix + '/' + id);
      await sleep(Math.min(pollIntervalMs, remaining), signal);
    }
  }

  /** Durable SSE replays events and reconnects from the last monotonic sequence. */
  async *watchEvents({after = 0, signal, reconnect = true, reconnectDelayMs = 250} = {}) {
    if (!Number.isSafeInteger(after) || after < 0) throw new RangeError('after must be a nonnegative integer');
    let cursor = after;
    for (;;) {
      checkAbort(signal);
      const response = await this.raw('GET', 'api/events?after=' + cursor, undefined, {signal});
      if (!response.headers.get('content-type')?.includes('text/event-stream')) {
        throw new DominionError('Expected text/event-stream from /api/events');
      }
      for await (const frame of decodeSSE(response.body)) {
        checkAbort(signal);
        let value;
        try { value = JSON.parse(frame.data); }
        catch { throw new DominionError('Malformed event SSE JSON'); }
        const seq = Number(value.seq ?? frame.id);
        if (!Number.isSafeInteger(seq) || seq <= cursor) continue;
        cursor = seq;
        yield value;
      }
      if (!reconnect) return;
      await sleep(reconnectDelayMs, signal);
    }
  }
}
