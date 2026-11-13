//! Provider-reported, not theoretically assumed, room token accounting.
//! Unknown cache usage stays null and must never be counted as a cache miss.
use super::*;

pub(super) fn summary(outputs:&[Value])->Value {
    let (mut total,mut prompt,mut generated,mut cached)=(0u64,0u64,0u64,0u64);
    let (mut measured,mut estimated,mut unknown_cache)=(0usize,0usize,0usize);
    for output in outputs{
        let usage=&output["usage"];
        if usage.is_null(){continue;}
        total=total.saturating_add(usage["totalTokens"].as_u64().unwrap_or(0));
        prompt=prompt.saturating_add(usage["promptTokens"].as_u64().unwrap_or(0));
        generated=generated.saturating_add(usage["completionTokens"].as_u64().unwrap_or(0));
        if usage["estimated"]==true{
            estimated+=1;
            unknown_cache+=1;
        }else if let Some(count)=usage["cachedPromptTokens"].as_u64(){
            if count<=usage["promptTokens"].as_u64().unwrap_or(0){
                measured+=1;
                cached=cached.saturating_add(count);
            }else{unknown_cache+=1;}
        }else{unknown_cache+=1;}
    }
    let complete_cache=measured>0 && unknown_cache==0;
    json!({
        "actualProviderUsageRecorded":measured>0,
        "promptTokens":prompt,"completionTokens":generated,"totalTokens":total,
        "cachedPromptTokens":if complete_cache{json!(cached)}else{Value::Null},
        "uncachedPromptTokens":if complete_cache{json!(prompt.saturating_sub(cached))}else{Value::Null},
        "cacheReadRatio":if complete_cache && prompt>0{
            json!(cached as f64 / prompt as f64)
        }else{Value::Null},
        "measuredCalls":measured,"estimatedCalls":estimated,
        "unknownCacheCalls":unknown_cache,
        "isMonetarySavingMeasured":false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_hit_ratio_requires_all_turns_to_report() {
        let actual=json!({"usage":{"promptTokens":100,"cachedPromptTokens":60,
            "completionTokens":10,"totalTokens":110,"estimated":false}});
        let missing=json!({"usage":{"promptTokens":100,"cachedPromptTokens":null,
            "completionTokens":10,"totalTokens":110,"estimated":false}});
        let accurate=summary(&[actual.clone()]);
        assert_eq!(accurate["cachedPromptTokens"],60);
        assert_eq!(accurate["cacheReadRatio"],0.6);
        let mixed=summary(&[actual,missing]);
        assert!(mixed["cachedPromptTokens"].is_null());
        assert_eq!(mixed["unknownCacheCalls"],1);
        assert_eq!(mixed["totalTokens"],220);
    }
}
