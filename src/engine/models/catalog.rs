//! Hugging Face model search and metadata.
//! Runtime implementations connected by engine::models.
use super::*;

pub async fn catalog(query: &str, limit: usize, format: &str) -> Result<Vec<Value>, String> {
    if format == "all" {
        let limit = limit.clamp(1, 100);
        let (gguf, mlx) = tokio::join!(
            catalog_filtered(query, limit, "gguf"),
            catalog_filtered(query, limit, "mlx")
        );
        let mut items = gguf.unwrap_or_default();
        items.extend(mlx.unwrap_or_default());
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| {
            item["id"]
                .as_str()
                .is_some_and(|id| seen.insert(id.to_string()))
        });
        items.sort_by(|a, b| {
            b["downloads"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["downloads"].as_u64().unwrap_or(0))
        });
        items.truncate(limit);
        return Ok(items);
    }
    catalog_filtered(query, limit, format).await
}

pub(crate) async fn catalog_filtered(query: &str, limit: usize, format: &str) -> Result<Vec<Value>, String> {
    let mut url = reqwest::Url::parse("https://huggingface.co/api/models")
        .map_err(|error| error.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        if !query.trim().is_empty() {
            pairs.append_pair("search", query.trim());
        }
        pairs
            .append_pair("filter", if format == "mlx" { "mlx" } else { "gguf" })
            .append_pair("sort", "downloads")
            .append_pair("direction", "-1")
            .append_pair("limit", &limit.clamp(1, 100).to_string());
    }
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
    if repo.trim().is_empty() {
        return Err("모델 id 가 필요합니다.".into());
    }
    let url = format!("https://huggingface.co/api/models/{repo}?blobs=true");
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
    let mut options = Vec::new();
    let siblings = value
        .get("siblings")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for sibling in &siblings {
        let Some(file) = sibling.get("rfilename").and_then(|value| value.as_str()) else {
            continue;
        };
        if !file.to_ascii_lowercase().ends_with(".gguf") {
            continue;
        }
        let bytes = sibling
            .get("size")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let quantization = file.rsplit('.').nth(1).unwrap_or("").to_uppercase();
        options.push(json!({
            "id": file,
            "format": "gguf",
            "label": file.rsplit('/').next().unwrap_or(file),
            "quantization": quantization,
            "bytes": bytes,
            "sizeLabel": format_bytes(bytes),
            "files": [file],
            "shards": 1,
        }));
    }
    options.sort_by(|a, b| {
        a.get("bytes")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .cmp(&b.get("bytes").and_then(|value| value.as_u64()).unwrap_or(0))
    });
    if let Some(mlx) = mlx_download_files(&siblings) {
        options.push(mlx);
    }
    let author = repo.split('/').next().unwrap_or("");
    Ok(json!({
        "id": repo,
        "author": author,
        "name": repo.split('/').nth(1).unwrap_or(repo),
        "library": value.get("library_name").and_then(|v| v.as_str()).unwrap_or("gguf"),
        "downloads": value.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
        "likes": value.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
        "gated": value.get("gated").and_then(|v| v.as_bool()).unwrap_or(false),
        "updatedAt": value.get("lastModified"),
        "description": "",
        "task": value.get("pipeline_tag").and_then(|v| v.as_str()).unwrap_or(""),
        "tags": value.get("tags").cloned().unwrap_or_else(|| json!([])),
        "revision": value.get("sha").and_then(|v| v.as_str()).unwrap_or("main"),
        "license": "",
        "architecture": "",
        "options": options,
        "readme": "",
    }))
}

