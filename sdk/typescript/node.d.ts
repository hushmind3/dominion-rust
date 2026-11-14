import type {DominionClient} from "./dist/index.js";
export interface RuntimeConfig {
  binary:string;
  workspace:string;
  dataDir:string;
  webDir?:string;
  port?:number;
  startupTimeoutMs?:number;
}
export declare class DominionRuntime {
  private constructor();
  readonly client:DominionClient;
  readonly instanceId:string;
  readonly port:number;
  static spawn(config:RuntimeConfig):Promise<DominionRuntime>;
  shutdown():Promise<void>;
}
