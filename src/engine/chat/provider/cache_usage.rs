//! Provider-reported cache accounting. Never infer a cache hit from a hint.
 //! OpenAI Responses supports optional cache-write TTL details. Anthropic
 //! reports cache creation and read separately. Both remain input tokens.
use super::Completion;
use crate::state::AppState;
use serde_json::{json,Value};

pub(super) fn observe(completion:&mut Completion,usage:&Value){
    completion.cached_prompt_tokens=super::cached_prompt_tokens(usage)
        .or_else(||usage["cache_read_input_tokens"].as_u64());
    completion.cache_write_tokens=usage.pointer("/input_tokens_details/cache_write_tokens")
        .and_then(Value::as_u64)
        .or_else(||usage.pointer("/prompt_tokens_details/cache_write_tokens").and_then(Value::as_u64))
        .or_else(||usage["cache_creation_input_tokens"].as_u64());
    completion.cache_write_30m_tokens=usage.pointer("/input_tokens_details/cache_write_tokens_details/ttl_30m_tokens")
        .and_then(Value::as_u64);
    completion.cache_write_12h_tokens=usage.pointer("/input_tokens_details/cache_write_tokens_details/ttl_12h_tokens")
        .and_then(Value::as_u64);
}
pub(super) fn audit(state:&AppState,provider_id:&str,model:&str,session_id:&str,
    c:&Completion){
    if !c.usage_exact || (c.cached_prompt_tokens.is_none() && c.cache_write_tokens.is_none()){
        return;
    }
    let read=c.cached_prompt_tokens.map(|v|v.min(c.prompt_tokens));
    let write=c.cache_write_tokens.map(|v|v.min(c.prompt_tokens.saturating_sub(read.unwrap_or(0))));
    let uncached=match (read,write){
        (Some(r),Some(w))=>Some(c.prompt_tokens.saturating_sub(r).saturating_sub(w)),
        _=>None
    };
    state.store.audit("model.cache_usage",json!({
        "providerId":provider_id,"model":model,"sessionId":session_id,
        "promptTokens":c.prompt_tokens,"cacheReadTokens":read,
        "cacheWriteTokens":write,"ordinaryInputTokens":uncached,
        "cacheWrite30mTokens":c.cache_write_30m_tokens,
        "cacheWrite12hTokens":c.cache_write_12h_tokens,
        "usageExact":true,"costMeasured":false,
        "notice":"Cache read/write are subsets of promptTokens, never additive"
    }));
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn openai_cache_details_distinguish_write_and_read(){
        let mut c=Completion::default();
        observe(&mut c,&json!({"input_tokens_details":{"cached_tokens":400,
            "cache_write_tokens":300,"cache_write_tokens_details":{
                "ttl_30m_tokens":200,"ttl_12h_tokens":100}}}));
        assert_eq!(c.cached_prompt_tokens,Some(400));
        assert_eq!(c.cache_write_tokens,Some(300));
        assert_eq!(c.cache_write_12h_tokens,Some(100));
    }
    #[test]
    fn anthropic_cache_creation_is_not_mistaken_for_cache_hit(){
        let mut c=Completion::default();
        observe(&mut c,&json!({"cache_read_input_tokens":0,
            "cache_creation_input_tokens":950}));
        assert_eq!(c.cached_prompt_tokens,Some(0));
        assert_eq!(c.cache_write_tokens,Some(950));
    }
    #[test]
    fn absent_cache_data_remains_unknown_not_zero(){
        let mut c=Completion::default();
        observe(&mut c,&json!({"input_tokens":100}));
        assert_eq!(c.cached_prompt_tokens,None);
        assert_eq!(c.cache_write_tokens,None);
    }
}
