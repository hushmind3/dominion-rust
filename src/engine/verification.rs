//! Shared acceptance checks used by ordinary chat and goal execution.
use crate::state::AppState;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub(crate) fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
/// Only explicit, bounded and machine-checkable predicates are accepted.
pub fn prepare_acceptance_checks(state:&AppState,input:&str)->Value {
    json!(baseline_checks(state,infer_criteria(input)))
}
pub fn check_acceptance(state:&AppState,input:&str,checks:&Value,evidence:&[Value])->Vec<String>{
    acceptance_gaps(state,input,checks,evidence)
}
pub fn validate_criteria(criteria: &Value) -> Result<(), String> {
    let items = criteria
        .as_array()
        .ok_or("successCriteria는 배열이어야 합니다.")?;
    if items.len() > 20 {
        return Err("검증 조건은 최대 20개입니다.".into());
    }
    for item in items {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or("검증 조건의 type이 필요합니다.")?;
        let required: &[&str] = match kind {
            "file_exists" | "file_absent" | "file_changed" => &["path"],
            "file_contains" => &["path", "contains"],
            "file_equals" => &["path", "content"],
            "tool_succeeded" => &["tool"],
            "command_succeeded" => &["command"],
            _ => return Err(format!("지원하지 않는 검증 조건: {kind}")),
        };
        if let Some(path) = item.get("path").and_then(Value::as_str) {
            let path = Path::new(path);
            if !path.is_absolute()
                && path
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err("상대 검증 경로는 워크스페이스 안에 있어야 합니다.".into());
            }
        }
        for key in required {
            let value = item
                .get(*key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| format!("{kind} 조건에 {key}가 필요합니다."))?;
            if value.len() > 4096 {
                return Err("검증 조건 값이 너무 깁니다.".into());
            }
        }
    }
    Ok(())
}
/// Treat Korean inflected verbs as substrings but English instructions as
/// complete words. "brunch", "preview" or "prerun" must not silently turn a
/// read-only user request into a mandatory side-effect/tool execution.
pub(crate) fn looks_actionable(goal: &str) -> bool {
    crate::engine::text_intent::has_any(goal, &[
        "만들","생성","저장","작성","수정","편집","삭제","제거","실행",
        "설치","복사","이동","다운로드","빌드","커밋","배포",
        "create","write","save","edit","delete","remove","run",
        "install","build","update","deploy","commit","fix","refactor",
        "patch","implement","modify","execute","test","compile",
    ])
}
pub(crate) fn infer_criteria(goal: &str) -> Vec<Value> {
    if !looks_actionable(goal) {
        return vec![];
    }
    let lower = goal.to_lowercase();
    let deletion=crate::engine::text_intent::has_any(&lower,
        &["삭제","제거","delete","remove"]);
    let writing=crate::engine::text_intent::has_any(&lower, &[
        "만들","생성","저장","작성","수정","편집",
        "create","write","save","edit","update","fix","refactor","patch","implement","modify"
    ]);
    if !deletion && !writing {
        return vec![];
    }
    if !deletion {
        if let Some(replacement) = goal.split('`').nth(1)
            .and_then(|path| literal_replacement(goal, path))
        {
            return vec![replacement];
        }
    }
    // "a.txt를 읽고 b.txt에 저장해" must not require the read-only input to
    // change. With both a read and a write verb, only the last path is the
    // likely output; without a read verb every path is still inferred.
    let reading=crate::engine::text_intent::has_any(&lower,
        &["읽","확인","조회","read","check"]);
    let mut paths = Vec::new();
    for raw in goal.split(|c: char| {
        c.is_whitespace() || matches!(c, '`' | '\'' | '"' | ',' | '(' | ')' | '[' | ']')
    }) {
        // Korean particles can follow a path without a space (a.txt를).
        let token = raw.trim_end_matches(|c: char| matches!(c, '.' | ':' | ';' | '!' | '?'));
        let candidate = token.char_indices().find_map(|(index, ch)| {
            (ch == '.')
                .then(|| {
                    let ext: String = token[index + 1..]
                        .chars()
                        .take_while(|ch| ch.is_ascii_alphanumeric())
                        .collect();
                    (index, ext)
                })
                .filter(|(_, ext)| {
                    [
                        "txt", "md", "rs", "json", "py", "js", "ts", "tsx", "csv", "html", "toml",
                        "yaml", "yml", "sh",
                    ]
                    .contains(&ext.as_str())
                })
        });
        if let Some((index, ext)) = candidate {
            let path = &token[..index + 1 + ext.len()];
            if path.contains("://") || path.starts_with('-') || path.is_empty() {
                continue;
            }
            if !paths
                .iter()
                .any(|v: &Value| v.get("path").and_then(Value::as_str) == Some(path))
            {
                paths.push(json!({"type":if deletion {"file_absent"} else {"file_changed"},"path":path,"inferred":true}));
            }
        }
    }
    if reading && paths.len() > 1 {
        if let Some(output) = paths.pop() {
            paths = vec![output];
        }
    }
    if let [check] = paths.as_mut_slice() {
        let path = check["path"].as_str().unwrap_or_default();
        if !deletion {
            if let Some(content) = literal_creation(goal, path) {
                *check = json!({"type":"file_equals","path":path,"content":content,"inferred":true});
            }
        }
    }
    paths
}

