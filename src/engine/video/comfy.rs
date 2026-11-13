//! Isolated ComfyUI prompt/workflow transport and MP4 capture.
use super::*;

pub(super) fn substitute(value: &mut Value, variables: &Value) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}"))
                .and_then(|key| variables.get(key)) {
                *value = replacement.clone();
                return;
            }
            let mut expanded = String::new();
            let mut rest = text.as_str();
            while let Some((prefix, candidate)) = rest.split_once("{{") {
                let Some((key, suffix)) = candidate.split_once("}}") else { break };
                expanded.push_str(prefix);
                match variables.get(key) {
                    Some(Value::String(replacement)) => expanded.push_str(replacement),
                    Some(replacement) => expanded.push_str(&replacement.to_string()),
                    None => { expanded.push_str("{{"); expanded.push_str(key); expanded.push_str("}}"); }
                }
                rest = suffix;
            }
            expanded.push_str(rest);
            *text = expanded;
        }
        Value::Array(items) => items.iter_mut().for_each(|item| substitute(item, variables)),
        Value::Object(fields) => fields.values_mut().for_each(|item| substitute(item, variables)),
        _ => {}
    }
}
pub(super) async fn comfy(ctx: &JobContext, input: &Value, dir: &Path) -> Result<Value, String> {
    let cfg = &input["config"];
    let base = cfg["comfyUrl"].as_str().unwrap_or("").trim_end_matches('/');
    let parsed = reqwest::Url::parse(base).map_err(|_| "ComfyUI 주소 오류")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("ComfyUI HTTP(S) 주소를 확인하세요.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut workflow = cfg["comfyWorkflow"].clone();
    let legacy = json!({"prompt":input["brief"],"duration":input["duration"]});
    substitute(&mut workflow, input.get("render").unwrap_or(&legacy));
    ctx.checkpoint(json!({"phase":"submitting","progress":10,"videoState":{"inFlight":true}}))?;
    let response = client
        .post(format!("{base}/prompt"))
        .json(&json!({"prompt":workflow,"client_id":ctx.job_id}))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 결과를 확인할 수 없습니다.")?;
    if !response.status().is_success() {
        ctx.checkpoint(json!({"videoState":{"inFlight":false}}))?;
        return Err(format!("ComfyUI 워크플로 거부: HTTP {}", response.status()));
    }
    let answer: Value = response
        .json()
        .await
        .map_err(|_| "needs_review: ComfyUI 제출 응답 오류")?;
    let prompt_id = answer["prompt_id"]
        .as_str()
        .ok_or("needs_review: ComfyUI prompt_id가 없습니다.")?;
    ctx.checkpoint(json!({"phase":"rendering","progress":25,"videoState":{"inFlight":true,"promptId":prompt_id}}))?;
    let start = std::time::Instant::now();
    loop {
        ctx.checkpoint(json!({}))?;
        if start.elapsed() > Duration::from_secs(900) {
            return Err(
                "needs_review: ComfyUI 생성 대기시간이 지났습니다. 원격 작업 상태를 확인하세요."
                    .into(),
            );
        }
        let history: Value = client
            .get(format!("{base}/history/{prompt_id}"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let result = &history[prompt_id];
        if result["status"]["status_str"] == "error" {
            ctx.checkpoint(json!({"videoState":{"inFlight":false,"promptId":prompt_id}}))?;
            return Err("ComfyUI 워크플로 실행 실패".into());
        }
        if let Some(outputs) = result["outputs"].as_object() {
            for node in outputs.values() {
                for field in ["videos", "gifs"] {
                    if let Some(files) = node[field].as_array() {
                        for file in files {
                            let filename = file["filename"].as_str().unwrap_or("");
                            if !filename.to_lowercase().ends_with(".mp4") {
                                continue;
                            }
                            let response = client
                                .get(format!("{base}/view"))
                                .query(&[
                                    ("filename", filename),
                                    ("subfolder", file["subfolder"].as_str().unwrap_or("")),
                                    ("type", file["type"].as_str().unwrap_or("output")),
                                ])
                                .timeout(Duration::from_secs(120))
                                .send()
                                .await
                                .map_err(|e| e.to_string())?;
                            if !response.status().is_success() {
                                return Err("ComfyUI 결과 다운로드 실패".into());
                            }
                            use futures_util::StreamExt;
                            use tokio::io::AsyncWriteExt;
                            let mut stream = response.bytes_stream();
                            let mut total = 0usize;
                            let mut output = tokio::fs::File::create(dir.join("output.part.mp4"))
                                .await
                                .map_err(|e| e.to_string())?;
                            while let Some(chunk) = stream.next().await {
                                ctx.checkpoint(json!({}))?;
                                let chunk = chunk.map_err(|e| e.to_string())?;
                                total += chunk.len();
                                if total > 256 * 1024 * 1024 {
                                    return Err("영상 결과가 256MiB 제한을 넘었습니다.".into());
                                }
                                output.write_all(&chunk).await.map_err(|e| e.to_string())?;
                            }
                            drop(output);
                            std::fs::rename(dir.join("output.part.mp4"), dir.join("output.mp4"))
                                .map_err(|e| e.to_string())?;
                            ctx.checkpoint(
                                json!({"videoState":{"inFlight":false,"promptId":prompt_id}}),
                            )?;
                            return Ok(
                                json!({"engine":"comfyui","promptId":prompt_id,"bytes":total}),
                            );
                        }
                    }
                }
            }
            if result["status"]["completed"] == true {
                return Err(
                    "ComfyUI 완료 결과에 MP4가 없습니다. 영상 출력 노드를 확인하세요.".into(),
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
