//! Hugging Face catalog and model detail queries.
use super::*;

/// Hugging Face search, mapped to the shape the model explorer expects.

#[allow(dead_code)]
pub async fn catalog(query: &str, limit: usize, format: &str) -> Result<Vec<Value>, String> {
    catalog_with_filters(query, limit, format, "downloads", None, false).await
}

pub async fn catalog_with_filters(
    query: &str, limit: usize, format: &str, sort: &str,
    max_params: Option<usize>, chat_only: bool,
) -> Result<Vec<Value>, String> {
    let sort = match sort {
        "likes" => "likes",
        "lastModified" => "lastModified",
        _ => "downloads",
    };
    if format == "all" {
        let limit = limit.clamp(1, 100);
        let (gguf, mlx) = tokio::join!(
            catalog_filtered(query, limit, "gguf", sort, max_params, chat_only),
            catalog_filtered(query, limit, "mlx", sort, max_params, chat_only)
        );
        let mut items = gguf.unwrap_or_default();
        items.extend(mlx.unwrap_or_default());
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| {
            item["id"].as_str().is_some_and(|id| seen.insert(id.to_string()))
        });
        items.sort_by(|a, b| {
            let order = match sort {
                "likes" => b["likes"].as_u64().unwrap_or(0)
                    .cmp(&a["likes"].as_u64().unwrap_or(0)),
                "lastModified" => b["updatedAt"].as_str().unwrap_or("")
                    .cmp(a["updatedAt"].as_str().unwrap_or("")),
                _ => b["downloads"].as_u64().unwrap_or(0)
                    .cmp(&a["downloads"].as_u64().unwrap_or(0)),
            };
            order.then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
        });
        items.truncate(limit);
        return Ok(items);
    }
    catalog_filtered(query, limit, format, sort, max_params, chat_only).await
}

pub(super) fn hf_catalog_url(
    query: &str, limit: usize, format: &str, sort: &str,
    max_params: Option<usize>, chat_only: bool,
) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse("https://huggingface.co/api/models")
        .map_err(|error| error.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        if !query.trim().is_empty() {
            pairs.append_pair("search", query.trim());
        }
        pairs.append_pair("filter", if format == "mlx" { "mlx" } else { "gguf" });
        pairs.append_pair("sort", sort);
        pairs.append_pair("direction", "-1");
        pairs.append_pair("limit", &limit.clamp(1, 100).to_string());
        if let Some(max) = max_params.filter(|n| [1, 2, 3, 4, 8].contains(n)) {
            pairs.append_pair("num_parameters", &format!("max:{max}B"));
        }
        if chat_only {
            pairs.append_pair("pipeline_tag", "text-generation");
        }
    }
    Ok(url)
}

pub(super) async fn catalog_filtered(
    query: &str, limit: usize, format: &str, sort: &str,
    max_params: Option<usize>, chat_only: bool,
) -> Result<Vec<Value>, String> {
    let url = hf_catalog_url(query, limit, format, sort, max_params, chat_only)?;
    let response = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Hugging Face HTTP {}", response.status()));
    }
    let items: Vec<Value> = response.json().await.map_err(|error| error.to_string())?;
    Ok(items
        .into_iter()
        .map(|item| {
            let id = item.get("id").and_then(|value| value.as_str()).unwrap_or("");
            json!({
                "id": id,
                "author": id.split('/').next().unwrap_or(""),
                "name": id.split('/').nth(1).unwrap_or(id),
                "library": item.get("library_name").and_then(|value| value.as_str()).unwrap_or("gguf"),
                "downloads": item.get("downloads").and_then(|value| value.as_u64()).unwrap_or(0),
                "likes": item.get("likes").and_then(|value| value.as_u64()).unwrap_or(0),
                "gated": item.get("gated").and_then(|value| value.as_bool()).unwrap_or(false),
                "updatedAt": item.get("lastModified"),
                "description": "",
                "task": item.get("pipeline_tag").and_then(|value| value.as_str()).unwrap_or(""),
                "tags": item.get("tags").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect())
}

pub async fn details(repo: &str) -> Result<Value, String> {
    let repo = safe_repo(repo)?;
    // Card metadata and model configuration are opt-in fields on the Hub API.
    // Without these flags the UI still receives empty license/architecture.
    let url = format!("https://huggingface.co/api/models/{repo}?blobs=true&cardData=true&config=true");
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Hugging Face HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    let siblings = value
        .get("siblings")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut options = gguf_download_options(&siblings);
    options.sort_by(|a, b| {
        a.get("bytes")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .cmp(&b.get("bytes").and_then(|value| value.as_u64()).unwrap_or(0))
    });
    if let Some(mlx) = mlx_download_files(&siblings) {
        options.push(mlx);
    }
    // Model card metadata belongs to the detail view. A missing README must
    // never make a downloadable model look broken.
    let readme = match reqwest::Client::new()
        .get(format!("https://huggingface.co/{repo}/raw/main/README.md"))
        .timeout(Duration::from_secs(5))
        .send().await
    {
        Ok(response) if response.status().is_success()
            && response.content_length().unwrap_or(0) <= 1_048_576 => {
            response.bytes().await.ok()
                .map(|bytes| String::from_utf8_lossy(&bytes[..bytes.len().min(64_000)]).into_owned())
                .unwrap_or_default()
        }
        _ => String::new(),
    };
    let author = repo.split('/').next().unwrap_or("");
    Ok(json!({
        "id": repo,
        "author": author,
        "name": repo.split('/').nth(1).unwrap_or(&repo),
        "library": value.get("library_name").and_then(|v| v.as_str()).unwrap_or("gguf"),
        "downloads": value.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
        "likes": value.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
        "gated": value.get("gated").and_then(|v| v.as_bool()).unwrap_or(false),
        "updatedAt": value.get("lastModified"),
        "description": value["cardData"]["description"].as_str().unwrap_or(""),
        "task": value.get("pipeline_tag").and_then(|v| v.as_str()).unwrap_or(""),
        "tags": value.get("tags").cloned().unwrap_or_else(|| json!([])),
        "revision": value.get("sha").and_then(|v| v.as_str()).unwrap_or("main"),
        "license": value["cardData"]["license"].as_str().unwrap_or(""),
        "architecture": value["config"]["model_type"].as_str()
            .or_else(|| value["config"]["architectures"][0].as_str()).unwrap_or(""),
        "options": options,
        "readme": readme,
    }))
}

