//! Shared effort contract. Model catalogs, not UI tiers, decide availability.
use serde_json::{json, Value};

pub fn valid_effort(value: &str) -> bool {
    value.is_empty() || (value.len() <= 32
        && value.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'))
}

pub fn validate(value: &Value) -> Result<(), String> {
    match value.get("reasoningEffort") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(effort)) if valid_effort(effort) => Ok(()),
        _ => Err("추론 강도 값이 올바르지 않습니다.".into()),
    }
}

/// Never drop `none` or silently downgrade a requested tier. A missing value
/// leaves the provider's advertised default in charge.
pub fn codex_options(catalog: Option<&Value>, model: &str, effort: Option<&str>) -> Result<Option<Value>, String> {
    let Some(effort) = effort.filter(|value| !value.is_empty()) else { return Ok(None) };
    if !valid_effort(effort) { return Err("추론 강도 값이 올바르지 않습니다.".into()); }
    let row = catalog.and_then(|catalog| catalog["models"].as_array())
        .and_then(|rows| rows.iter().find(|row| row["id"] == model));
    if let Some(supported) = row.and_then(|row| row["reasoningEfforts"].as_array()).filter(|items| !items.is_empty()) {
        if !supported.iter().any(|value| value.as_str() == Some(effort)) {
            let levels = supported.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
            return Err(format!("{model}은 추론 강도 '{effort}'를 지원하지 않습니다. 지원 값: {levels}"));
        }
    }
    Ok(Some(json!({"effort":effort})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advertised_efforts_are_preserved_and_unsupported_choices_are_rejected() {
        let catalog = json!({"models":[{"id":"dynamic-model","reasoningEfforts":["none","low","xhigh","max","ultra","future_tier"]}]});
        for effort in ["none", "low", "xhigh", "max", "ultra", "future_tier"] {
            assert_eq!(codex_options(Some(&catalog), "dynamic-model", Some(effort)).unwrap(), Some(json!({"effort":effort})));
        }
        assert!(codex_options(Some(&catalog), "dynamic-model", Some("high")).is_err());
        assert_eq!(codex_options(Some(&catalog), "dynamic-model", None).unwrap(), None);
        assert!(validate(&json!({"reasoningEffort":false})).is_err());
        assert!(!valid_effort("ultra\n"));
    }
}
