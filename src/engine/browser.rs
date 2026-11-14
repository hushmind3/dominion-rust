//! Browser GUI control through the Chrome DevTools Protocol.
//!
//! DOMINION launches a local Chrome/Chromium instance with remote debugging
//! enabled and drives it over CDP. Every call opens a short-lived websocket to
//! the current page target, sends one command and closes it, so the engine
//! never holds a socket open between tool calls.

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

struct Session {
    child: Child,
    port: u16,
    profile: PathBuf,
}

static SESSION: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

fn session() -> &'static Mutex<Option<Session>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

fn chrome_bin() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DOMINION_CHROME") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mac_apps: [&str; 4] = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];
    if cfg!(target_os = "macos") {
        if let Some(path) = mac_apps.iter().map(PathBuf::from).find(|path| path.is_file()) {
            return Some(path);
        }
    }
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "brave-browser",
        "microsoft-edge",
    ];
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|path| path.is_file())
}

fn chrome_available() -> bool {
    chrome_bin().is_some()
}

fn devtools_port(profile: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(profile.join("DevToolsActivePort")).ok()?;
    text.lines().next()?.trim().parse().ok()
}

async fn wait_for_port(profile: &Path, child: &mut Child) -> Result<u16, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(port) = devtools_port(profile) {
            let ready = reqwest::get(format!("http://127.0.0.1:{port}/json/version"))
                .await
                .map(|response| response.status().is_success())
                .unwrap_or(false);
            if ready {
                return Ok(port);
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "브라우저가 실행 중 종료되었습니다 (exit {status}). Chrome 원격 디버깅을 사용할 수 없습니다."
            ));
        }
        if tokio::time::Instant::now() > deadline {
            return Err("브라우저 원격 디버깅 포트를 열지 못했습니다.".into());
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

async fn ensure_session(data_dir: &Path) -> Result<u16, String> {
    if let Ok(mut guard) = session().lock() {
        if let Some(existing) = guard.as_mut() {
            if existing.child.try_wait().ok().flatten().is_none() {
                return Ok(existing.port);
            }
            *guard = None;
        }
    }
    let bin = chrome_bin().ok_or(
        "Chrome 또는 Chromium을 찾을 수 없습니다. DOMINION_CHROME 환경 변수로 실행 파일을 지정할 수 있습니다.",
    )?;
    let profile = data_dir.join("browser-profile");
    std::fs::create_dir_all(&profile).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(profile.join("DevToolsActivePort"));
    let mut child = Command::new(&bin)
        .args([
            "--headless=new",
            "--remote-debugging-port=0",
            "--remote-debugging-address=127.0.0.1",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-dev-shm-usage",
            "--disable-gpu",
            "--hide-scrollbars",
            "--window-size=1280,900",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("브라우저 실행 실패: {error}"))?;
    let port = match wait_for_port(&profile, &mut child).await {
        Ok(port) => port,
        Err(error) => {
            let _ = child.kill();
            return Err(error);
        }
    };
    if let Ok(mut guard) = session().lock() {
        *guard = Some(Session {
            child,
            port,
            profile,
        });
    }
    Ok(port)
}

async fn page_target(port: u16) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let targets = client
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| format!("브라우저 대상 조회 실패: {error}"))?
        .error_for_status()
        .map_err(|error| format!("브라우저 대상 HTTP 오류: {error}"))?
        .json::<Value>()
        .await
        .map_err(|error| format!("브라우저 대상 응답 오류: {error}"))?;
    if let Some(page) = targets
        .as_array()
        .into_iter()
        .flatten()
        .find(|target| target["type"] == "page" && target["webSocketDebuggerUrl"].is_string())
    {
        return Ok(page.clone());
    }
    let created = client
        .put(format!("http://127.0.0.1:{port}/json/new?about:blank"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| format!("브라우저 탭 생성 실패: {error}"))?;
    let created = created
        .error_for_status()
        .map_err(|error| format!("브라우저 탭 HTTP 오류: {error}"))?
        .json::<Value>()
        .await
        .map_err(|error| format!("브라우저 탭 응답 오류: {error}"))?;
    if created["webSocketDebuggerUrl"].is_string() {
        return Ok(created);
    }
    Err("브라우저 페이지 대상을 찾을 수 없습니다.".into())
}

async fn cdp(ws_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async(ws_url),
    )
    .await
    .map_err(|_| "브라우저 DevTools 연결 시간 초과".to_string())?
    .map_err(|error| format!("브라우저 DevTools 연결 실패: {error}"))?;
    let payload = json!({ "id": 1, "method": method, "params": params }).to_string();
    socket
        .send(Message::Text(payload.into()))
        .await
        .map_err(|error| format!("브라우저 명령 전송 실패: {error}"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("브라우저 명령 시간 초과: {method}"));
        }
        let message = tokio::time::timeout(remaining, socket.next())
            .await
            .map_err(|_| format!("브라우저 명령 시간 초과: {method}"))?;
        match message {
            Some(Ok(Message::Text(text))) => {
                let value: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if value["id"] != 1 {
                    continue;
                }
                let _ = socket.close(None).await;
                if let Some(error) = value.get("error") {
                    return Err(format!(
                        "브라우저 오류: {}",
                        error["message"].as_str().unwrap_or("알 수 없는 오류")
                    ));
                }
                return Ok(value["result"].clone());
            }
            Some(Ok(_)) => continue,
            Some(Err(error)) => return Err(format!("브라우저 DevTools 오류: {error}")),
            None => return Err("브라우저 DevTools 연결이 닫혔습니다.".into()),
        }
    }
}

