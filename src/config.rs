use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Clone)]
pub struct ModelSettings {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub input_krw_per_million: f64,
    pub cached_input_krw_per_million: f64,
    pub output_krw_per_million: f64,
    pub cache_write_krw_per_million: f64,
    pub cache_ttl: Option<String>,
    pub cache_key: Option<String>,
    pub parallel_tool_calls: bool,
}

#[derive(Clone)]
pub struct Settings {
    pub bind: SocketAddr,
    pub token: Option<String>,
    pub data_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub main: ModelSettings,
    pub economy: ModelSettings,
    pub max_steps: usize,
    pub shell_enabled: bool,
    /// Maximum serialized characters sent to a model in one agent turn.
    pub context_max_chars: usize,
}

fn var(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
fn nonempty_var(name: &str, default: &str) -> String {
    env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(||default.to_string())
}
fn price(name: &str, default: f64) -> f64 {
    env::var(name).ok().and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite() && *v >= 0.0).unwrap_or(default)
}
impl Settings {
    pub fn from_env() -> Result<Self, String> {
        let bind: SocketAddr = var("DOMINION_BIND", "127.0.0.1:8787").parse().map_err(|e| format!("invalid DOMINION_BIND: {e}"))?;
        let token = env::var("DOMINION_ACCESS_TOKEN").ok().filter(|s| !s.is_empty());
        if !bind.ip().is_loopback() && token.is_none() {
            return Err("Remote bind requires DOMINION_ACCESS_TOKEN".into());
        }
        let cache_ttl=env::var("DOMINION_PROMPT_CACHE_TTL").ok().filter(|v|!v.is_empty());
        if cache_ttl.as_deref().is_some_and(|v|!matches!(v,"30m"|"12h")){
            return Err("DOMINION_PROMPT_CACHE_TTL must be 30m or 12h".into());
        }
        let cache_key=env::var("DOMINION_PROMPT_CACHE_KEY").ok().filter(|v|!v.is_empty()&&v.len()<=128);
        let cache_multiplier=if cache_ttl.as_deref()==Some("12h"){2.5}else{1.25};
        let main_input=price("DOMINION_INPUT_KRW_PER_MTOK",0.0);
        let parallel_tool_calls=var("DOMINION_PARALLEL_TOOL_CALLS","true")!="false";
        let main = ModelSettings {
            name: nonempty_var("DOMINION_MODEL", "gpt-6.1-sol"),
            base_url: nonempty_var("DOMINION_BASE_URL", "https://api.openai.com/v1").trim_end_matches('/').to_string(),
            api_key: var("DOMINION_API_KEY", ""),
            input_krw_per_million: main_input,
            cached_input_krw_per_million: price("DOMINION_CACHED_INPUT_KRW_PER_MTOK",price("DOMINION_INPUT_KRW_PER_MTOK",0.0)),
            output_krw_per_million: price("DOMINION_OUTPUT_KRW_PER_MTOK", 0.0),
            cache_write_krw_per_million:price("DOMINION_CACHE_WRITE_KRW_PER_MTOK",main_input*cache_multiplier),
            cache_ttl:cache_ttl.clone(),cache_key:cache_key.clone(),parallel_tool_calls,
        };
        let economy = ModelSettings {
            name: nonempty_var("DOMINION_ECONOMY_MODEL", &main.name),
            base_url: nonempty_var("DOMINION_ECONOMY_BASE_URL", &main.base_url).trim_end_matches('/').to_string(),
            api_key: nonempty_var("DOMINION_ECONOMY_API_KEY", &main.api_key),
            input_krw_per_million: price("DOMINION_ECONOMY_INPUT_KRW_PER_MTOK", main.input_krw_per_million),
            cached_input_krw_per_million: price("DOMINION_ECONOMY_CACHED_INPUT_KRW_PER_MTOK",main.cached_input_krw_per_million),
            output_krw_per_million: price("DOMINION_ECONOMY_OUTPUT_KRW_PER_MTOK", main.output_krw_per_million),
            cache_write_krw_per_million:price("DOMINION_ECONOMY_CACHE_WRITE_KRW_PER_MTOK",main.cache_write_krw_per_million),
            cache_ttl,cache_key,parallel_tool_calls,
        };
        let max_steps = var("DOMINION_MAX_STEPS", "30").parse::<usize>().unwrap_or(30).clamp(1, 100);
        Ok(Self {
            bind, token,
            data_dir: PathBuf::from(var("DOMINION_DATA_DIR", "./runtime")),
            workspace_dir: PathBuf::from(var("DOMINION_WORKSPACE_DIR", "./workspace")),
            main, economy, max_steps,
            shell_enabled: var("DOMINION_ENABLE_SHELL", "false") == "true",
            context_max_chars: var("DOMINION_CONTEXT_MAX_CHARS", "80000").parse::<usize>().unwrap_or(80000).clamp(8000,500000),
        })
    }
}
