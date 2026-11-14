/** Zero-dependency tool extension host for DOMINION's native Rust MCP client. */
import { createServer } from 'node:http';

const modern = '2026-07-28';
const legacy = '2024-11-05';
const json = (res, status, data) => {
  const body = JSON.stringify(data);
  res.writeHead(status, { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body), 'cache-control': 'no-store' });
  res.end(body);
};
const error = (id, code, message) => ({ jsonrpc: '2.0', id: id ?? null, error: { code, message } });
const reply = (id, result) => ({ jsonrpc: '2.0', id, result });
const textValue = value => typeof value === 'string' ? value : JSON.stringify(value);

/** Register JavaScript tools without replacing DOMINION's Rust agent runtime. */
export class DominionToolServer {
  constructor({ name = 'dominion-tools', version = '0.1.0', token = '', maxBodyBytes = 1024 * 1024 } = {}) {
    this.name = name;
    this.version = version;
    this.token = token;
    this.maxBodyBytes = maxBodyBytes;
    this.tools = new Map();
    this.http = null;
    this.url = null;
  }
  tool(name, { description = '', inputSchema = { type: 'object', properties: {} } } = {}, handler) {
    if (!/^[a-zA-Z][a-zA-Z0-9_.-]{0,127}$/.test(name)) throw new TypeError('Invalid MCP tool name');
    if (typeof handler !== 'function') throw new TypeError('Tool handler must be a function');
    if (!inputSchema || inputSchema.type !== 'object') throw new TypeError('Tool requires an object JSON schema');
    if (this.tools.has(name)) throw new Error(`Tool already registered: ${name}`);
    this.tools.set(name, { description, inputSchema, handler });
    return this;
  }
  async listen({ host = '127.0.0.1', port = 0 } = {}) {
    if (this.http) throw new Error('Tool server is already listening');
    if (!['127.0.0.1', 'localhost', '::1'].includes(host) && !this.token) {
      throw new Error('Non-loopback MCP requires a configured bearer token');
    }
    const http = createServer((req, res) => void this.handle(req, res));
    try {
      await new Promise((resolve, reject) => {
        http.once('error', reject);
        http.listen(port, host, resolve);
      });
    } catch (e) { http.close(); throw e; }
    const address = http.address();
    const hostname = host === '::1' ? '[::1]' : host;
    this.url = `http://${hostname}:${address.port}/mcp`;
    this.http = http;
    return this.url;
  }
  async close() {
    if (!this.http) return;
    const server = this.http;
    this.http = null;
    this.url = null;
    await new Promise((resolve, reject) => server.close(e => e ? reject(e) : resolve()));
  }
  async handle(req, res) {
    const route = req.url?.split('?')[0];
    if (route !== '/mcp' || req.method !== 'POST') return json(res, 404, error(null, -32601, 'Method not found'));
    if (req.headers.origin) return json(res, 403, error(null, -32000, 'Origin not allowed'));
    if (this.token && req.headers.authorization !== `Bearer ${this.token}`) {
      return json(res, 401, error(null, -32001, 'Unauthorized'));
    }
    let bytes = 0;
    const chunks = [];
    try {
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > this.maxBodyBytes) return json(res, 413, error(null, -32700, 'Request too large'));
        chunks.push(chunk);
      }
    } catch { return json(res, 400, error(null, -32700, 'Invalid request stream')); }
    let payload;
    try { payload = JSON.parse(Buffer.concat(chunks).toString('utf8')); }
    catch { return json(res, 400, error(null, -32700, 'Invalid JSON')); }
    if (!payload || typeof payload !== 'object' || Array.isArray(payload) || payload.jsonrpc !== '2.0'
        || typeof payload.method !== 'string') {
      return json(res, 400, error(payload?.id, -32600, 'Invalid JSON-RPC request'));
    }
    const protocol = req.headers['mcp-protocol-version'];
    const meta = payload.params?._meta?.['io.modelcontextprotocol/protocolVersion'];
    if (protocol || meta) {
      if (protocol !== modern || meta !== modern || req.headers['mcp-method'] !== payload.method ||
          (payload.method === 'tools/call' && req.headers['mcp-name'] !== payload.params?.name)) {
        return json(res, 400, error(payload.id, -32020, 'MCP headers and metadata mismatch'));
      }
    }
    if (!Object.hasOwn(payload, 'id')) { res.writeHead(202); res.end(); return; }
    const id = payload.id;
    switch (payload.method) {
      case 'initialize':
        return json(res, 200, reply(id, {
          protocolVersion: payload.params?.protocolVersion === legacy ? legacy : modern,
          capabilities: { tools: { listChanged: false } }, serverInfo: { name: this.name, version: this.version },
        }));
      case 'ping': return json(res, 200, reply(id, {}));
      case 'tools/list':
        return json(res, 200, reply(id, { tools: [...this.tools].map(([name, tool]) => ({
          name, description: tool.description, inputSchema: tool.inputSchema,
        })) }));
      case 'tools/call': {
        const tool = this.tools.get(payload.params?.name);
        if (!tool) return json(res, 404, error(id, -32602, 'Unknown tool'));
        const args = payload.params?.arguments ?? {};
        if (!args || typeof args !== 'object' || Array.isArray(args)) {
          return json(res, 400, error(id, -32602, 'Tool arguments must be an object'));
        }
        try {
          const output = await tool.handler(args, { requestId: id, toolName: payload.params.name });
          const result = output && typeof output === 'object' && Array.isArray(output.content) ? output :
            { content: [{ type: 'text', text: textValue(output ?? null) }], isError: false };
          return json(res, 200, reply(id, result));
        } catch (e) {
          return json(res, 200, reply(id, { content: [{ type: 'text', text: e?.message || 'Tool failed' }], isError: true }));
        }
      }
      default: return json(res, 404, error(id, -32601, 'Method not found'));
    }
  }
}
