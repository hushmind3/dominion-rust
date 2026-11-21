/** Resolve the actual native chat provider/model contract before rendering the picker.
 * Durable sessions stay pinned to their creation route even when an agent's
 * default model is edited later. An explicit owner override is the only
 * reason to choose another endpoint. */
type Session={agentId:string;providerId?:string;model?:string}
type Agent={providerId?:string;model?:string}
type Provider={id:string;model?:string}
type Override={providerId:string;model:string}

export function chatRoute(
 session:Session|undefined,agentId:string,agent:Agent|undefined,
 override:Override|undefined,providers:readonly Provider[]
):{providerId:string;model:string}{
 if(override){
  const configured=providers.find(item=>item.id===override.providerId)
  return {providerId:override.providerId,model:override.model||configured?.model||''}
 }
 const providerId=session?.providerId??agent?.providerId??providers[0]?.id??''
 const configured=providers.find(item=>item.id===providerId)
 const stored=session?.agentId===agentId&&session.providerId===providerId
  ?session.model:undefined
 const model=stored||(agent?.providerId===providerId?agent.model:undefined)||configured?.model||''
 return {providerId,model}
}
