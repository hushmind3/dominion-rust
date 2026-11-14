//! Parallel research tasks with distinct child sessions and bounded fan-out.
//! Teammates are read-only; the parent integrates and verifies any edits.
use serde_json::Value;
use std::collections::HashSet;
pub fn validate(tasks:&Value)->Result<Vec<(String,String)>,String>{
    let Some(items)=tasks.as_array().filter(|v|(2..=4).contains(&v.len())) else{
        return Err("Parallel delegation requires 2 to 4 independent tasks".into());
    };
    let mut seen=HashSet::new();
    let mut plans=Vec::new();
    for item in items{
        let Some(agent)=item["agentId"].as_str().map(str::trim)
            .filter(|s|!s.is_empty()&&s.len()<200) else {
            return Err("Every parallel task needs a valid agentId".into());
        };
        let Some(prompt)=item["prompt"].as_str().map(str::trim)
            .filter(|s|!s.is_empty()&&s.chars().count()<=4000) else {
            return Err("Parallel task prompt must contain 1..4000 chars".into());
        };
        let marker=format!("{agent}:{prompt}");
        if !seen.insert(marker){return Err("Duplicate parallel task".into());}
        plans.push((agent.to_string(),prompt.to_string()));
    }
    Ok(plans)
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn bounded_unique_subtasks(){
        assert!(validate(&json!([{"agentId":"a","prompt":"inspect API"},
            {"agentId":"a","prompt":"inspect UI"}])).is_ok());
        assert!(validate(&json!([{"agentId":"a","prompt":"x"}])).is_err());
        assert!(validate(&json!([{"agentId":"a","prompt":"x"},
            {"agentId":"a","prompt":"x"}])).is_err());
    }
}
