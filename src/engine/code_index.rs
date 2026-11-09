//! Durable, deterministic repository inspection.
//!
//! The expensive part of self-improvement was not reading code once; it was
//! making a model rediscover the same repository on every attempt. This module
//! reads every source file once, stores a content hash, symbols, tests and
//! static findings, and reuses unchanged files on later inspections. RSI then
//! receives a compact file map and the full context of the likely targets
//! instead of spending hundreds of thousands of input tokens listing the tree.
use crate::{engine::memory, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
    time::Instant,
};

const INDEX_KIND: &str = "code_index";
const FILE_KIND: &str = "code_index_file";
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SYMBOLS_PER_FILE: usize = 400;
const MAP_MAX_CHARS: usize = 48_000;
const FOCUS_MAX_CHARS: usize = 18_000;
const PARSER_VERSION: u64 = 3;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn index_id(source: &Path) -> String {
    format!(
        "code-index-{}",
        memory::stable_id(&[&source.to_string_lossy()])
    )
}

fn is_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if name.ends_with(".min.js") || name.ends_with(".lock") || name.starts_with('.') {
        return false;
    }
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some(
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "mjs"
                | "cjs"
                | "css"
                | "html"
                | "json"
                | "toml"
                | "md"
                | "sh"
                | "yml"
                | "yaml"
        )
    )
}

fn collect_files(source: &Path) -> Vec<PathBuf> {
    let mut roots = vec![source.join("src"), source.join("tests")];
    for extra in ["web/src", "src-tauri/src", "scripts", ".github/workflows"] {
        let path = source.join(extra);
        if path.is_dir() {
            roots.push(path);
        }
    }
    for file in ["Cargo.toml", "web/package.json", "README.md"] {
        let path = source.join(file);
        if path.is_file() {
            roots.push(path);
        }
    }
    let mut files = Vec::new();
    let mut stack = roots;
    while let Some(path) = stack.pop() {
        if path.is_file() {
            if is_source(&path) {
                files.push(path);
            }
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let next = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if matches!(
                    name.as_str(),
                    ".git" | "target" | "node_modules" | "dist" | "runtime" | "tmp"
                ) {
                    continue;
                }
                stack.push(next);
            } else if kind.is_file() && is_source(&next) {
                files.push(next);
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn symbol_name(rest: &str) -> String {
    rest.chars()
        .take_while(|ch| ch.is_alphanumeric() || matches!(ch, '_' | ':' | '<' | '>'))
        .collect::<String>()
        .trim_end_matches(['<', '>'])
        .to_string()
}

fn rust_symbol(line: &str) -> Option<(&'static str, String)> {
    let mut rest = line;
    for prefix in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(next) = rest.strip_prefix(prefix) {
            rest = next;
        }
    }
    for prefix in ["async ", "unsafe ", "extern \"C\" "] {
        if let Some(next) = rest.strip_prefix(prefix) {
            rest = next;
        }
    }
    if let Some(next) = rest.strip_prefix("const ") {
        if let Some(function) = next.strip_prefix("fn ") {
            return Some(("fn", symbol_name(function)));
        }
        return Some(("const", symbol_name(next)));
    }
    for (prefix, kind) in [
        ("fn ", "fn"),
        ("struct ", "struct"),
        ("enum ", "enum"),
        ("trait ", "trait"),
        ("mod ", "mod"),
        ("type ", "type"),
        ("macro_rules! ", "macro"),
    ] {
        if let Some(next) = rest.strip_prefix(prefix) {
            let name = symbol_name(next);
            if !name.is_empty() {
                return Some((kind, name));
            }
        }
    }
    None
}

fn script_symbol(line: &str) -> Option<(&'static str, String)> {
    let mut rest = line;
    for prefix in ["export default ", "export ", "declare "] {
        if let Some(next) = rest.strip_prefix(prefix) {
            rest = next;
        }
    }
    for prefix in ["async "] {
        if let Some(next) = rest.strip_prefix(prefix) {
            rest = next;
        }
    }
    for (prefix, kind) in [
        ("function ", "fn"),
        ("class ", "class"),
        ("interface ", "interface"),
        ("type ", "type"),
        ("const ", "const"),
        ("let ", "const"),
    ] {
        if let Some(next) = rest.strip_prefix(prefix) {
            let name = symbol_name(next);
            if !name.is_empty() {
                return Some((kind, name));
            }
        }
    }
    None
}

fn quotes_name(line: &str, marker: &str) -> Option<String> {
    let rest = line.split_once(marker)?.1.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' && quote != '`' {
        return None;
    }
    let name = rest[1..].split(quote).next()?.trim();
    (!name.is_empty()).then(|| name.chars().take(180).collect())
}

fn comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut quote = false;
    let mut escaped = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let ch = bytes[index] as char;
        if quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' || ch == '`' {
                quote = false;
            }
        } else if ch == '"' || ch == '`' {
            quote = true;
        } else if ch == '/' && bytes[index + 1] == b'/' {
            return Some(index + 2);
        } else if ch == '/' && bytes[index + 1] == b'*' {
            return Some(index + 2);
        }
        index += 1;
    }
    None
}