/// Recognize a bounded, unambiguous owner contract. The hash is captured
/// before any tool runs and covers every untouched byte as well as the edit.
fn literal_replacement(goal: &str, path: &str) -> Option<Value> {
    let mut parts = goal.trim().split('`');
    let (before, file, edit, old, middle, new, after) = (
        parts.next()?, parts.next()?, parts.next()?, parts.next()?,
        parts.next()?, parts.next()?, parts.next()?,
    );
    if parts.next().is_some() || file != path || path.trim().is_empty() || path.len() > 4096
        || path.chars().any(char::is_control) || old.is_empty() || old == new
        || old.len() > 4096 || new.len() > 4096
    {
        return None;
    }
    let korean = before.is_empty()
        && matches!(edit.trim(), "파일을 수정해." | "파일을 수정해 주세요.")
        && matches!(middle.trim(), "을" | "를")
        && matches!(after.trim_end_matches('.').trim(),
            "으로 바꾸고 나머지 내용은 그대로 유지해" | "로 바꾸고 나머지 내용은 그대로 유지해");
    let english = before.eq_ignore_ascii_case("edit ")
        && edit.eq_ignore_ascii_case(" and replace ")
        && middle.eq_ignore_ascii_case(" with ")
        && after.trim_end_matches('.').trim().eq_ignore_ascii_case("and preserve everything else");
    (korean || english).then(|| json!({
        "type":"file_replace_exact", "path":path, "old":old, "new":new, "inferred":true,
    }))
}

fn literal_creation<'a>(goal: &'a str, path: &str) -> Option<&'a str> {
    let rest = goal.trim().trim_end_matches('.').strip_prefix(path)?
        .strip_prefix(" 파일을 만들고 내용을 ")?.strip_suffix("로 저장해")?;
    let content = rest.strip_prefix('`').and_then(|value| value.strip_suffix('`'))
        .unwrap_or(rest);
    (!content.is_empty() && content.len() <= 4096
        && content.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
        .then_some(content)
}

pub(crate) fn has_result_verifier(checks: &Value) -> bool {
    checks.as_array().is_some_and(|items| {
        !items.is_empty() && items.iter().all(|item| {
            item["inferred"] != true || item["type"] != "file_changed"
        })
    })
}

fn read_bounded_file(path: &Path) -> Option<Vec<u8>> {
    const LIMIT: u64 = 8 * 1024 * 1024;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= LIMIT).then_some(bytes)
}

fn exact_replacement(bytes: Vec<u8>, old: &str, new: &str) -> Option<String> {
    let content = String::from_utf8(bytes).ok()?;
    if old.is_empty() || content.matches(old).count() != 1 {
        return None;
    }
    Some(content.replacen(old, new, 1))
}

