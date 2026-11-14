import {spawn as defaultSpawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {DominionClient, DominionError} from './index.js';

/** Run only the DOMINION Rust binary. No third-party agent runtime or CLI. */
export class DominionRuntime {
  constructor({process, client, port}) {this.process = process; this.client = client; this.port = port;}

  static async start({binaryPath, args = [], cwd, dataDir, workspace, port = 0,
    token = '', startupTimeoutMs = 15000, signal, fetch: fetcher = globalThis.fetch,
    spawnProcess = defaultSpawn, onOutput} = {}) {
    if (!binaryPath || typeof binaryPath !== 'string') throw new TypeError('binaryPath must point to the DOMINION Rust binary');
    if (!Number.isInteger(port) || port < 0 || port > 65535) throw new RangeError('port must be 0..65535');
    if (!Number.isFinite(startupTimeoutMs) || startupTimeoutMs < 1) throw new RangeError('startupTimeoutMs must be positive');
    if (signal?.aborted) throw signal.reason ?? new Error('Aborted');
    const instanceId = randomUUID();
    const child = spawnProcess(binaryPath, args, {
      cwd, stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        DOMINION_ENGINE_INSTANCE: instanceId,
        DOMINION_PORT: String(port),
        DOMINION_DESKTOP_PARENT: '1',
        ...(dataDir ? {DOMINION_DATA: dataDir} : {}),
        ...(workspace ? {DOMINION_WORKSPACE: workspace} : {})
      }
    });
    let output = '';
    try {
      const actualPort = await new Promise((resolve, reject) => {
        let line = '';
        const dispose = () => {
          clearTimeout(timeout);
          child.stdout?.off('data', stdout);
          child.stderr?.off('data', stderr);
          child.off('error', error);
          child.off('close', closed);
          signal?.removeEventListener('abort', aborted);
        };
        const fail = reason => {dispose(); reject(reason);};
        const stdout = chunk => {
          const text = String(chunk);
          try {onOutput?.(text, 'stdout');} catch {}
          output = (output + text).slice(-8192);
          line = (line + text).slice(-8192);
          const match = /DOMINION \(rust engine\) listening on http:\/\/127\.0\.0\.1:(\d+)/.exec(line);
          if (match) {dispose(); resolve(Number(match[1]));}
        };
        const stderr = chunk => {
          const text = String(chunk);
          try {onOutput?.(text, 'stderr');} catch {}
          output = (output + text).slice(-8192);
        };
        const error = cause => fail(cause);
        const closed = (code, kind) => fail(new DominionError('DOMINION exited during startup (' + (code ?? kind) + '): ' + output));
        const aborted = () => fail(signal.reason ?? new Error('Aborted'));
        const timeout = setTimeout(() => fail(new DominionError('DOMINION startup timed out: ' + output)), startupTimeoutMs);
        child.stdout?.on('data', stdout);
        child.stderr?.on('data', stderr);
        child.on('error', error);
        child.on('close', closed);
        signal?.addEventListener('abort', aborted, {once:true});
      });
      const url = 'http://127.0.0.1:' + actualPort;
      const deadline = Date.now() + startupTimeoutMs;
      for (;;) {
        if (signal?.aborted) throw signal.reason ?? new Error('Aborted');
        if (child.exitCode !== null) throw new DominionError('DOMINION process exited before health check');
        try {
          const response = await fetcher(url + '/api/health', {signal});
          const health = response.ok && await response.json();
          if (health?.service === 'dominion' &&
              health?.runtime === 'rust' &&
              health?.instanceId === instanceId) {
            return new DominionRuntime({
              process: child,
              client: new DominionClient({baseUrl: url, token, fetch: fetcher}),
              port: actualPort
            });
          }
        } catch (error) {
          if (signal?.aborted) throw signal.reason ?? error;
        }
        if (Date.now() >= deadline) {
          throw new DominionError('DOMINION did not become healthy with matching instance id');
        }
        await new Promise(resolve => setTimeout(resolve, 80));
      }
    } catch (error) {
      child.stdin?.end();
      child.kill?.('SIGTERM');
      throw error;
    }
  }

  async close({timeoutMs = 3000} = {}) {
    const child = this.process;
    if (child.exitCode !== null || child.signalCode !== null) return;
    await new Promise(resolve => {
      let settled = false;
      const finish = () => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        child.off('close', finish);
        resolve();
      };
      const timer = setTimeout(() => {child.kill('SIGTERM'); finish();}, timeoutMs);
      child.on('close', finish);
      child.stdin.end(); // DOMINION_DESKTOP_PARENT: clean shutdown on EOF.
    });
  }
}