fn code_only(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut quote = false;
    let mut escaped = false;
    for ch in line.chars() {
        if quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' || ch == '`' {
                quote = false;
            }
            output.push(' ');
        } else if ch == '"' || ch == '`' {
            quote = true;
            output.push(' ');
        } else {
            output.push(ch);
        }
    }
    output
}

fn parse_file(rel: &str, content: &str) -> Value {
    let language = rel
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or("text")
        .to_string();
    let mut symbols = Vec::new();
    let mut tests = Vec::new();
    let mut todos = Vec::new();
    let mut summary = Vec::new();
    let mut patterns = BTreeMap::<&'static str, u64>::new();
    let mut test_pending = false;
    let mut in_tests = rel.starts_with("tests/")
        || rel.contains("/tests/")
        || rel.ends_with("/tests.rs")
        || rel.contains(".test.")
        || rel.contains(".spec.")
        || rel.ends_with("_test.rs")
        || rel.contains("_fixture");
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("//!") || trimmed.starts_with("///") || trimmed.starts_with("//"))
            && summary.len() < 3
        {
            let text = trimmed
                .trim_start_matches("//!")
                .trim_start_matches("///")
                .trim_start_matches("//")
                .trim();
            if !text.is_empty() {
                summary.push(text.chars().take(240).collect::<String>());
            }
        }
        if trimmed.starts_with("#[cfg(") && trimmed.contains("test") {
            in_tests = true;
        }
        if trimmed.starts_with("#[test")
            || trimmed.starts_with("#[tokio::test")
            || trimmed.starts_with("#[async_std::test")
        {
            test_pending = true;
        }
        let symbol = if matches!(language.as_str(), "rs") {
            rust_symbol(trimmed)
        } else if matches!(language.as_str(), "ts" | "tsx" | "js" | "jsx") {
            script_symbol(trimmed)
        } else {
            None
        };
        if let Some((kind, name)) = symbol {
            if test_pending && kind == "fn" {
                tests.push(name.clone());
                test_pending = false;
            }
            if symbols.len() < MAX_SYMBOLS_PER_FILE {
                symbols.push(json!({"kind":kind,"name":name,"line":index + 1}));
            }
        }
        if matches!(language.as_str(), "ts" | "tsx" | "js" | "jsx") {
            for marker in ["it(", "test(", "describe("] {
                if trimmed.contains(marker) {
                    if let Some(name) = quotes_name(trimmed, marker) {
                        tests.push(name);
                    }
                }
            }
        }
        let comment = comment_start(line)
            .map(|position| &line[position..])
            .or_else(|| {
                let trimmed = line.trim_start();
                trimmed
                    .strip_prefix('*')
                    .or_else(|| trimmed.strip_prefix("# "))
            });
        if let Some(comment) = comment {
            let upper = comment.to_uppercase();
            for marker in ["TODO", "FIXME", "HACK", "XXX"] {
                if upper.contains(marker) {
                    todos.push(json!({"line":index + 1,"text":comment.trim().chars().take(220).collect::<String>()}));
                    break;
                }
            }
        }
        if in_tests {
            continue;
        }
        let code = comment_start(line)
            .map(|position| &line[..position])
            .unwrap_or(line);
        let code = code_only(code);
        for (key, needle) in [
            ("unwrap", ".unwrap()"),
            ("expect", ".expect("),
            ("panic", "panic!("),
            ("unsafe", "unsafe "),
            ("unimplemented", "unimplemented!("),
            ("todo_macro", "todo!("),
            ("dbg", "dbg!("),
            ("allow_dead_code", "allow(dead_code)"),
        ] {
            let count = code.matches(needle).count() as u64;
            if count > 0 {
                *patterns.entry(key).or_insert(0) += count;
            }
        }
    }
    tests.sort();
    tests.dedup();
    json!({
        "path": rel,
        "language": language,
        "lines": content.lines().count(),
        "bytes": content.len(),
        "symbols": symbols,
        "symbolsTruncated": symbols.len() >= MAX_SYMBOLS_PER_FILE,
        "tests": tests,
        "todos": todos,
        "patterns": patterns,
        "summary": summary,
    })
}

