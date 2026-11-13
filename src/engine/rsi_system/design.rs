//! Role taxonomy and source-backed design completeness checks.
use serde_json::Value;

pub(super) fn valid_role(name:&str)->bool {
    !name.is_empty() && name.len()<=64 && name.bytes().all(|c|
        c.is_ascii_alphanumeric() || c==b'_' || c==b'-')
}

pub(super) fn role_domain(role:&Value)->&str {
    if let Some(domain)=role["domain"].as_str().filter(|d|
        crate::engine::rsi_evolve::DOMAINS.contains(d)) {return domain;}
    match role["role"].as_str().unwrap_or("") {
        "memory"=>"memory", "learning"=>"learning", "models"=>"learning", "tools"=>"tools",
        "harness"=>"context", "frontend"=>"ui", _=>"improver"
    }
}

/// Verify a design's complete implementation, not one coincidentally edited
/// file. New/renamed intelligence roles use the same contract as existing ones.
pub(crate) fn design_implemented(design:&Value, paths:&[Value])->bool {
    let Some(targets)=design["targetFiles"].as_array().filter(|v|!v.is_empty()) else{return false;};
    targets.iter().all(|target|target.as_str().is_some_and(|path|
        crate::engine::rsi::safe_path(path) && paths.iter().any(|changed|changed.as_str()==Some(path))))
}

pub(super) fn design_roles(design:&Value)->Vec<String> {
    design["components"].as_array().map(|roles|roles.iter().filter_map(Value::as_str)
        .filter(|role|valid_role(role)).map(str::to_string).collect())
        .unwrap_or_else(||design["component"].as_str().filter(|role|valid_role(role))
            .map(|role|vec![role.to_string()]).unwrap_or_default())
}
