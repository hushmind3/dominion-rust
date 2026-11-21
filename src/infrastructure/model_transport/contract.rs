use super::*;
pub(super) fn anthropic_native_cache(base_url:&str)->bool{
    reqwest::Url::parse(base_url).ok().is_some_and(|url|
        url.scheme()=="https"&&url.host_str()==Some("api.anthropic.com")
            &&url.port_or_known_default()==Some(443))
}
pub(super) fn anthropic_messages(original:&[Value],native_cache:bool)->Result<(String,Vec<Value>),String>{
    let mut system=Vec::<String>::new();
    let mut messages=Vec::<Value>::new();
    for record in original{
        let role=record.get("role").and_then(Value::as_str)
            .ok_or("model history entry missing role")?;
        let content=match record.get("content"){
            Some(Value::String(s))=>s.as_str(),
            Some(Value::Null)|None=>"",
            _=>return Err("Anthropic model adapter requires plain-text history content".into()),
        };
        match role{
            "system"|"developer"=>{if !content.is_empty(){system.push(content.to_owned());}},
            "user"=>{
                let mut block=json!({"type":"text","text":content});
                if native_cache && record.get("dominion_cache_anchor")==Some(&Value::Bool(true)){
                    block["cache_control"]=json!({"type":"ephemeral"});
                }
                messages.push(json!({"role":"user","content":[block]}));
            },
            "assistant"=>{
                let mut blocks=Vec::<Value>::new();
                if !content.is_empty(){blocks.push(json!({"type":"text","text":content}));}
                if let Some(tool_calls)=record.get("tool_calls").and_then(Value::as_array){
                    for call in tool_calls{
                        let id=call.get("id").and_then(Value::as_str)
                            .ok_or("previous native tool call lacks ID")?;
                        let name=call.pointer("/function/name").and_then(Value::as_str)
                            .ok_or("previous native tool call lacks name")?;
                        let raw=call.pointer("/function/arguments").and_then(Value::as_str)
                            .ok_or("previous native tool call lacks arguments")?;
                        let input:Value=serde_json::from_str(raw)
                            .map_err(|e|format!("previous tool input is invalid: {e}"))?;
                        blocks.push(json!({"type":"tool_use","id":id,"name":name,"input":input}));
                    }
                }
                if blocks.is_empty(){return Err("assistant message contains no content".into());}
                messages.push(json!({"role":"assistant","content":blocks}));
            },
            "tool"=>{
                let call_id=record.get("tool_call_id").and_then(Value::as_str)
                    .ok_or("tool receipt missing associated call ID")?;
                let result=json!({"type":"tool_result","tool_use_id":call_id,"content":content});
                if let Some(last)=messages.last_mut(){
                    if last.get("role").and_then(Value::as_str)==Some("user"){
                        if let Some(blocks)=last.get_mut("content").and_then(Value::as_array_mut){
                            if blocks.iter().all(|v|v["type"]=="tool_result"){
                                blocks.push(result);continue;
                            }
                        }
                    }
                }
                messages.push(json!({"role":"user","content":[result]}));
            },
            other=>return Err(format!("unsupported native message role for Anthropic: {other}")),
        }
    }
    if messages.is_empty(){return Err("Anthropic request has no conversation messages".into());}
    Ok((system.join("\n\n"),messages))
}
/// Provider finish status is part of the execution contract. A model that
/// exhausted its output budget or returned filtered/paused content has not
/// delivered a complete plan, tool request or final answer. Never feed it to
/// the harness verifier as a successful completion. This error is not
/// retryable: model usage may already have been charged upstream.
pub(super) fn validate_completion(protocol:&str,reason:Option<&str>,
    has_text:bool,has_calls:bool)->Result<(),String>{
 let valid=match (protocol,reason){
  ("openai",Some("stop"))|("anthropic",Some("end_turn"|"stop_sequence"))=>
   has_text&&!has_calls, // A final answer must never smuggle a live tool call.
  ("openai",Some("tool_calls"))|("anthropic",Some("tool_use"))=>
   has_calls,
  ("openai"|"anthropic",None)=>has_text||has_calls, // local mock/compat
  _=>false
 };
 if valid{return Ok(());}
 let label=reason.unwrap_or("unspecified");
 Err(format!(
  "model provider response incomplete or unusable ({protocol} stop_reason={label}); \
the reply was not executed or verified. Upstream model tokens may already be billed; inspect usage before retrying"))
}

const MAX_PROVIDER_RESPONSE_BYTES:usize=8*1024*1024;

/// Bounded even if Content-Length is absent or falsified.
pub(super) async fn bounded_body(mut response:reqwest::Response)->Result<Vec<u8>,String>{
 if response.content_length().is_some_and(|n|n>MAX_PROVIDER_RESPONSE_BYTES as u64){
  return Err("provider response exceeds 8 MiB cap".into());
 }
 let mut body=Vec::new();
 while let Some(chunk)=response.chunk().await.map_err(|e|format!("provider body: {e}"))?{
  if body.len().saturating_add(chunk.len())>MAX_PROVIDER_RESPONSE_BYTES{
   return Err("provider response exceeds 8 MiB cap".into());
  }
  body.extend_from_slice(&chunk);
 }
 Ok(body)
}
/// 429 is a definite rejection; all other failures have uncertain billing.
pub(super) fn retryable(error:&str)->bool{error.starts_with("provider HTTP 429 ")}
pub(super) fn safe_fallback(error:&str)->bool{retryable(error)}
pub(super) fn estimate_cost(settings:&ModelSettings,input:u64,cached:u64,written:u64,output:u64)->f64{
    let cached=cached.min(input);
    let written=written.min(input-cached);
    let noncached=input-cached-written;
    ((noncached as f64*settings.input_krw_per_million)
        +(cached as f64*settings.cached_input_krw_per_million)
        +(written as f64*settings.cache_write_krw_per_million)
        +(output as f64*settings.output_krw_per_million))/1_000_000.0
}
