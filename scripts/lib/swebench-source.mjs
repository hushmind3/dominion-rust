// Exact base tree in a new, isolated Git object database. No future refs.
// The mirror-linked staging checkout is destroyed before the agent starts.
import {randomBytes} from 'node:crypto'
import {execFile as callback} from 'node:child_process'
import {cp,lstat,readFile,rm} from 'node:fs/promises'
import {basename,join} from 'node:path'
import {promisify} from 'node:util'
const execFile=promisify(callback)
async function git(...args){
    const env={...process.env}
    for(const key of ['GIT_DIR','GIT_WORK_TREE','GIT_OBJECT_DIRECTORY',
        'GIT_ALTERNATE_OBJECT_DIRECTORIES','GIT_INDEX_FILE'])delete env[key]
    return (await execFile('git',args,{timeout:180000,maxBuffer:8*1024*1024,env})).stdout.trim()
}
export async function prepareSealedSource(mirror,base,source){
    if(!/^[a-fA-F0-9]{40}$/.test(base))throw Error('invalid source revision')
    try{
        await lstat(source)
        throw Error('sealed source destination already exists')
    }catch(error){if(error.code!=='ENOENT')throw error}
    const stage=source+'.seed-'+randomBytes(8).toString('hex')
    let staged=false
    try{
        // A trusted, temporary checkout. Hooks and CRLF transformation disabled.
        await git('-C',mirror,'-c','core.hooksPath=/dev/null',
            '-c','core.autocrlf=false','worktree','add','--detach',stage,base)
        staged=true
        // Copy the actual checked-out tree, not archive (which may honor
        // export-ignore/export-subst and omit or rewrite benchmark files).
        await cp(stage,source,{recursive:true,force:false,errorOnExist:true,
            verbatimSymlinks:true,preserveTimestamps:true,
            filter:path=>basename(path)!=='.git'})
        const foreignGit=await lstat(join(source,'.git')).then(()=>true)
            .catch(e=>{if(e.code==='ENOENT')return false;throw e})
        if(foreignGit)throw Error('unexpected Git metadata in source snapshot')
        await git('-C',source,'init','--quiet','--initial-branch=benchmark-base')
        await git('-C',source,'-c','core.hooksPath=/dev/null','add','--force','-A','--','.')
        await git('-C',source,'-c','core.hooksPath=/dev/null',
            '-c','commit.gpgsign=false','-c','user.name=Dominion Benchmark',
            '-c','user.email=benchmark@localhost',
            'commit','--quiet','--allow-empty','-m','Sealed historical task base')
        const [sealedCommit,originalTree,sealedTree,parents,remotes,commits]=await Promise.all([
            git('-C',source,'rev-parse','HEAD'),
            git('-C',mirror,'rev-parse',base+'^{tree}'),
            git('-C',source,'rev-parse','HEAD^{tree}'),
            git('-C',source,'rev-list','--parents','-n','1','HEAD'),
            git('-C',source,'remote'),
            git('-C',source,'rev-list','--all','--count')
        ])
        const alternates=await readFile(join(source,'.git','objects','info','alternates'),'utf8')
            .catch(e=>{if(e.code==='ENOENT')return '';throw e})
        if(originalTree!==sealedTree)throw Error('sealed source tree differs from historical base')
        if(parents.split(/\s+/).length!==1 ||remotes||commits!=='1'||alternates)
            throw Error('sealed source exposes additional history or shared objects')
        return {sealedCommit,originalCommit:base,tree:sealedTree,
            historyCommits:1,remoteCount:0,objectsShared:false}
    }catch(error){
        await rm(source,{recursive:true,force:true}).catch(()=>{})
        throw error
    }finally{
        if(staged){
            await git('-C',mirror,'worktree','remove','--force',stage)
                .catch(async()=>{
                    await rm(stage,{recursive:true,force:true}).catch(()=>{})
                    await git('-C',mirror,'worktree','prune').catch(()=>{})
                })
        }else await rm(stage,{recursive:true,force:true}).catch(()=>{})
    }
}