fn module_of(path: &str) -> String {
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["src", "engine", ..] => "src/engine".into(),
        ["src", "api", ..] => "src/api".into(),
        ["src", ..] => "src".into(),
        ["web", "src", ..] => "web/src".into(),
        ["tests", ..] => "tests".into(),
        _ => parts.first().copied().unwrap_or("root").to_string(),
    }
}

fn hotspot_reasons(file: &Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(todos) = file["todos"].as_array().filter(|rows| !rows.is_empty()) {
        reasons.push(format!("TODO/FIXME {}", todos.len()));
    }
    let patterns = &file["patterns"];
    let unwraps = patterns["unwrap"].as_u64().unwrap_or(0);
    if unwraps >= 8 {
        reasons.push(format!("unwrap {unwraps}"));
    }
    if patterns["panic"].as_u64().unwrap_or(0) > 0 {
        reasons.push(format!("panic {}", patterns["panic"]));
    }
    if patterns["unsafe"].as_u64().unwrap_or(0) > 0 {
        reasons.push(format!("unsafe {}", patterns["unsafe"]));
    }
    if file["lines"].as_u64().unwrap_or(0) >= 1_200 {
        reasons.push(format!("큰 파일 {}줄", file["lines"]));
    }
    reasons
}

fn hotspot_score(file: &Value) -> u64 {
    let todos = file["todos"].as_array().map(|rows| rows.len()).unwrap_or(0) as u64;
    let patterns = &file["patterns"];
    todos * 40
        + patterns["unwrap"].as_u64().unwrap_or(0) * 2
        + patterns["panic"].as_u64().unwrap_or(0) * 20
        + patterns["unsafe"].as_u64().unwrap_or(0) * 30
        + file["lines"].as_u64().unwrap_or(0) / 100
}

fn file_id(index_id: &str, rel: &str) -> String {
    format!("{index_id}:{rel}")
}

fn index_files(state: &AppState, index_id: &str) -> Vec<Value> {
    state
        .store
        .all(FILE_KIND)
        .into_iter()
        .filter(|file| file["indexId"].as_str() == Some(index_id))
        .collect()
}