async fn evaluate(ws_url: &str, expression: &str) -> Result<Value, String> {
    let result = cdp(
        ws_url,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
        }),
    )
    .await?;
    if let Some(exception) = result.get("exceptionDetails") {
        return Err(format!(
            "페이지 스크립트 오류: {}",
            exception["text"].as_str().unwrap_or("알 수 없는 오류")
        ));
    }
    Ok(result["result"]["value"].clone())
}

async fn navigate(ws_url: &str, url: &str) -> Result<Value, String> {
    cdp(ws_url, "Page.enable", json!({})).await?;
    let result = cdp(ws_url, "Page.navigate", json!({ "url": url })).await?;
    if let Some(error) = result["errorText"].as_str().filter(|text| !text.is_empty()) {
        return Err(format!("페이지 이동 실패: {error}"));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let state = evaluate(ws_url, "document.readyState").await?;
        if state.as_str() == Some("complete") {
            return Ok(json!({
                "url": evaluate(ws_url, "location.href").await?.as_str().unwrap_or(url),
                "title": evaluate(ws_url, "document.title").await?,
            }));
        }
        if tokio::time::Instant::now() > deadline {
            return Err("페이지 로딩 시간이 초과되었습니다.".into());
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

const OBSERVE_SCRIPT: &str = r#"(() => {
  const selector = 'a,button,input,textarea,select,[role=button],[role=link],[contenteditable=true]';
  const elements = [...document.querySelectorAll(selector)].filter((element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
  }).slice(0, 150).map((element, index) => {
    element.setAttribute('data-dominion-idx', String(index));
    const text = (element.innerText || element.value || element.getAttribute('placeholder') || '').trim().replace(/\s+/g, ' ').slice(0, 120);
    const label = (element.getAttribute('aria-label') || element.getAttribute('name') || '').slice(0, 80);
    return {
      index,
      tag: element.tagName.toLowerCase(),
      type: element.getAttribute('type') || '',
      label,
      text,
      href: element.getAttribute('href') || '',
      disabled: Boolean(element.disabled),
    };
  });
  return {
    title: document.title,
    url: location.href,
    text: (document.body ? document.body.innerText : '').slice(0, 12000),
    elements,
  };
})()"#;

fn element_index(args: &Value) -> Result<u64, String> {
    args["index"]
        .as_u64()
        .ok_or_else(|| "index 인자가 필요합니다.".to_string())
}

fn element_script(index: u64, body: &str) -> String {
    format!(
        "(() => {{ const el = document.querySelector('[data-dominion-idx=\"{index}\"]'); if (!el) return {{ ok: false, error: '요소를 찾을 수 없습니다. 먼저 observe를 실행하세요.' }}; {body} }})()"
    )
}

async fn observe(ws_url: &str) -> Result<Value, String> {
    let value = evaluate(ws_url, OBSERVE_SCRIPT).await?;
    if value.is_null() {
        return Err("페이지 관찰 결과가 비어 있습니다.".into());
    }
    Ok(json!({
        "title": value["title"],
        "url": value["url"],
        "text": value["text"],
        "elements": value["elements"],
        "elementCount": value["elements"].as_array().map(Vec::len).unwrap_or(0),
    }))
}

async fn click(ws_url: &str, index: u64) -> Result<Value, String> {
    let script = element_script(
        index,
        "el.scrollIntoView({ block: 'center', inline: 'center' }); el.focus(); el.click(); return { ok: true, tag: el.tagName.toLowerCase(), text: (el.innerText || el.value || '').trim().slice(0, 120) };",
    );
    let value = evaluate(ws_url, &script).await?;
    if value["ok"] != true {
        return Err(value["error"].as_str().unwrap_or("클릭에 실패했습니다.").into());
    }
    Ok(json!({ "clicked": true, "index": index, "tag": value["tag"], "text": value["text"] }))
}

async fn type_text(ws_url: &str, index: u64, text: &str) -> Result<Value, String> {
    let literal = serde_json::to_string(text).map_err(|error| error.to_string())?;
    let script = element_script(
        index,
        &format!(
            "el.scrollIntoView({{ block: 'center', inline: 'center' }}); el.focus(); const text = {literal}; if (el.isContentEditable) {{ el.innerText = text; }} else {{ const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype; const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set; if (setter) setter.call(el, text); else el.value = text; }} el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); return {{ ok: true, value: (el.value || el.innerText || '').slice(0, 200) }};"
        ),
    );
    let value = evaluate(ws_url, &script).await?;
    if value["ok"] != true {
        return Err(value["error"].as_str().unwrap_or("입력에 실패했습니다.").into());
    }
    Ok(json!({ "typed": true, "index": index, "value": value["value"] }))
}

async fn press(ws_url: &str, key: &str) -> Result<Value, String> {
    let (code, key_code) = match key {
        "Enter" => ("Enter", 13),
        "Escape" => ("Escape", 27),
        "Tab" => ("Tab", 9),
        "ArrowUp" => ("ArrowUp", 38),
        "ArrowDown" => ("ArrowDown", 40),
        "ArrowLeft" => ("ArrowLeft", 37),
        "ArrowRight" => ("ArrowRight", 39),
        "Backspace" => ("Backspace", 8),
        _ => return Err(format!("지원하지 않는 키입니다: {key}")),
    };
    let params = json!({
        "type": "keyDown",
        "key": key,
        "code": code,
        "windowsVirtualKeyCode": key_code,
        "nativeVirtualKeyCode": key_code,
    });
    cdp(ws_url, "Input.dispatchKeyEvent", params.clone()).await?;
    let mut up = params;
    up["type"] = json!("keyUp");
    cdp(ws_url, "Input.dispatchKeyEvent", up).await?;
    Ok(json!({ "pressed": key }))
}

async fn screenshot(ws_url: &str, workspace: &Path) -> Result<Value, String> {
    let result = cdp(
        ws_url,
        "Page.captureScreenshot",
        json!({ "format": "png", "captureBeyondViewport": false }),
    )
    .await?;
    let encoded = result["data"]
        .as_str()
        .ok_or("스크린샷 데이터가 없습니다.")?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("스크린샷 디코딩 실패: {error}"))?;
    let dir = workspace.join(".dominion").join("browser");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!(
        "screenshot-{}.png",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f")
    ));
    std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "bytes": bytes.len(),
    }))
}