pub(crate) fn resolve_path(state: &AppState, path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("상대 검증 경로는 워크스페이스 밖으로 나갈 수 없습니다.".into());
    }
    let root = state
        .config
        .workspace
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let resolved = root.join(path);
    let mut ancestor = resolved.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or("검증 경로의 상위 폴더를 찾을 수 없습니다.")?;
    }
    if !ancestor
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(&root)
    {
        return Err("검증 경로가 심볼릭 링크를 통해 워크스페이스 밖으로 나갑니다.".into());
    }
    Ok(resolved)
}
pub(crate) fn fingerprint(path: &Path) -> Value {
    use std::hash::{Hash, Hasher};
    let Ok(meta) = std::fs::metadata(path) else {
        return Value::Null;
    };
    // Checkpoints are compact. This hash detects changes; it is not a security
    // signature and is never used to authorize access.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if meta.is_file() && meta.len() <= 8 * 1024 * 1024 {
        if let Ok(bytes) = std::fs::read(path) {
            bytes.hash(&mut hasher);
            return json!({"bytes":meta.len(),"hash":hasher.finish().to_string()});
        }
    }
    json!({"bytes":meta.len(),"modified":meta.modified().ok().and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_nanos().to_string())})
}
pub(crate) fn baseline_checks(state: &AppState, mut checks: Vec<Value>) -> Value {
    for check in &mut checks {
        if check["type"] == "file_replace_exact" {
            let original = resolve_path(state, check["path"].as_str().unwrap_or_default())
                .ok().and_then(|path| read_bounded_file(&path));
            check["sourceBytes"] = json!(original.as_ref().map(Vec::len));
            check["expectedSha256"] = original.and_then(|bytes| exact_replacement(
                bytes, check["old"].as_str()?, check["new"].as_str()?,
            )).map(|expected| format!("{:x}", Sha256::digest(expected.as_bytes())))
                .map(Value::String).unwrap_or(Value::Null);
        } else if check["type"] == "file_changed"
            || (check["type"] == "file_equals" && check["inferred"] == true)
        {
            check["baseline"] = resolve_path(state, check["path"].as_str().unwrap_or(""))
                .map(|path| fingerprint(&path))
                .unwrap_or(Value::Null);
        }
    }
    json!(checks)
}
pub(crate) fn successful_evidence(item: &Value) -> bool {
    let result = &item["result"];
    if result.is_null() || tool_name(item).is_empty() {
        return false;
    }
    if item.get("status").and_then(Value::as_str) == Some("failed")
        || result.get("isError").and_then(Value::as_bool) == Some(true)
        || result.get("success").and_then(Value::as_bool) == Some(false)
        || (matches!(tool_name(item),
            "workspace_write" | "file_write" | "workspace_replace"
            | "workspace_replace_range" | "workspace_replace_function")
            && result.get("written").and_then(Value::as_bool) == Some(false))
        || result
            .get("error")
            .map(|value| !value.is_null())
            .unwrap_or(false)
    {
        return false;
    }
    if let Some(code) = result.get("exitCode") {
        return code.as_i64() == Some(0);
    }
    true
}
pub(crate) fn tool_name(item: &Value) -> &str {
    item.get("tool")
        .or_else(|| item.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
}
pub(crate) fn is_action_evidence(item: &Value) -> bool {
    if !successful_evidence(item) {
        return false;
    }
    match tool_name(item) {
        "workspace_write" | "file_write"
        | "workspace_replace" | "workspace_replace_range"
        | "workspace_replace_function" => true,
        "shell_exec" | "terminal_exec" => {
            let args = item
                .get("arguments")
                .or_else(|| item.get("args"))
                .unwrap_or(&Value::Null);
            let command = string(args, "command");
            let first = command.split_whitespace().next().unwrap_or("");
            let program = Path::new(first)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            // A small conservative filter, never a shell parser. Complex or
            // unknown commands need a command_succeeded acceptance criterion.
            if tool_name(item) == "shell_exec"
                && command
                    .chars()
                    .any(|c| matches!(c, ';' | '|' | '&' | '>' | '<' | '\n'))
            {
                return false;
            }
            match program {
                "mkdir" | "rmdir" | "cp" | "mv" | "rm" | "touch" | "install" | "tee" | "pytest"
                | "make" => true,
                "cargo" | "npm" | "pnpm" | "yarn" | "git" => {
                    let subcommand = if tool_name(item) == "terminal_exec" {
                        args.pointer("/args/0")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    } else {
                        command.split_whitespace().nth(1).unwrap_or("")
                    };
                    matches!(
                        subcommand,
                        "test"
                            | "build"
                            | "check"
                            | "clippy"
                            | "run"
                            | "install"
                            | "add"
                            | "commit"
                            | "push"
                            | "apply"
                            | "checkout"
                            | "switch"
                            | "merge"
                            | "rebase"
                    )
                }
                _ => false,
            }
        }
        "github_api" => item
            .pointer("/arguments/method")
            .or_else(|| item.pointer("/args/method"))
            .and_then(Value::as_str)
            .map(|s| !matches!(s, "GET" | "HEAD" | "OPTIONS"))
            .unwrap_or(false),
        // Unknown remote tools need explicit criteria rather than guessing
        // side effects from a name supplied by an external server.
        _ => false,
    }
}
pub(crate) fn command_matches(item: &Value, command: &str) -> bool {
    if !matches!(tool_name(item), "shell_exec" | "terminal_exec") || !successful_evidence(item) {
        return false;
    }
    let args = item
        .get("arguments")
        .or_else(|| item.get("args"))
        .unwrap_or(&Value::Null);
    if tool_name(item) == "shell_exec" {
        return args.get("command").and_then(Value::as_str).map(str::trim) == Some(command.trim());
    }
    let mut actual = string(args, "command");
    if let Some(argv) = args.get("args").and_then(Value::as_array) {
        for arg in argv.iter().filter_map(Value::as_str) {
            actual.push(' ');
            actual.push_str(arg);
        }
    }
    actual.trim() == command.trim()
}
/// A test command verifies the source state AT THAT POINT, not a future
/// modified state. Reusing a passing test from before the last source write is
/// a false positive on multi-turn autonomous coding tasks.
fn source_mutation(item: &Value) -> bool {
    if !successful_evidence(item) { return false; }
    let args=item.get("arguments").or_else(||item.get("args")).unwrap_or(&Value::Null);
    match tool_name(item) {
        "workspace_write" | "workspace_replace" | "workspace_replace_range"
        | "workspace_replace_function" | "file_write" => true,
        "shell_exec" | "terminal_exec" => {
            let command=string(args,"command").to_lowercase();
            let argv=args["args"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).collect::<Vec<_>>().join(" ").to_lowercase();
            let full=if argv.is_empty(){command.clone()}else{format!("{command} {argv}")};
            // Direct source-mutation shell commands are not file_write tools.
            // Keep this list conservative; avoid classifying test/build commands
            // as writes merely because they execute a compiler.
            full.starts_with("git apply ")
                || full.starts_with("cargo fmt")
                || full.starts_with("go fmt")
                || full.starts_with("gofmt -w")
                || full.starts_with("rustfmt ")
                || full.starts_with("sed -i")
                || full.starts_with("perl -pi")
                || full.starts_with("npm run format")
                || full.starts_with("pnpm format")
                || full.starts_with("yarn format")
                || full.starts_with("tee ")
                || full.contains(" > ")
        }
        _ => false,
    }
}

/// Determines if a successful later edit invalidates a prior test receipt.
/// opSeq is authoritative inside one execution. RFC3339 timestamps order
/// receipts across successive goal iterations/executions. Missing chronology
/// fails closed: an unsequenced write cannot validate an older test.
fn test_stale_after_write(test: &Value, write: &Value) -> bool {
    let same_run=test["runId"].as_str().is_some_and(|id|
        !id.is_empty() && write["runId"].as_str()==Some(id));
    if same_run {
        if let (Some(check),Some(edit))=(test["opSeq"].as_u64(),write["opSeq"].as_u64()){
            return edit>check;
        }
    }
    let check_time=test["startedAt"].as_str().or_else(||test["finishedAt"].as_str())
        .and_then(|time|chrono::DateTime::parse_from_rfc3339(time).ok());
    let write_time=write["finishedAt"].as_str().or_else(||write["startedAt"].as_str())
        .and_then(|time|chrono::DateTime::parse_from_rfc3339(time).ok());
    match (check_time,write_time) {
        (Some(check),Some(edit)) => edit>=check,
        // A later unsequenced edit cannot safely preserve the test's guarantee.
        _ => true,
    }
}

pub(crate) fn fresh_command_matches(evidence: &[Value], command: &str) -> bool {
    evidence.iter().filter(|item|command_matches(item,command)).any(|test| {
        !evidence.iter().filter(|item|source_mutation(item))
            .any(|write| test_stale_after_write(test,write))
    })
}

pub(crate) fn acceptance_gaps(
    state: &AppState,
    goal: &str,
    checks: &Value,
    evidence: &[Value],
) -> Vec<String> {
    let checks = checks.as_array().cloned().unwrap_or_default();
    let mut gaps = Vec::new();
    let has_explicit_tool = checks.iter().any(|check| {
        matches!(
            check["type"].as_str(),
            Some("tool_succeeded" | "command_succeeded")
        )
    });
    if looks_actionable(goal) && !evidence.iter().any(is_action_evidence) && !has_explicit_tool {
        gaps.push("변경 또는 실행을 입증하는 성공한 도구 결과가 없습니다. 조회 결과만으로 행동 목표를 완료할 수 없습니다.".into());
    }
    for check in checks {
        let kind = check["type"].as_str().unwrap_or("");
        let path = check["path"].as_str().unwrap_or("");
        let resolved = if kind.starts_with("file_") {
            match resolve_path(state, path) {
                Ok(path) => path,
                Err(error) => {
                    gaps.push(error);
                    continue;
                }
            }
        } else {
            state.config.workspace.clone()
        };
        let passed = match kind {
            "file_exists" => resolved.is_file(),
            "file_absent" => std::fs::symlink_metadata(&resolved)
                .map(|_| false)
                .unwrap_or_else(|e| e.kind() == std::io::ErrorKind::NotFound),
            "file_changed" => resolved.is_file() && fingerprint(&resolved) != check["baseline"],
            "file_replace_exact" => check["expectedSha256"].as_str().is_some_and(|expected| {
                read_bounded_file(&resolved)
                    .is_some_and(|bytes| format!("{:x}", Sha256::digest(bytes)) == expected)
            }),
            "file_equals" => check["content"].as_str().is_some_and(|expected| {
                (check["inferred"] != true || fingerprint(&resolved) != check["baseline"])
                    && read_bounded_file(&resolved).is_some_and(|bytes| bytes == expected.as_bytes())
            }),
            "file_contains" => {
                std::fs::metadata(&resolved)
                    .map(|meta| meta.is_file() && meta.len() <= 8 * 1024 * 1024)
                    .unwrap_or(false)
                    && std::fs::read_to_string(&resolved)
                        .map(|text| text.contains(check["contains"].as_str().unwrap_or("")))
                        .unwrap_or(false)
            }
            "tool_succeeded" => evidence.iter().any(|item| {
                successful_evidence(item) && tool_name(item) == check["tool"].as_str().unwrap_or("")
            }),
            "command_succeeded" => fresh_command_matches(
                evidence, check["command"].as_str().unwrap_or(""),
            ),
            _ => false,
        };
        if !passed {
            if kind == "command_succeeded"
                && evidence.iter().any(|item|command_matches(item,
                    check["command"].as_str().unwrap_or("")))
            {
                gaps.push(format!(
                    "검증 명령 '{}'의 이전 성공 후 코드·파일이 다시 변경되었습니다. 최신 소스에서 같은 검증을 다시 실행하세요.",
                    check["command"].as_str().unwrap_or("")
                ));
                continue;
            }
            if kind == "file_replace_exact" {
                gaps.push(format!("The exact replacement in {path} has not been verified. Preserve every other byte of the original file."));
                continue;
            }
            gaps.push(format!(
                "검증 조건 미충족: {}",
                serde_json::to_string(&check).unwrap_or_default()
            ));
        }
    }
    gaps
}


#[cfg(test)]
mod inference_tests {
    use super::*;

    #[test]
    fn reading_an_input_does_not_require_it_to_change() {
        let checks = infer_criteria("a.txt를 읽고 b.txt에 결과를 저장해");
        assert_eq!(checks.len(), 1, "{checks:?}");
        assert_eq!(checks[0]["path"], "b.txt");
        assert_eq!(checks[0]["type"], "file_changed");
    }


    #[test]
    fn incidental_substrings_do_not_turn_research_into_mutation() {
        for phrase in [
            "compare the latest brunch options",
            "preview a tutorial about runner designs",
            "summarize the prerun log",
        ] {
            assert!(!looks_actionable(phrase), "false action: {phrase}");
            assert!(infer_criteria(phrase).is_empty());
        }
        for phrase in ["implement src/lib.rs", "fix src/lib.rs",
            "refactor src/lib.rs", "수정해줘 src/lib.rs"] {
            assert!(looks_actionable(phrase), "missed action: {phrase}");
            assert!(!infer_criteria(phrase).is_empty());
        }
    }

    #[test]
    fn surgical_source_edits_are_actual_action_receipts() {
        for tool in ["workspace_replace","workspace_replace_range",
            "workspace_replace_function","workspace_write"] {
            let item=json!({"tool":tool,"status":"completed",
                "arguments":{"path":"src/lib.rs","old":"a","new":"b"},
                "result":{"written":true}});
            assert!(is_action_evidence(&item),"{tool} must count as source edit");
        }
        assert!(!is_action_evidence(&json!({
            "tool":"workspace_replace_range","status":"failed",
            "result":{"success":false}
        })));
        assert!(!is_action_evidence(&json!({
            "tool":"workspace_replace_function","status":"completed",
            "arguments":{"path":"src/lib.rs"},
            "result":{"written":false}
        })));
    }

    #[test]
    fn later_source_edits_invalidate_previous_successful_tests() {
        let checked=json!({"type":"command_succeeded","command":"cargo test"});
        let cmd=|seq:u64|json!({
            "tool":"shell_exec","runId":"run-1","opSeq":seq,
            "arguments":{"command":"cargo test"},
            "result":{"exitCode":0},
            "startedAt":"2026-11-14T10:01:00Z",
            "finishedAt":"2026-11-14T10:01:10Z"
        });
        let write=|seq:u64|json!({
            "tool":"workspace_replace_range","runId":"run-1","opSeq":seq,
            "arguments":{"path":"src/lib.rs"},"result":{"written":true},
            "startedAt":"2026-11-14T10:01:01Z",
            "finishedAt":"2026-11-14T10:01:02Z"
        });
        assert!(fresh_command_matches(&[write(1),cmd(2)],"cargo test"));
        assert!(!fresh_command_matches(&[cmd(1),write(2)],"cargo test"));
        assert!(fresh_command_matches(&[cmd(1),write(2),cmd(3)],"cargo test"));
        assert!(fresh_command_matches(&[cmd(1),json!({
            "tool":"workspace_read","runId":"run-1","opSeq":2,
            "result":{"content":"read-only"}
        })],"cargo test"));
        assert!(fresh_command_matches(&[cmd(1),json!({
            "tool":"workspace_write","runId":"run-1","opSeq":2,
            "result":{"success":false}
        })],"cargo test"));
        assert_eq!(checked["command"],"cargo test");
    }

    #[test]
    fn multi_iteration_tests_require_a_new_check_after_newer_source_write() {
        let (state,root)={
            let root=std::env::temp_dir().join(format!("goal-stale-check-{}",uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let config=crate::config::Config{
                port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()
            };
            let state=crate::state::AppState::new(config.clone(),
                crate::db::Store::open(&config.db_path()).unwrap(),
                crate::vault::Vault::open(&config.key_path()));
            (state,root)
        };
        let test=json!({
            "tool":"shell_exec","runId":"old-run","opSeq":3,
            "arguments":{"command":"cargo test"},"result":{"exitCode":0},
            "startedAt":"2026-11-14T10:00:00Z","finishedAt":"2026-11-14T10:01:00Z"
        });
        let later_edit=json!({
            "tool":"workspace_write","runId":"new-run","opSeq":1,
            "arguments":{"path":"src/lib.rs"},"result":{"written":true},
            "startedAt":"2026-11-14T10:02:00Z","finishedAt":"2026-11-14T10:02:01Z"
        });
        let criteria=json!([{"type":"command_succeeded","command":"cargo test"}]);
        let gaps=acceptance_gaps(&state,"run tests",&criteria,&[test.clone(),later_edit.clone()]);
        assert_eq!(gaps.len(),1,"{gaps:?}");
        assert!(gaps[0].contains("다시 실행"),"{gaps:?}");
        let new_test=json!({
            "tool":"terminal_exec","runId":"third-run","opSeq":1,
            "arguments":{"command":"cargo","args":["test"]},
            "result":{"exitCode":0},
            "startedAt":"2026-11-14T10:03:00Z","finishedAt":"2026-11-14T10:03:20Z"
        });
        assert!(acceptance_gaps(&state,"run tests",&criteria,
            &[test,later_edit,new_test]).is_empty());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn unsequenced_writes_fail_closed_but_failed_writes_do_not() {
        let ok=json!({"tool":"shell_exec","arguments":{"command":"npm test"},
            "result":{"exitCode":0}});
        let changed=json!({"tool":"workspace_write","result":{"written":true}});
        assert!(!fresh_command_matches(&[ok.clone(),changed.clone()],"npm test"));
        let failed=json!({"tool":"workspace_write","result":{"error":"permission denied"}});
        assert!(fresh_command_matches(&[ok,failed],"npm test"));
    }

    #[test]
    fn multiple_outputs_are_still_inferred_without_a_read_verb() {
        let checks = infer_criteria("a.txt와 b.txt를 만들어줘");
        assert_eq!(checks.len(), 2, "{checks:?}");
    }
}
