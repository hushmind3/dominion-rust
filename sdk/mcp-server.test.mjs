import test from 'node:test';
import assert from 'node:assert/strict';
import { DominionToolServer } from './mcp-server.mjs';

async function rpc(url, method, params = {}, id = 1, headers = {}) {
  const response = await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify({ jsonrpc: '2.0', id, method, params }) });
  return { status: response.status, body: await response.json() };
}

test('Rust legacy MCP handshake, tool discovery and real custom tool execution', async () => {
  const sdk = new DominionToolServer({ name: 'calculator' });
  sdk.tool('sum', { description: 'add numbers', inputSchema: { type: 'object', properties: { a: {type:'number'}, b: {type:'number'} }, required: ['a','b'] } },
    ({a,b}) => a + b);
  const url = await sdk.listen();
  try {
    let r = await rpc(url, 'initialize', { protocolVersion: '2024-11-05' });
    assert.equal(r.body.result.protocolVersion, '2024-11-05');
    r = await rpc(url, 'tools/list');
    assert.equal(r.body.result.tools[0].name, 'sum');
    r = await rpc(url, 'tools/call', { name: 'sum', arguments: { a: 2, b: 3 } });
    assert.equal(r.body.result.content[0].text, '5');
  } finally { await sdk.close(); }
});

test('2026-07-28 MCP metadata invokes the same developer-defined tool', async () => {
  const sdk = new DominionToolServer();
  sdk.tool('echo', {}, ({value}) => value);
  const url = await sdk.listen();
  try {
    const headers = { 'mcp-protocol-version': '2026-07-28', 'mcp-method': 'tools/call', 'mcp-name': 'echo' };
    const params = { name:'echo', arguments: { value: 'hello' }, _meta: { 'io.modelcontextprotocol/protocolVersion': '2026-07-28' } };
    let r = await rpc(url, 'tools/call', params, 5, headers);
    assert.equal(r.body.result.content[0].text, 'hello');
    r = await rpc(url, 'tools/call', params, 6, { ...headers, 'mcp-name': 'wrong' });
    assert.equal(r.status, 400);
  } finally { await sdk.close(); }
});

test('origin and bearer guards prevent cross-site tool execution', async () => {
  const sdk = new DominionToolServer({ token: 'secret' });
  sdk.tool('echo', {}, ({text}) => text);
  const url = await sdk.listen();
  try {
    let r = await rpc(url, 'tools/list');
    assert.equal(r.status, 401);
    r = await rpc(url, 'tools/list', {}, 1, { authorization: 'Bearer secret', origin: 'https://attacker.example' });
    assert.equal(r.status, 403);
    r = await rpc(url, 'tools/list', {}, 1, { authorization: 'Bearer secret' });
    assert.equal(r.status, 200);
  } finally { await sdk.close(); }
});
