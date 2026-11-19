// Shared pure estimates for the Hugging Face model browser.
// Do not add GPU VRAM to system RAM: MLX uses unified memory.
export type ModelCandidate={id:string;format:'gguf'|'mlx';bytes:number}
export type MemoryFit='comfortable'|'tight'|'insufficient'|'unknown'
export type RuntimeFailure='memory'|'file'|'engine'|'other'
export const MEMORY_HEADROOM=1.3

export function requiredMemory(bytes:number):number{
 return Number.isFinite(bytes)&&bytes>0?Math.ceil(bytes*MEMORY_HEADROOM):0
}

export function memoryFit(bytes:number,available:number):MemoryFit{
 if(!Number.isFinite(available)||available<=0||requiredMemory(bytes)===0)return 'unknown'
 return available>=requiredMemory(bytes)?'comfortable':available>=bytes?'tight':'insufficient'
}

export function selectRecommended<T extends ModelCandidate>(options:readonly T[],availableRam:number,availableGpu:number):{option:T|null;basis:'gpu'|'ram'|null}{
 const eligible=options.filter(o=>o.bytes>0&&Number.isFinite(o.bytes))
 const gpu=eligible.filter(o=>o.format==='gguf'&&memoryFit(o.bytes,availableGpu)==='comfortable').sort((a,b)=>b.bytes-a.bytes)[0]
 if(gpu)return {option:gpu,basis:'gpu'}
 const ram=eligible.filter(o=>memoryFit(o.bytes,availableRam)==='comfortable').sort((a,b)=>b.bytes-a.bytes)[0]
 return {option:ram||null,basis:ram?'ram':null}
}

export function selectSmaller<T extends ModelCandidate>(options:readonly T[],currentBytes:number,availableRam:number,availableGpu:number):{option:T|null;fits:boolean}{
 const smaller=options.filter(o=>o.bytes>0&&o.bytes<currentBytes)
 const best=selectRecommended(smaller,availableRam,availableGpu).option
 if(best)return {option:best,fits:true}
 return {option:smaller.sort((a,b)=>a.bytes-b.bytes)[0]||null,fits:false}
}

export function classifyRuntimeFailure(error:string):RuntimeFailure{
 const message=error.toLowerCase()
 if(/out.of.memory|\boom\b|memory|메모리|allocat|cuda.*alloc|not enough resources/i.test(message))return 'memory'
 if(/not.found|no.such.file|missing.file|corrupt|invalid.gguf|checksum|파일.*없|파일.*찾을 수 없|손상|불완전|완전하지|safetensors|tokenizer|찾을 수 없습니다.*모델|모델.*찾을 수 없습니다/i.test(message))return 'file'
 if(/backend|engine|executable|llama[.-]?(?:cpp|server)|mlx.*실행기|not.installed|설치.*안|spawn|실행기.*필요|실행기.*없/i.test(message))return 'engine'
 return 'other'
}

export type LaunchDownloadAction='start'|'wait'|'resume'|'installed'
export function planDownloadLaunch(status:string|undefined,installed:boolean):LaunchDownloadAction{
 if(installed)return 'installed'
 if(status==='resolving'||status==='downloading')return 'wait'
 if(status==='paused'||status==='failed'||status==='cancelled')return 'resume'
 return 'start'
}

export type ServerModelFormat='gguf'|'mlx'
export function preferredServerModelFormat(
 platform:string|undefined,
 arch:string|undefined,
 mlx:{available?:boolean;installable?:boolean}|null|undefined,
 gguf:{available?:boolean}|null|undefined
):ServerModelFormat|null{
 if(!platform)return null
 // MLX only targets Apple Silicon on the actual Rust server.
 // Prefer an installed execution engine over requiring a fresh install.
 const appleSilicon=platform==='macos'&&(arch==='aarch64'||arch==='arm64')
 if(!appleSilicon)return 'gguf'
 if(mlx?.available===true)return 'mlx'
 if(gguf?.available===true)return 'gguf'
 return mlx?.installable===true?'mlx':'gguf'
}

export type LocalModelToLaunch={id:string;format:'gguf'|'mlx';bytes:number}
export type LaunchCapabilities={
 gguf?:{available?:boolean;path?:string|null}
 mlx?:{available?:boolean;standalone?:boolean}
 lmstudio?:{available?:boolean}
 memory?:{free?:number}
 gpu?:{free?:number}|null
}
export type ModelPreflight={status:'ready'|'warning'|'blocked'|'unknown';reason:'engine'|'memory'|'none';engine:string;memory:MemoryFit;gpu:MemoryFit}
// This is an advisory check. File size is only a weight-memory approximation;
// the runtime owns actual allocations (KV cache, offload, fragmentation).
export function modelPreflight(model:LocalModelToLaunch,capabilities:LaunchCapabilities|undefined):ModelPreflight{
 const engine=model.id.startsWith('lms:')?'LM Studio':model.format==='mlx'?'MLX (mlx-lm)':'llama.cpp'
 const ram=memoryFit(model.bytes,capabilities?.memory?.free||0)
 const gpu=model.format==='gguf'?memoryFit(model.bytes,capabilities?.gpu?.free||0):'unknown'
 const memory=ram==='comfortable'||gpu==='comfortable'?'comfortable'
  :ram==='tight'||gpu==='tight'?'tight'
  :ram==='insufficient'&&gpu==='insufficient'?'insufficient'
  :ram==='insufficient'&&model.format==='mlx'?'insufficient'
  :ram==='insufficient'&&(!capabilities?.gpu||capabilities.gpu.free===0)?'insufficient':'unknown'
 let engineAvailable:boolean|null=null
 if(model.id.startsWith('lms:')){
  if(capabilities?.lmstudio?.available!==undefined)engineAvailable=capabilities.lmstudio.available
 }else if(model.format==='mlx'){
  if(model.id.startsWith('mlx:')){
   if(capabilities?.mlx?.available!==undefined)engineAvailable=capabilities.mlx.available
  }else if(capabilities?.mlx?.standalone!==undefined)engineAvailable=capabilities.mlx.standalone
 }else if(capabilities?.gguf?.path!==undefined){
  engineAvailable=Boolean(capabilities.gguf.path)
 }else if(capabilities?.gguf?.available===false){
  engineAvailable=false
 }
 if(engineAvailable===false)return {status:'blocked',reason:'engine',engine,memory,gpu}
 if(memory==='insufficient'||memory==='tight')return {status:'warning',reason:'memory',engine,memory,gpu}
 if(engineAvailable===null||memory==='unknown')return {status:'unknown',reason:'none',engine,memory,gpu}
 return {status:'ready',reason:'none',engine,memory,gpu}
}

export type ManagedEngineId='llama-cpp'|'mlx-lm'|'lm-studio'
export function engineForModel(model:LocalModelToLaunch):ManagedEngineId{
 if(model.id.startsWith('lms:'))return 'lm-studio'
 return model.format==='mlx'?'mlx-lm':'llama-cpp'
}
