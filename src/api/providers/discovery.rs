//! Remote public provider and free model catalog.
use super::{
    AppState,
    State,
    Json,
    json,
    Value,
    Duration,
    error,
};

/// Public provider catalog backed by models.dev. The cache keeps the Models
/// tab usable offline and avoids hammering the upstream on every visit.
pub async fn provider_catalog(State(state): State<AppState>) -> Json<Value> {
    let cached = state.store.get("provider_catalog", "models.dev");
    if let Some(cached) = &cached {
        let fresh = cached["fetchedAt"]
            .as_str()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
            .is_some_and(|time| {
                (chrono::Utc::now() - time.with_timezone(&chrono::Utc)).num_seconds() < 86_400
            });
        if fresh {
            return Json(json!({
                "providers":cached["providers"],
                "cached":true
            }));
        }
    }
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(20)).build() {
        Ok(client) => client,
        Err(error) => return Json(json!({"providers":[],"error":error.to_string()})),
    };
    let fetched = async {
        let response = client
            .get("https://models.dev/api.json")
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        response
            .json::<Value>()
            .await
            .map_err(|_| "카탈로그 JSON 형식을 읽을 수 없습니다.".to_string())
    }
    .await;
    let data = match fetched {
        Ok(data) => data,
        Err(error) => {
            if let Some(cached) = cached {
                return Json(json!({
                    "providers":cached["providers"],
                    "cached":true,
                    "error":format!("최신 카탈로그를 가져오지 못했습니다: {error}")
                }));
            }
            return Json(json!({"providers":[],"error":format!("공개 카탈로그를 가져오지 못했습니다: {error}")}));
        }
    };
    let mut providers = Vec::new();
    for (id, item) in data.as_object().into_iter().flatten() {
        let Some(api) = item["api"].as_str().filter(|value| !value.is_empty()) else {
            continue;
        };
        let models = item["models"].as_object().map(|models| models.len()).unwrap_or(0);
        if models == 0 {
            continue;
        }
        providers.push(json!({
            "id":id,
            "name":item["name"].as_str().unwrap_or(id),
            "api":api,
            "models":models,
            "env":item["env"],
            "doc":item["doc"],
        }));
    }
    providers.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    providers.truncate(300);
    let record = json!({
        "id":"models.dev",
        "fetchedAt":chrono::Utc::now().to_rfc3339(),
        "providers":providers
    });
    state.store.put("provider_catalog", &record);
    Json(json!({"providers":record["providers"],"cached":false}))
}

/// Refreshes every enabled provider's model catalogue and stores the
/// free/paid/unknown classification with a timestamp. Runs on demand and from
/// the background scheduler; the Models page reads the cached result.
pub async fn refresh_free_catalog(state: &AppState) -> Value {
    let lock = "free-catalog-refresh";
    if !state.begin_session(lock) {
        return state
            .store
            .get("free_catalog", "catalog")
            .unwrap_or_else(|| json!({"id":"catalog","status":"running","providers":[]}));
    }
    let _guard = state.session_guard(lock);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(error) => return json!({"id":"catalog","status":"error","error":error.to_string()}),
    };
    let port = state.config.port;
    let mut providers = Vec::new();
    let (mut free, mut paid, mut unknown) = (0u64, 0u64, 0u64);
    for provider in state.store.all("provider") {
        if provider["enabled"].as_bool() == Some(false) {
            continue;
        }
        let Some(id) = provider["id"].as_str() else {
            continue;
        };
        let name = provider["name"].as_str().unwrap_or(id);
        let url = format!("http://127.0.0.1:{port}/api/providers/{id}/models");
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let models = response.json::<Vec<Value>>().await.unwrap_or_default();
                let mut rows = Vec::new();
                for model in models {
                    match model["freeStatus"].as_str() {
                        Some("free") => free += 1,
                        Some("paid") => paid += 1,
                        _ => unknown += 1,
                    }
                    rows.push(json!({
                        "id":model["id"],
                        "free":model["free"],
                        "freeStatus":model["freeStatus"],
                        "freeSource":model["freeSource"],
                    }));
                }
                providers.push(json!({
                    "providerId":id,"providerName":name,"reachable":true,
                    "models":rows
                }));
            }
            Ok(response) => {
                providers.push(json!({
                    "providerId":id,"providerName":name,"reachable":false,
                    "error":format!("HTTP {}",response.status()),"models":[]
                }));
            }
            Err(error) => {
                providers.push(json!({
                    "providerId":id,"providerName":name,"reachable":false,
                    "error":error.to_string(),"models":[]
                }));
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let record = json!({
        "id":"catalog",
        "status":"ready",
        "updatedAt":chrono::Utc::now().to_rfc3339(),
        "providers":providers,
        "counts":{"free":free,"paid":paid,"unknown":unknown},
    });
    state.store.put("free_catalog", &record);
    record
}

pub async fn free_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(
        state
            .store
            .get("free_catalog", "catalog")
            .unwrap_or_else(|| {
                json!({
                    "id":"catalog","status":"empty","providers":[],
                    "counts":{"free":0,"paid":0,"unknown":0}
                })
            }),
    )
}

pub async fn free_catalog_refresh(State(state): State<AppState>) -> Json<Value> {
    Json(refresh_free_catalog(&state).await)
}

