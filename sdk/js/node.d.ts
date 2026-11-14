import type {DominionClient} from './index.js';
import type {ChildProcessWithoutNullStreams} from 'node:child_process';
export interface RuntimeOptions {
  binaryPath: string;
  args?: string[];
  cwd?: string;
  dataDir?: string;
  workspace?: string;
  port?: number; // 0 lets Rust choose an available localhost port.
  token?: string;
  startupTimeoutMs?: number;
  signal?: AbortSignal;
  fetch?: typeof fetch;
  onOutput?: (chunk: string, stream: 'stdout' | 'stderr') => void;
}
export class DominionRuntime {
  readonly process: ChildProcessWithoutNullStreams;
  readonly client: DominionClient;
  readonly port: number;
  static start(options: RuntimeOptions): Promise<DominionRuntime>;
  close(options?: {timeoutMs?: number}): Promise<void>;
}
