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
pub(crate) fn looks_actionable(goal: &str) -> bool {
    [
        "만들",
        "생성",
        "저장",
        "작성",
        "수정",
        "편집",
        "삭제",
        "제거",
        "실행",
        "설치",
        "복사",
        "이동",
        "다운로드",
        "빌드",
        "커밋",
        "배포",
        "create",
        "write",
        "save",
        "edit",
        "delete",
        "remove",
        "run",
        "install",
        "build",
        "update",
        "deploy",
        "commit",
    ]
    .iter()
    .any(|v| goal.to_lowercase().contains(v))
}
pub(crate) fn infer_criteria(goal: &str) -> Vec<Value> {
    if !looks_actionable(goal) {
        return vec![];
    }
    let lower = goal.to_lowercase();
    let deletion = ["삭제", "제거", "delete", "remove"]
        .iter()
        .any(|v| lower.contains(v));
    let writing = [
        "만들", "생성", "저장", "작성", "수정", "편집", "create", "write", "save", "edit", "update",
    ]
    .iter()
    .any(|v| lower.contains(v));
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
    let reading = ["읽", "확인", "조회", "read", "check"]
        .iter()
        .any(|verb| lower.contains(verb));
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
/// Child operations are returned by DOMINION's own delegate_task handler
/// after checking parent-session ownership. A child's prose, queued state,
/// or failed tool is never counted as completed work.
fn delegated_successful_operations(item:&Value)->Vec<Value>{
    if tool_name(item)!="delegate_task" || !successful_evidence(item)
        || item["result"]["status"]!="completed" {return Vec::new();}
    item["result"]["operations"].as_array().into_iter().flatten()
        .filter(|operation|successful_evidence(operation))
        .take(20).cloned().collect()
}
pub(crate) fn evidence_has_tool(item:&Value,required:&str)->bool{
    (successful_evidence(item) && tool_name(item)==required)
        || delegated_successful_operations(item).iter()
            .any(|child|tool_name(child)==required)
}

pub(crate) fn is_action_evidence(item: &Value) -> bool {
    if !successful_evidence(item) {
        return false;
    }
    match tool_name(item) {
        "delegate_task" => delegated_successful_operations(item).iter().any(is_action_evidence),
        "workspace_write" | "file_write" => true,
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
    if tool_name(item)=="delegate_task" {
        return delegated_successful_operations(item).iter()
            .any(|child|command_matches(child,command));
    }
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
                evidence_has_tool(item,check["tool"].as_str().unwrap_or(""))
            }),
            "command_succeeded" => evidence
                .iter()
                .any(|item| command_matches(item, check["command"].as_str().unwrap_or(""))),
            _ => false,
        };
        if !passed {
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
    fn multiple_outputs_are_still_inferred_without_a_read_verb() {
        let checks = infer_criteria("a.txt와 b.txt를 만들어줘");
        assert_eq!(checks.len(), 2, "{checks:?}");
    }
}
