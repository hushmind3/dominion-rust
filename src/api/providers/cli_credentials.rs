//! CLI authentication and credential import endpoints.
use super::*;

pub(super) async fn claude_cli_authenticated() -> bool {
    if home_file(".claude/.credentials.json").is_some_and(|path| path.exists()) {
        return true;
    }
    // Claude on macOS can hold its login in Keychain instead of a credentials file.
    let binary = home_file(".local/bin/claude")
        .filter(|path| path.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("claude"));
    let status = tokio::time::timeout(
        Duration::from_secs(4),
        tokio::process::Command::new(binary).args(["auth", "status"]).kill_on_drop(true).output(),
    ).await;
    matches!(status,
        Ok(Ok(output)) if output.status.success()
            && serde_json::from_slice::<Value>(&output.stdout).ok()
                .and_then(|value| value["loggedIn"].as_bool()) == Some(true)
    )
}

pub(super) fn cli_provider_id(source:&str)->String{format!("cli-{source}")}

pub(super) fn ensure_cli_provider(state:&AppState,source:&str,authenticated:bool)->bool{
 if !authenticated{return false;}
 let id=cli_provider_id(source);
 if state.store.get("provider",&id).is_none(){
  let (name,default_model)=match source{
   "claude"=>("Claude Code (CLI 계정)","default"),
   "gemini"=>("Gemini CLI (Google 계정)","auto"),
   "opencode"=>("OpenCode (CLI 계정)","default"),
   _=>return false
  };
  let record=json!({"id":id,"name":name,"baseUrl":format!("cli://{source}"),
      "kind":"cli","model":default_model,"enabled":true,"local":false,
      "toolMode":"compat","reasoningProtocol":"openai"});
  if let Some(saved)=state.store.put("provider",&record){state.bus.record("provider",&saved);}
 }
 state.store.get("provider",&id).is_some()
}

pub async fn set_cli_model(State(state): State<AppState>,Path(source): Path<String>,Json(body): Json<Value>)->Response{
 if !matches!(source.as_str(),"claude"|"gemini"|"opencode"){return error(StatusCode::BAD_REQUEST,"지원하지 않는 CLI입니다.");}
 let id=cli_provider_id(&source);
 let Some(mut record)=state.store.get("provider",&id) else{return error(StatusCode::NOT_FOUND,"먼저 CLI 계정을 연결하세요.");};
 let Some(model)=body["model"].as_str().map(str::trim).filter(|v|!v.is_empty()&&v.len()<160&&!v.chars().any(char::is_whitespace)) else{
  return error(StatusCode::BAD_REQUEST,"모델 ID가 올바르지 않습니다.");
 };
 record["model"]=json!(model);
 match state.store.put("provider",&record){
  Some(saved)=>{state.bus.record("provider",&saved);Json(public_provider(&state,saved)).into_response()},
  None=>error(StatusCode::INTERNAL_SERVER_ERROR,"모델 선택을 저장하지 못했습니다.")
 }
}