fn close_session() -> Value {
    let mut closed = false;
    if let Ok(mut guard) = session().lock() {
        if let Some(mut current) = guard.take() {
            let _ = current.child.kill();
            let _ = current.child.wait();
            let _ = std::fs::remove_file(current.profile.join("DevToolsActivePort"));
            closed = true;
        }
    }
    json!({ "closed": closed })
}

/// Run one browser action. Actions: status, open, observe, click, type, press,
/// screenshot, close.
pub async fn execute(
    action: &str,
    args: &Value,
    data_dir: &Path,
    workspace: &Path,
) -> Result<Value, String> {
    match action {
        "status" => {
            let mut running = false;
            let mut port = None;
            if let Ok(mut guard) = session().lock() {
                if let Some(current) = guard.as_mut() {
                    if current.child.try_wait().ok().flatten().is_none() {
                        running = true;
                        port = Some(current.port);
                    } else {
                        *guard = None;
                    }
                }
            }
            return Ok(json!({
                "available": chrome_available(),
                "running": running,
                "port": port,
                "browser": chrome_bin().map(|path| path.to_string_lossy().to_string()),
            }));
        }
        "close" => return Ok(close_session()),
        _ => {}
    }
    let port = ensure_session(data_dir).await?;
    let target = page_target(port).await?;
    let ws_url = target["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("브라우저 페이지 WebSocket 주소가 없습니다.")?;
    match action {
        "open" => {
            let url = args["url"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or("url 인자가 필요합니다.")?;
            let mut result = navigate(ws_url, url).await?;
            result["targetId"] = target["id"].clone();
            Ok(result)
        }
        "observe" => {
            let mut snapshot = observe(ws_url).await?;
            snapshot["targetId"] = target["id"].clone();
            Ok(snapshot)
        }
        "click" => click(ws_url, element_index(args)?).await,
        "type" => {
            let text = args["text"].as_str().ok_or("text 인자가 필요합니다.")?;
            type_text(ws_url, element_index(args)?, text).await
        }
        "press" => {
            let key = args["key"].as_str().ok_or("key 인자가 필요합니다.")?;
            press(ws_url, key).await
        }
        "screenshot" => screenshot(ws_url, workspace).await,
        other => Err(format!(
            "지원하지 않는 브라우저 동작입니다: {other} (status, open, observe, click, type, press, screenshot, close)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_script_quotes_the_index() {
        let script = element_script(3, "return { ok: true };");
        assert!(script.contains("data-dominion-idx=\"3\""));
        assert!(script.contains("return { ok: true };"));
    }

    #[tokio::test]
    async fn controls_a_local_page_end_to_end() {
        // This test launches a real browser and requires a working Chrome
        // sandbox/CDP port, not merely a Chrome binary on PATH. General Rust
        // CI runners may expose Chrome but block remote debugging. Keep this
        // as a real, explicit browser smoke instead of a flaky unit gate:
        // DOMINION_BROWSER_E2E=1 cargo test --release controls_a_local_page_end_to_end
        if std::env::var("DOMINION_BROWSER_E2E").as_deref() != Ok("1") {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "dominion-browser-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let status = execute("status", &json!({}), &dir, &dir).await.unwrap();
        if status["available"] != true {
            return;
        }
        let html = "<html><head><title>Start</title></head><body><h1>Hello</h1><button onclick=\"document.title='Clicked'\">Press me</button><input id='n'></body></html>";
        use base64::Engine;
        let url = format!(
            "data:text/html;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(html)
        );
        let opened = execute("open", &json!({ "url": url }), &dir, &dir)
            .await
            .unwrap();
        assert_eq!(opened["title"], "Start");
        let snapshot = execute("observe", &json!({}), &dir, &dir).await.unwrap();
        let button = snapshot["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|element| element["text"] == "Press me")
            .expect("button element")
            .clone();
        let index = button["index"].as_u64().unwrap();
        execute("click", &json!({ "index": index }), &dir, &dir)
            .await
            .unwrap();
        let after = execute("observe", &json!({}), &dir, &dir).await.unwrap();
        assert_eq!(after["title"], "Clicked");
        let input = after["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|element| element["tag"] == "input")
            .expect("input element")["index"]
            .as_u64()
            .unwrap();
        execute(
            "type",
            &json!({ "index": input, "text": "dominion" }),
            &dir,
            &dir,
        )
        .await
        .unwrap();
        let typed = execute("observe", &json!({}), &dir, &dir).await.unwrap();
        assert!(typed["elements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|element| element["text"] == "dominion"));
        let shot = execute("screenshot", &json!({}), &dir, &dir)
            .await
            .unwrap();
        assert!(std::path::Path::new(shot["path"].as_str().unwrap()).is_file());
        assert!(shot["bytes"].as_u64().unwrap() > 0);
        execute("close", &json!({}), &dir, &dir).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn observe_script_collects_interactive_elements() {
        assert!(OBSERVE_SCRIPT.contains("document.querySelectorAll"));
        assert!(OBSERVE_SCRIPT.contains("data-dominion-idx"));
        assert!(OBSERVE_SCRIPT.contains("elements"));
    }
}
