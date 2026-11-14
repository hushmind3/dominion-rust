import test from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {DominionRuntime} from '../node.js';

const fakeProcess = () => {
  const child = new EventEmitter();
  child.exitCode = null;
  child.signalCode = null;
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  child.stdin = {end() {child.exitCode = 0; child.emit('close', 0, null);}};
  child.kill = () => {child.exitCode = 1; child.emit('close', 1, 'SIGTERM');};
  queueMicrotask(() => child.stdout.emit('data', Buffer.from('DOMINION (rust engine) listening on http://127.0.0.1:47311\n')));
  return child;
};

test('Node SDK owns only its launched Rust instance and releases on stdin EOF', async () => {
  let spawnOptions, child;
  const runtime = await DominionRuntime.start({
    binaryPath: '/opt/dominion-rust', port: 0,
    spawnProcess: (_file, _args, options) => {
      spawnOptions = options; child = fakeProcess(); return child;
    },
    fetch: async () => new Response(JSON.stringify({
      service:'dominion', runtime:'rust',
      instanceId: spawnOptions.env.DOMINION_ENGINE_INSTANCE
    }), {headers: {'content-type':'application/json'}})
  });
  assert.equal(runtime.port, 47311);
  assert.equal(runtime.client.baseUrl.port, '47311');
  assert.equal(spawnOptions.env.DOMINION_PORT, '0');
  assert.equal(spawnOptions.env.DOMINION_DESKTOP_PARENT, '1');
  await runtime.close();
  assert.equal(child.exitCode, 0);
});

test('Node SDK refuses to spawn an unconfigured binary path', async () => {
  await assert.rejects(DominionRuntime.start({}), /binaryPath/);
});
