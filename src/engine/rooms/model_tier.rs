//! Opt-in, owner-tagged room model tiering.
/// A model label is supplied by the owner, never guessed from its name, price,
/// or provider. Keep routes stable for every speaker within one owner message.
use serde_json::Value;

pub(super) type RoomRoute = (String, String, Option<String>);

pub(super) fn select(room: &Value, pool: Vec<RoomRoute>, owner_text: &str) -> Vec<RoomRoute> {
    if room["tieredRouting"] != true || pool.is_empty() {
        return pool;
    }
    let Some(configured) = room["models"].as_array().filter(|rows| !rows.is_empty()) else {
        return pool;
    };
    let tier = if super::is_deep_request(owner_text) { "deep" } else { "fast" };
    let matched = pool.iter().filter(|(provider, model, _)| {
        configured.iter().any(|entry| {
            entry["providerId"].as_str() == Some(provider.as_str())
                && entry["model"].as_str() == Some(model.as_str())
                && entry["tier"].as_str() == Some(tier)
        })
    }).cloned().collect::<Vec<_>>();
    // Never fabricate a provider route, reject a legitimate owner selection,
    // or send an empty candidate set when a tier was left unconfigured.
    if matched.is_empty() { pool } else { matched }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn opt_in_model_tiering_respects_owner_labels_and_fallback() {
        let pool = vec![
            ("p".into(),"small".into(),None),
            ("p".into(),"large".into(),Some("high".into()))
        ];
        let room = json!({"tieredRouting":true,"models":[
            {"providerId":"p","model":"small","tier":"fast"},
            {"providerId":"p","model":"large","tier":"deep"}
        ]});
        assert_eq!(select(&room,pool.clone(),"안녕")[0].1,"small");
        assert_eq!(select(&room,pool.clone(),"구현 구조 분석")[0].1,"large");
        assert_eq!(select(&json!({"tieredRouting":false}),pool.clone(),"구현")[0].1,"small");
        assert_eq!(select(&json!({"tieredRouting":true,"models":[]}),pool.clone(),"구현"),pool);
        let untagged=json!({"tieredRouting":true,"models":[
            {"providerId":"p","model":"small"},
            {"providerId":"p","model":"large"}
        ]});
        assert_eq!(select(&untagged,pool.clone(),"구현"),pool);
    }
}
