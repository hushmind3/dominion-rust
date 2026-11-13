//! Explicit lightweight/complex model routes for room turns.
//! The owner supplies the two models; no provider names or costs are guessed.
use super::*;

pub(super) fn select(room:&Value,owner_text:&str)->Option<(String,String,Option<String>,&'static str)>{
    if room["tierRoutingEnabled"]!=true{return None;}
    let tier=if is_deep_request(owner_text){"deep"}else{"light"};
    let route=&room["tierRoutes"][tier];
    let provider=route["providerId"].as_str().filter(|s|!s.is_empty())?;
    let model=route["model"].as_str().filter(|s|!s.is_empty())?;
    let effort=route["reasoningEffort"].as_str().filter(|s|!s.is_empty()).map(str::to_string);
    Some((provider.to_string(),model.to_string(),effort,tier))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn explicit_tiering_only_and_safely_selects_two_owner_routes(){
        let mut room=json!({"tierRoutingEnabled":true,"tierRoutes":{
            "light":{"providerId":"cheap","model":"small","reasoningEffort":"none"},
            "deep":{"providerId":"strong","model":"large","reasoningEffort":"high"}
        }});
        assert_eq!(select(&room,"안녕").unwrap().3,"light");
        assert_eq!(select(&room,"엔진을 분석하고 설계해줘").unwrap().1,"large");
        room["tierRoutingEnabled"]=json!(false);
        assert!(select(&room,"엔진 분석").is_none());
    }
}
