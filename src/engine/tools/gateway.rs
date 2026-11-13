//! External GitHub and delegation boundaries.
//! Active Rust execution paths imported by tools.rs.
use super::workspace::*;
use super::*;

pub(crate) async fn github_api(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let token = crate::engine::oauth::access_token(ctx.state, "github")
        .ok_or("GitHub 계정이 연결되지 않았습니다. 설정 > 계정 연동에서 연결하세요.")?;
    let method = string_arg(args, "method").unwrap_or("GET").to_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PATCH" | "PUT" | "DELETE") {
        return Err("지원하지 않는 HTTP 메서드입니다.".into());
    }
    let path = required_arg(args, "path")?;
    if !path.starts_with('/') || path.contains("..") {
        return Err("GitHub API 경로는 / 로 시작해야 합니다.".into());
    }
    let method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| error.to_string())?;
    let mut request = reqwest::Client::new()
        .request(method, format!("https://api.github.com{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "DOMINION")
        .timeout(Duration::from_secs(30));
    if let Some(body) = args.get("body").filter(|value| !value.is_null()) {
        request = request.json(body);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
    Ok(json!({
        "status": status,
        "body": parsed.to_string().chars().take(20_000).collect::<String>(),
    }))
}

const MAX_DELEGATION_DEPTH: usize = 4;

/// Number of delegated sessions between the owner-facing session and this one.
/// A chain cannot loop forever, so a cycle is cut defensively.
pub(crate) fn delegation_depth(store: &Store, session_id: &str) -> usize {
    let mut depth = 0;
    let mut seen = std::collections::HashSet::new();
    let mut current = Some(session_id.to_string());
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(session) = store.get("session", &id) else {
            break;
        };
        if session["delegated"].as_bool() == Some(true) {
            depth += 1;
        }
        current = session["parentSessionId"].as_str().map(str::to_string);
    }
    depth
}

pub(crate) async fn delegate(ctx: &ToolContext<'_>, args: &Value) -> Result<Value, String> {
    let agent_id = required_arg(args, "agentId")?;
    // A delegated worker cannot delegate beyond a bounded depth. This keeps a
    // runaway chain from consuming unbounded workers.
    if delegation_depth(ctx.store, ctx.session_id) >= MAX_DELEGATION_DEPTH {
        return Err(format!(
            "위임 깊이 제한({MAX_DELEGATION_DEPTH})에 도달했습니다. 상위 작업으로 결과를 모아 처리하세요."
        ));
    }
    let prompt = required_arg(args, "prompt")?;
    let agent = ctx
        .state
        .store
        .get("agent", &agent_id)
        .ok_or_else(|| format!("위임 대상 에이전트를 찾을 수 없습니다: {agent_id}"))?;
    let wait = args["wait"].as_bool().unwrap_or(true);
    let timeout = args["timeoutSeconds"]
        .as_u64()
        .unwrap_or(90)
        .clamp(5, 300);

    let session = ctx
        .state
        .store
        .put(
            "session",
            &json!({
                "title": format!("Delegate · {}", prompt.chars().take(36).collect::<String>()),
                "agentId": agent_id,
                "providerId": agent["providerId"],
                "model": agent["model"],
                "parentSessionId": ctx.session_id,
                "delegated": true,
            }),
        )
        .ok_or("위임 세션을 만들 수 없습니다.")?;
    let session_id = session["id"].as_str().unwrap_or("").to_string();
    let job = crate::engine::jobs::enqueue(
        ctx.state,
        "prompt",
        json!({
            "prompt": prompt,
            "agentId": agent_id,
            "providerId": agent["providerId"],
            "model": agent["model"],
            "sessionId": session_id,
            "toolsEnabled": ctx.tools_enabled && ctx.tools_execution_enabled,
            "toolsExecutionEnabled": ctx.tools_execution_enabled,
            "parentSessionId": ctx.session_id,
            "parentAgentId": ctx.agent_id,
            "delegation": true,
        }),
    )
    .ok_or("위임 작업을 만들 수 없습니다.")?;
    let job_id = job["id"].as_str().unwrap_or("").to_string();
    crate::engine::jobs::start_now(ctx.state.clone(), job_id.clone());

    if !wait {
        return Ok(json!({
            "jobId": job_id, "status": "queued", "sessionId": session_id,
            "hint": "task_status 또는 task_result로 확인하세요."
        }));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        if ctx.state.is_cancelled(ctx.session_id) {
            return Err("실행이 중단되었습니다. 위임 작업은 백그라운드에서 계속됩니다.".into());
        }
        let Some(current) = ctx.state.store.get("job", &job_id) else {
            return Err("위임 작업 기록이 사라졌습니다.".into());
        };
        let status = current["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled" | "needs_review") {
            return task_payload(ctx, &json!({"jobId": job_id}), true);
        }
        if std::time::Instant::now() >= deadline {
            return Ok(json!({
                "jobId": job_id, "status": status, "sessionId": session_id,
                "hint": "아직 실행 중입니다. task_result로 나중에 확인하세요."
            }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

pub(crate) fn task_payload(ctx: &ToolContext<'_>, args: &Value, with_result: bool) -> Result<Value, String> {
    let job_id = required_arg(args, "jobId")?;
    let job = ctx
        .state
        .store
        .get("job", &job_id)
        .ok_or_else(|| format!("작업을 찾을 수 없습니다: {job_id}"))?;
    // A job result is visible only to the session that created it or to the
    // parent session that delegated it.
    let owned = job["input"]["sessionId"].as_str() == Some(ctx.session_id)
        || job["input"]["parentSessionId"].as_str() == Some(ctx.session_id);
    if !owned {
        return Err("이 작업에 접근할 권한이 없습니다.".into());
    }
    let status = job["status"].as_str().unwrap_or("unknown");
    let mut payload = json!({
        "jobId": job_id,
        "kind": job["kind"],
        "status": status,
        "stepIndex": job["stepIndex"],
        "stepTotal": job["stepTotal"],
        "error": job["error"],
        "result": job["result"],
    });
    if !with_result || !matches!(status, "completed" | "failed" | "needs_review") {
        return Ok(payload);
    }
    let Some(session_id) = job
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .or_else(|| job["sessionId"].as_str())
    else {
        return Ok(payload);
    };
    let messages = ctx.state.store.messages(session_id);
    let answer = messages
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .and_then(|message| message["content"].as_str())
        .unwrap_or("")
        .chars()
        .take(8000)
        .collect::<String>();
    let operations = ctx
        .state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|operation| {
            operation["runId"].as_str().is_some()
                && operation["sessionId"].as_str() == Some(session_id)
                && operation["status"] == "completed"
        })
        .map(|operation| {
            json!({"tool":operation["tool"],"arguments":operation["arguments"],"result":operation["result"]})
        })
        .take(20)
        .collect::<Vec<_>>();
    payload["sessionId"] = json!(session_id);
    payload["content"] = json!(answer);
    payload["operations"] = json!(operations);
    Ok(payload)
}

