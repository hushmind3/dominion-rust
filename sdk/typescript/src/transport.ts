import type {RequestOptions} from "./types.js";

export class DominionHttpError extends Error {
  constructor(
    readonly status: number,
    readonly method: string,
    readonly path: string,
    readonly payload: unknown,
  ) {
    const detail = payload && typeof payload === "object" && "error" in payload
      && typeof payload.error === "string" ? payload.error : `HTTP ${status}`;
    super(`${method} ${path}: ${detail}`);
    this.name = "DominionHttpError";
  }
}

export class DominionProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DominionProtocolError";
  }
}

export class DominionStreamInterrupted extends Error {
  constructor(message = "Chat stream ended without a terminal 'done' event; do not replay tool actions automatically.") {
    super(message);
    this.name = "DominionStreamInterrupted";
  }
}

export type FetchImplementation = typeof fetch;

export interface TransportOptions {
  baseUrl?: string;
  adminToken?: string;
  fetch?: FetchImplementation;
  timeoutMs?: number;
}

/** Only HTTP calls cross this boundary: no remote SDK, subprocess, or hidden agent engine. */
export class DominionTransport {
  readonly baseUrl: string;
  private readonly token: string | undefined;
  private readonly fetchImpl: FetchImplementation;
  private readonly timeoutMs: number;

  constructor(options: TransportOptions = {}) {
    const url = new URL(options.baseUrl ?? "http://127.0.0.1:4317");
    if (!["http:", "https:"].includes(url.protocol)
      || url.username || url.password || url.search || url.hash) {
      throw new TypeError("baseUrl must be an HTTP(S) URL without credentials or query parameters");
    }
    this.baseUrl = url.toString().replace(/\/+$/, "");
    this.token = options.adminToken;
    this.fetchImpl = options.fetch ?? fetch;
    this.timeoutMs = options.timeoutMs ?? 30_000;
  }

  private url(path: string): string {
    if (!path.startsWith("/api/")) throw new TypeError("Only Dominion API paths are accepted");
    return this.baseUrl + path;
  }

  private headers(body: unknown, accept: string): Headers {
    const headers = new Headers({Accept: accept});
    if (body !== undefined) headers.set("Content-Type", "application/json");
    if (this.token) headers.set("X-Dominion-Token", this.token);
    return headers;
  }

  private async send(
    path: string, method: string, body: unknown, options: RequestOptions,
    accept: string, streaming: boolean,
  ): Promise<Response> {
    const controller = new AbortController();
    const cancel = () => controller.abort(options.signal?.reason);
    if (options.signal?.aborted) cancel();
    else options.signal?.addEventListener("abort", cancel, {once: true});
    const timeout = streaming ? undefined : setTimeout(
      () => controller.abort(new Error("Dominion HTTP request timed out")),
      options.timeoutMs ?? this.timeoutMs,
    );
    try {
      const response = await this.fetchImpl(this.url(path), {
        method, headers: this.headers(body, accept),
        body: body === undefined ? undefined : JSON.stringify(body),
        // SSE keeps the original signal attached after headers arrive;
        // otherwise cancelling a long-running turn would stop neither
        // the socket nor the consuming async iterator.
        signal: streaming ? options.signal : controller.signal, cache: "no-store",
      });
      if (!response.ok) {
        const raw = (await response.text()).slice(0, 4_096);
        let payload: unknown = raw;
        try { payload = JSON.parse(raw); } catch { /* preserve body for diagnostics */ }
        throw new DominionHttpError(response.status, method, path, payload);
      }
      if (streaming && !response.headers.get("content-type")?.toLowerCase().startsWith("text/event-stream")) {
        throw new DominionProtocolError(`${method} ${path}: expected text/event-stream`);
      }
      return response;
    } finally {
      options.signal?.removeEventListener("abort", cancel);
      if (timeout !== undefined) clearTimeout(timeout);
    }
  }

  async json<T>(path: string, method = "GET", body?: unknown, options: RequestOptions = {}): Promise<T> {
    const response = await this.send(path, method, body, options, "application/json", false);
    const raw = await response.text();
    try { return JSON.parse(raw) as T; }
    catch { throw new DominionProtocolError(`${method} ${path}: expected valid JSON response`); }
  }

  async sse(path: string, method = "GET", body?: unknown, options: RequestOptions = {}): Promise<ReadableStream<Uint8Array>> {
    const response = await this.send(path, method, body, options, "text/event-stream", true);
    if (!response.body) throw new DominionProtocolError(`${method} ${path}: SSE response body is missing`);
    return response.body;
  }
}

export interface SseFrame { data: string; id?: string; event?: string; }

/** Standards-compatible SSE framing across split UTF-8, split CRLF and multi-line data. */
export async function* decodeSse(
  stream: ReadableStream<Uint8Array>,
): AsyncGenerator<SseFrame> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    while (true) {
      const {done, value} = await reader.read();
      if (done) {
        buffer += decoder.decode();
        if (buffer.trim()) throw new DominionStreamInterrupted("SSE ended mid-frame; stream cannot be treated as complete");
        return;
      }
      buffer += decoder.decode(value, {stream: true});
      if (buffer.length > 2_000_000) throw new DominionProtocolError("SSE event exceeds the 2 MB buffer limit");
      let boundary: RegExpExecArray | null;
      while ((boundary = /\r\n\r\n|\n\n|\r\r/.exec(buffer))) {
        const block = buffer.slice(0, boundary.index);
        buffer = buffer.slice(boundary.index + boundary[0].length);
        const data: string[] = [];
        let id: string | undefined;
        let event: string | undefined;
        for (const line of block.split(/\r\n|\n|\r/)) {
          if (!line || line.startsWith(":")) continue;
          const colon = line.indexOf(":");
          const field = colon < 0 ? line : line.slice(0, colon);
          const rawValue = colon < 0 ? "" : line.slice(colon + 1);
          const content = rawValue.startsWith(" ") ? rawValue.slice(1) : rawValue;
          if (field === "data") data.push(content);
          else if (field === "id" && !content.includes("\0")) id = content;
          else if (field === "event") event = content;
        }
        if (data.length) yield {data: data.join("\n"), ...(id === undefined ? {} : {id}), ...(event === undefined ? {} : {event})};
      }
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}

export function parseSseJson<T extends {type: string}>(frame: SseFrame): T {
  let payload: unknown;
  try { payload = JSON.parse(frame.data); }
  catch { throw new DominionProtocolError("SSE event contained malformed JSON"); }
  if (!payload || typeof payload !== "object" || Array.isArray(payload)
    || typeof (payload as {type?: unknown}).type !== "string") {
    throw new DominionProtocolError("SSE JSON event must be an object with a type string");
  }
  return payload as T;
}

export async function delay(ms: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) throw signal.reason ?? new Error("Aborted");
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => { signal?.removeEventListener("abort", onAbort); resolve(); }, ms);
    function onAbort() {
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
      reject(signal?.reason ?? new Error("Aborted"));
    }
    signal?.addEventListener("abort", onAbort, {once: true});
  });
}