pub async fn cli_credentials(State(state): State<AppState>) -> Json<Value> {
    let codex = codex_auth().ok();
    let connected = state.store.get("provider", "codex-oauth");
    let claude = super::super::cli_oauth::credential_verified("claude").await;
    let gemini = super::super::cli_oauth::credential_verified("gemini").await;
    let opencode_auth = opencode_auth();
    let opencode = super::super::cli_oauth::credential_verified("opencode").await
        || opencode_auth.as_ref().and_then(Value::as_object).is_some_and(|entries| !entries.is_empty());
    let go_source = local_opencode_go_key().map(|(_, source)| source);
    let go_provider = direct_opencode_go(&state);
    let go_connected = go_provider.as_ref().and_then(|p| p["id"].as_str())
        .is_some_and(|id| state.store.has_secret(&format!("provider:{id}")));
    let claude_connected=ensure_cli_provider(&state,"claude",claude);
    let gemini_connected=ensure_cli_provider(&state,"gemini",gemini);
    let opencode_connected=ensure_cli_provider(&state,"opencode",opencode);
    let opencode_importable = opencode_auth
        .as_ref()
        .map(|auth| {
            opencode_api_entries(auth)
                .into_iter()
                .filter(|(provider, _)| opencode_target(provider).is_some())
                .count()
        })
        .unwrap_or(0);
    Json(json!([
        {
            "id":"codex",
            "name":"Codex (ChatGPT OAuth)",
            "loginSupported":true,
            "loginMethod":"browser",
            "available":codex.is_some() || connected.is_some(),
            "importAvailable":codex.is_some(),
            "connected":connected.is_some(),
            "supported":true,
            "accountId":connected.as_ref().and_then(|value| value["accountId"].as_str()).or_else(||codex.as_ref().and_then(|value| value["tokens"]["account_id"].as_str())),
            "model":connected.as_ref().and_then(|value| value["model"].as_str()).unwrap_or_else(||""),
            "hint":if connected.is_some() { "DOMINION에 ChatGPT 계정이 연결되어 있습니다. 사용 가능한 모델은 실제 계정 목록에서 조회합니다." } else if codex.is_some() { "Codex 앱 로그인이 감지되었습니다. 기존 인증 가져오기를 누르면 재로그인 없이 연결됩니다." } else { "브라우저에서 ChatGPT로 로그인하거나 Codex 앱의 기존 인증을 가져오세요." }
        },
        {
            "id":"claude",
            "name":"Claude Code (OAuth)",
            "connected":claude_connected,
            "model":state.store.get("provider","cli-claude").and_then(|p|p["model"].as_str().map(str::to_string)),
            "loginSupported":true,
            "loginMethod":"browser-cli",
            "available":claude,
            "supported":false,
            "hint":if claude { "Claude Code 인증 확인됨. DOMINION에서 계정 모델을 선택해 바로 대화할 수 있습니다." } else { "브라우저 로그인을 완료하면 모델이 DOMINION에 연결됩니다." }
        },
        {
            "id":"gemini",
            "name":"Gemini CLI (OAuth)",
            "connected":gemini_connected,
            "model":state.store.get("provider","cli-gemini").and_then(|p|p["model"].as_str().map(str::to_string)),
            "loginSupported":true,
            "loginMethod":"browser-cli",
            "available":gemini,
            "supported":false,
            "hint":if gemini { "Gemini CLI 로그인 확인됨. 기본 모델을 선택해 DOMINION에서 대화할 수 있습니다." } else { "브라우저 로그인 후 Gemini CLI 모델이 DOMINION에 연결됩니다." }
        },
        {
            "id":"opencode-go",
            "name":"OpenCode Go (API 키)",
            "loginSupported":false,
            "loginMethod":"api-key",
            "available":go_source.is_some() || go_connected,
            "importAvailable":go_source.is_some(),
            "supported":true,
            "connected":go_connected,
            "model":go_provider.as_ref().and_then(|p| p["model"].as_str()),
            "hint":if go_connected {
                "DOMINION에 키가 저장돼 있습니다. 실제 인증은 모델 조회로 검사하세요."
            } else if go_source.is_some() {
                "이 컴퓨터에 기존 OpenCode Go 키가 있습니다. 가져오면 DOMINION 금고에 별도 저장합니다."
            } else {
                "기존 로컬 키가 없습니다. OpenCode Go 콘솔에서 발급한 API 키를 직접 등록하세요."
            }
        },
        {
            "id":"opencode",
            "name":"OpenCode (OAuth/CLI)",
            "connected":opencode_connected,
            "model":state.store.get("provider","cli-opencode").and_then(|p|p["model"].as_str().map(str::to_string)),
            "loginSupported":true,
            "loginMethod":"browser-cli",
            "available":opencode,
            "importAvailable":opencode_importable > 0,
            "supported":opencode_importable > 0,
            "hint":if opencode { "OpenCode 인증을 확인했습니다. 사용 가능한 모델을 선택하면 DOMINION에서 실행됩니다.".to_string() } else if opencode_importable > 0 { format!("기존 OpenCode API 키 {opencode_importable}개를 가져올 수 있습니다.") } else { "브라우저 로그인 후 모델을 선택하세요.".to_string() }
        }
    ]))
}

pub async fn import_cli_credential(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let source = body["source"].as_str().unwrap_or("");
    let result = match source {
        "codex" => (|| {
            let auth = codex_auth()?;
            let saved = save_codex_provider(&state, &auth["tokens"], true)?;
            Ok(public_provider(&state, saved))
        })(),
        "opencode" => import_opencode(&state),
        "opencode-go" => import_opencode_go(&state),
        _ => Err("지원하지 않는 OAuth/CLI 소스입니다.".to_string()),
    };
    match result {
        Ok(saved) => Json(saved).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

