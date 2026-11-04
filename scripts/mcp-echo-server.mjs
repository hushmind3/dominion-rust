#!/usr/bin/env node
// Minimal MCP stdio server used to verify the Rust MCP client.
// Not part of the runtime: this is a development fixture.
import readline from 'node:readline'

const send = (payload) => process.stdout.write(JSON.stringify(payload) + '\n')

const rl = readline.createInterface({ input: process.stdin })
rl.on('line', (line) => {
  let message
  try { message = JSON.parse(line) } catch { return }
  switch (message.method) {
    case 'initialize':
      send({
        jsonrpc: '2.0',
        id: message.id,
        result: {
          protocolVersion: '2024-11-05',
          capabilities: { tools: {} },
          serverInfo: { name: 'echo-lab', version: '1.0.0' },
        },
      })
      break
    case 'notifications/initialized':
      break
    case 'tools/list':
      send({
        jsonrpc: '2.0',
        id: message.id,
        result: {
          tools: [{
            name: 'echo',
            description: 'Echo text back with a prefix',
            inputSchema: {
              type: 'object',
              properties: { text: { type: 'string' } },
              required: ['text'],
            },
          }],
        },
      })
      break
    case 'tools/call': {
      const args = message.params?.arguments || {}
      send({
        jsonrpc: '2.0',
        id: message.id,
        result: { content: [{ type: 'text', text: 'MCP_ECHO: ' + String(args.text ?? '') }] },
      })
      break
    }
    default:
      if (message.id !== undefined) {
        send({ jsonrpc: '2.0', id: message.id, error: { code: -32601, message: 'Method not found' } })
      }
  }
})
