#!/usr/bin/env node
/**
 * Hard, deterministic source-tree hygiene gate.
 * Runs before builds in CI. It NEVER deletes files; violation => nonzero exit.
 * Every exception is a size-only ratchet of an inherited source file.
 */
import {execFileSync} from 'node:child_process'
import {readFileSync,statSync} from 'node:fs'
import {dirname,posix,resolve} from 'node:path'
import {fileURLToPath} from 'node:url'

const ROOT=resolve(fileURLToPath(new URL('..',import.meta.url)))
const BIG_SOURCE_LIMIT=48*1024

// Existing large modules cannot grow by one byte. New modules have a 48 KiB
// ceiling. Reduce these budgets as modules are refactored; never raise them.
export const inheritedLargeSources=Object.freeze({
  'src/engine/models.rs':107905,
  'src/engine/chat/runtime/turn.rs':106499,
  'web/src/i18n/ko.ts':80377,
  'src/engine/chat/provider.rs':79167,
  'src/api/providers.rs':79129,
  'src/engine/tools.rs':77796,
  'web/src/i18n/en.ts':72411,
  'src/engine/algorithms.rs':71641,
  'src/bin/dominion-benchmark.rs':65919,
  'src/engine/memory.rs':62532,
  'src/engine/rsi/execution/campaign.rs':60747,
  'src/engine/rsi_system.rs':60220,
  'src/engine/rsi/evaluation/worktree.rs':59888,
  'web/src/pages/Rooms.tsx':56635,
  'src/db.rs':54902,
  'web/src/pages/Extensions.tsx':52493,
  'src/engine/chat/runtime/program.rs':52048,
})

const generatedSegment=/(^|\/)(?:node_modules|target|dist|coverage|__pycache__|\.pytest_cache|\.cache|\.tmp|temp|tmp|logs|artifacts)(\/|$)/
const tempExtension=/(?:\.tmp|\.bak|\.orig|\.rej|\.swp|\.swo|\.sqlite-(?:wal|shm|journal)|\.part(?:\.[a-f0-9]{40})?|\.dominion-previous|\.profraw|\.profdata|\.log|\.trace)$/
const compiledExtension=/\.(?:exe|dll|so|dylib|o|rlib|rmeta|class|pyc|pyo)$/
const bannedSource=/(?:^|[/"'\x60])src\/(?:engine|api)\/goals\.rs\b/g
const modReference=/#\[path\s*=\s*"([^"]+)"\]/g

/**
 * Files are {path,size,sha,content}. A read-only injected manifest is used by
 * regression tests, so this guard's ability to fail is tested without
 * committing a real build artifact or creating dirty repository state.
 */
export function auditFiles(files){
  const errors=[]
  const paths=new Set(files.map(file=>file.path))
  const sources=new Map()
  const hashes=new Map()
  let docs=0
  let total=0
  for(const file of files){
    const path=file.path.replaceAll('\\','/')
    const size=file.size
    total+=size
    if(path.startsWith('docs/') && path.endsWith('.md'))docs++
    if(generatedSegment.test('/'+path)||tempExtension.test(path)||compiledExtension.test(path)
      ||/^(?:reports|logs)\//.test(path)){
      errors.push('generated/temporary file committed: '+path)
    }
    if(path==='src/engine/goals.rs'||path==='src/api/goals.rs'){
      errors.push('obsolete goal monolith reintroduced: '+path)
    }
    if(size>1024*1024){
      errors.push('tracked file exceeds 1 MiB: '+path)
    }
    if(path==='README.md' && size>20*1024){
      errors.push('README changelog bloat >20 KiB: '+path)
    }
    if((path==='src/engine/goals/mod.rs'||path==='src/api/goals/mod.rs')&&size>8*1024){
      errors.push('goal module entry >8 KiB: '+path)
    }
    if(path.startsWith('tests/')&&path.endsWith('.rs')&&size>400*1024){
      errors.push('overgrown Rust test file >400 KiB: '+path)
    }
    if((path.startsWith('src/')||path.startsWith('web/src/'))
        && /\.(?:rs|tsx?|jsx?|mjs)$/.test(path)){
      const cap=inheritedLargeSources[path]??BIG_SOURCE_LIMIT
      if(size>cap)errors.push('source grew beyond its non-regression size budget: '+path+
        ' ('+size+' > '+cap+' bytes)')
    }
    // Exact duplicate source/doc binary data is wasteful. Ignore empty
    // .gitkeep markers, which intentionally keep otherwise empty directories.
    if(file.sha && size>0 && !path.endsWith('/.gitkeep')){
      const existing=hashes.get(file.sha)
      if(existing)errors.push('duplicate tracked content: '+existing+' == '+path)
      else hashes.set(file.sha,path)
    }
    if(file.content!==undefined) sources.set(path,file.content)
  }
  if(docs>25) errors.push('documentation snapshot count exceeds 25: '+docs)
  if(!paths.has('src/engine/goals/mod.rs')||!paths.has('src/api/goals/mod.rs')){
    errors.push('canonical directory-based goals modules missing')
  }
  for(const [path,body] of sources){
    if(!path.endsWith('.rs')&&!path.endsWith('.mjs')&&path!=='README.md'
      &&path!=='ENGINE.md')continue
    // The checker itself quotes the obsolete path in a detection rule.
    if(path!=='scripts/check-repo-hygiene.mjs'
        &&bannedSource.test(body)){
      errors.push('stale deleted goals.rs reference in '+path)
    }
    bannedSource.lastIndex=0
    if(path.endsWith('.rs')){
      for(const match of body.matchAll(modReference)){
        const ref=posix.normalize(posix.join(posix.dirname(path),match[1]))
        if(!paths.has(ref)){
          errors.push('dangling Rust #[path] module: '+path+' => '+ref)
        }
      }
      // Source-index literal file lists are execution inputs for RSI. Never
      // let them silently point at code deleted during cleanup.
      if(path==='src/engine/rsi_system.rs'){
        const roleList=body.match(/const COMPONENTS:[\s\S]*?\n\];/)?.[0]??''
        for(const match of roleList.matchAll(/"(src\/[^"]+\.(?:rs|ts|tsx))"/g)){
          if(!paths.has(match[1]))errors.push('missing RSI indexed source: '+match[1])
        }
      }
    }
  }
  return {errors,files:files.length,bytes:total,docs}
}

export function auditRepository(root=ROOT){
  const raw=execFileSync('git',['ls-files','--stage','-z'],
    {cwd:root,maxBuffer:16*1024*1024}).toString('utf8')
  const files=[]
  for(const entry of raw.split('\0').filter(Boolean)){
    const match=entry.match(/^\d+\s+([a-f0-9]{40,64})\s+\d+\t(.+)$/s)
    if(!match)throw Error('unexpected git ls-files staging entry')
    const path=match[2]
    const full=resolve(root,path)
    const size=statSync(full).size
    const content=/\.(?:rs|mjs)$/.test(path)||['README.md','ENGINE.md'].includes(path)
      ?readFileSync(full,'utf8'):undefined
    files.push({path,size,sha:match[1],content})
  }
  return auditFiles(files)
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const result=auditRepository()
  if(result.errors.length){
    console.error('DOMINION HYGIENE GATE FAILED:\n'+result.errors.map(x=>' - '+x).join('\n'))
    process.exitCode=1
  }else{
    console.log('DOMINION HYGIENE OK: '+result.files+' tracked files, '+
      (result.bytes/1024/1024).toFixed(2)+' MiB, '+result.docs+
      ' docs; no source, duplicate, stale-reference, or generated-file residue')
  }
}