/// Read every source file, reuse unchanged hashes, and store the index.
pub fn refresh(state: &AppState, source: &Path) -> Result<Value, String> {
    let started = Instant::now();
    let source = source
        .canonicalize()
        .map_err(|_| "연결된 소스 폴더에 접근할 수 없습니다.".to_string())?;
    let id = index_id(&source);
    let existing = index_files(state, &id)
        .into_iter()
        .filter_map(|file| {
            let path = file["path"].as_str()?.to_string();
            Some((path, file))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    let mut changed = 0u64;
    let mut reused = 0u64;
    let mut removed = 0u64;
    for path in collect_files(&source) {
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&source) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !seen.insert(relative.clone()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let hash = digest(&bytes);
        let mut was_changed = false;
        let mut file = if let Some(previous) = existing.get(&relative) {
            if previous["hash"].as_str() == Some(hash.as_str())
                && previous["parserVersion"].as_u64() == Some(PARSER_VERSION)
            {
                reused += 1;
                previous.clone()
            } else {
                changed += 1;
                was_changed = true;
                let content = String::from_utf8_lossy(&bytes);
                let mut parsed = parse_file(&relative, &content);
                parsed["hash"] = json!(hash);
                parsed["indexId"] = json!(id);
                parsed["parserVersion"] = json!(PARSER_VERSION);
                parsed
            }
        } else {
            changed += 1;
            was_changed = true;
            let content = String::from_utf8_lossy(&bytes);
            let mut parsed = parse_file(&relative, &content);
            parsed["hash"] = json!(hash);
            parsed["indexId"] = json!(id);
            parsed["parserVersion"] = json!(PARSER_VERSION);
            parsed
        };
        file["id"] = json!(file_id(&id, &relative));
        if was_changed {
            state.store.put(FILE_KIND, &file);
        }
        files.push(file);
    }
    for path in existing.keys() {
        if !seen.contains(path) {
            state.store.remove(FILE_KIND, &file_id(&id, path));
            removed += 1;
        }
    }
    files.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let mut modules: BTreeMap<String, (u64, u64, u64, u64)> = BTreeMap::new();
    let mut symbol_total = 0u64;
    let mut test_total = 0u64;
    let mut todo_total = 0u64;
    let mut hotspots = Vec::new();
    for file in &files {
        let path = file["path"].as_str().unwrap_or("");
        if path.starts_with("tests/") {
            continue;
        }
        let lines = file["lines"].as_u64().unwrap_or(0);
        let symbols = file["symbols"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0) as u64;
        let tests = file["tests"].as_array().map(|rows| rows.len()).unwrap_or(0) as u64;
        let todos = file["todos"].as_array().map(|rows| rows.len()).unwrap_or(0) as u64;
        symbol_total += symbols;
        test_total += tests;
        todo_total += todos;
        let entry = modules.entry(module_of(path)).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        entry.1 += lines;
        entry.2 += symbols;
        entry.3 += tests;
        let reasons = hotspot_reasons(file);
        if !reasons.is_empty() {
            hotspots.push(json!({
                "path":path,
                "lines":lines,
                "symbols":symbols,
                "tests":tests,
                "reasons":reasons,
                "score":hotspot_score(file),
            }));
        }
    }
    hotspots.sort_by(|left, right| {
        right["score"]
            .as_u64()
            .cmp(&left["score"].as_u64())
            .then_with(|| left["path"].as_str().cmp(&right["path"].as_str()))
    });
    hotspots.truncate(24);
    let module_rows = modules
        .into_iter()
        .map(|(module, (files, lines, symbols, tests))| {
            json!({"module":module,"files":files,"lines":lines,"symbols":symbols,"tests":tests})
        })
        .collect::<Vec<_>>();
    let fingerprint_parts = files
        .iter()
        .map(|file| {
            format!(
                "{}:{}",
                file["path"].as_str().unwrap_or(""),
                file["hash"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>();
    let fingerprint_refs = fingerprint_parts
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let fingerprint = memory::stable_id(&fingerprint_refs);
    let meta = json!({
        "id": id,
        "sourceDir": source.to_string_lossy(),
        "fingerprint": fingerprint,
        "files": files.len(),
        "totalLines": files.iter().map(|file| file["lines"].as_u64().unwrap_or(0)).sum::<u64>(),
        "totalBytes": files.iter().map(|file| file["bytes"].as_u64().unwrap_or(0)).sum::<u64>(),
        "symbols": symbol_total,
        "tests": test_total,
        "todos": todo_total,
        "changedFiles": changed,
        "reusedFiles": reused,
        "removedFiles": removed,
        "modules": module_rows,
        "hotspots": hotspots,
        "durationMs": started.elapsed().as_millis() as u64,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
    });
    state
        .store
        .put(INDEX_KIND, &meta)
        .ok_or("전수조사 인덱스 저장 실패")?;
    state.store.audit(
        "rsi.index.refreshed",
        json!({"indexId":meta["id"],"files":meta["files"],"changed":changed,"reused":reused}),
    );
    Ok(meta)
}

pub fn get(state: &AppState, source: &Path) -> Option<Value> {
    let source = source.canonicalize().ok()?;
    state.store.get(INDEX_KIND, &index_id(&source))
}

fn excerpt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut output = String::new();
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("pub struct ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("export const ")
            || trimmed.starts_with("TODO")
            || trimmed.starts_with("FIXME")
        {
            in_code = true;
        }
        if in_code {
            output.push_str(line);
            output.push('\n');
        } else if output.is_empty() && trimmed.starts_with("//!") {
            output.push_str(line);
            output.push('\n');
        }
        if output.chars().count() >= max_chars {
            output.push_str("... (이하 생략)");
            break;
        }
    }
    if output.is_empty() {
        output = text.chars().take(max_chars).collect();
        output.push_str("\n... (이하 생략)");
    }
    output
}

/// Build the reusable inspection block embedded in the RSI prompt. The map
/// contains every source path and its symbols; full content is included only
/// for the selected focus files.
pub fn prompt_context(
    state: &AppState,
    source: &Path,
    focus: &[String],
) -> Result<(Value, String), String> {
    let meta = refresh(state, source)?;
    let source = source
        .canonicalize()
        .map_err(|_| "연결된 소스 폴더에 접근할 수 없습니다.".to_string())?;
    let id = index_id(&source);
    let files = index_files(state, &id);
    let mut block = String::new();
    writeln!(block, "## 저장소 전수조사 인덱스 (재사용됨)").ok();
    writeln!(
        block,
        "- 파일 {}개 / {}줄 / 심볼 {}개 / 테스트 {}개 / TODO·FIXME {}개",
        meta["files"], meta["totalLines"], meta["symbols"], meta["tests"], meta["todos"]
    )
    .ok();
    writeln!(
        block,
        "- 이번 갱신: 변경 {}개, 그대로 재사용 {}개, 삭제 {}개 / {}ms",
        meta["changedFiles"],
        meta["reusedFiles"],
        meta["removedFiles"],
        meta["durationMs"].as_u64().unwrap_or(0)
    )
    .ok();
    writeln!(
        block,
        "- 인덱스 지문: {}",
        meta["fingerprint"].as_str().unwrap_or("")
    )
    .ok();
    writeln!(block, "\n### 모듈 지도").ok();
    for module in meta["modules"].as_array().into_iter().flatten() {
        writeln!(
            block,
            "- {}: 파일 {}, {}줄, 심볼 {}, 테스트 {}",
            module["module"].as_str().unwrap_or(""),
            module["files"],
            module["lines"],
            module["symbols"],
            module["tests"]
        )
        .ok();
    }
    writeln!(block, "\n### 파일 지도 (경로: 줄 수 · 심볼)").ok();
    let mut map_chars = 0usize;
    for file in &files {
        let path = file["path"].as_str().unwrap_or("");
        let symbols = file["symbols"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|symbol| symbol["name"].as_str())
            .take(14)
            .collect::<Vec<_>>()
            .join(", ");
        let tests = file["tests"].as_array().map(|rows| rows.len()).unwrap_or(0);
        let line = format!(
            "- {path} ({}줄, 테스트 {tests}): {symbols}\n",
            file["lines"].as_u64().unwrap_or(0)
        );
        if map_chars + line.chars().count() > MAP_MAX_CHARS {
            block.push_str("- ... (파일 지도 나머지 생략)\n");
            break;
        }
        map_chars += line.chars().count();
        block.push_str(&line);
    }
    writeln!(block, "\n### 정적 발견 우선순위").ok();
    for hotspot in meta["hotspots"].as_array().into_iter().flatten() {
        writeln!(
            block,
            "- {}: {}",
            hotspot["path"].as_str().unwrap_or(""),
            hotspot["reasons"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|reason| reason.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
        .ok();
    }
    let mut selected = focus
        .iter()
        .filter(|path| crate::engine::rsi::safe_path(path))
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        selected = meta["hotspots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|hotspot| hotspot["path"].as_str().map(str::to_string))
            .take(2)
            .collect();
    }
    if !selected.is_empty() {
        writeln!(
            block,
            "\n### 우선 대상 파일의 실제 코드 (인덱스에서 선택됨. 전체 저장소를 다시 읽지 마세요)"
        )
        .ok();
        for relative in selected.into_iter().take(4) {
            if !crate::engine::rsi::safe_path(&relative) {
                continue;
            }
            let path = source.join(&relative);
            let Ok(canonical) = path.canonicalize() else {
                continue;
            };
            if !canonical.starts_with(&source) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&canonical) else {
                continue;
            };
            writeln!(
                block,
                "\n#### {relative} ({}줄)\n```rust\n{}\n```",
                text.lines().count(),
                excerpt(&text, FOCUS_MAX_CHARS)
            )
            .ok();
        }
    }
    writeln!(
        block,
        "\n### 재사용 규칙\n- 위 인덱스는 이미 전체 저장소를 읽어 만든 기억입니다. `workspace_list`로 처음부터 다시 탐색하지 마세요.\n- 대상 파일을 고를 때는 위 파일 지도와 정적 발견을 사용하세요.\n- 필요한 파일이 있으면 그 파일만 `workspace_read`로 확인하세요.\n- 이번 실행은 전수조사 결과를 재사용하는 **1회 수정**입니다."
    )
    .ok();
    Ok((meta, block))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    fn temp_state() -> (AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-index-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: data.clone(),
            web_dir: data.join("web"),
            workspace: data.clone(),
        };
        let state = AppState::new(
            cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()),
        );
        (state, root)
    }

    #[test]
    fn a_full_inspection_is_reused_and_only_reparses_changed_files() {
        let (state, root) = temp_state();
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(
            source.join("src/engine/rsi_evolve.rs"),
            "//! rsi\npub fn run_loop() {\n    // TODO: remove the cap\n}\n#[test]\nfn a_test() {}\n",
        )
        .unwrap();
        let first = refresh(&state, &source).unwrap();
        assert_eq!(first["files"], 1);
        assert_eq!(first["symbols"], 2);
        assert_eq!(first["tests"], 1);
        assert_eq!(first["todos"], 1);
        assert_eq!(first["changedFiles"], 1);
        let second = refresh(&state, &source).unwrap();
        assert_eq!(second["changedFiles"], 0);
        assert_eq!(second["reusedFiles"], 1);
        std::fs::write(
            source.join("src/engine/rsi_evolve.rs"),
            "pub fn run_loop() {}\n",
        )
        .unwrap();
        let third = refresh(&state, &source).unwrap();
        assert_eq!(third["changedFiles"], 1);
        assert_eq!(third["todos"], 0);
        let (_, block) = prompt_context(&state, &source, &[]).unwrap();
        assert!(block.contains("src/engine/rsi_evolve.rs"));
        assert!(block.contains("run_loop"));
        let _ = std::fs::remove_dir_all(root);
    }
}
