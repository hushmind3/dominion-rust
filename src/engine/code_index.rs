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
    path::{Path, PathBuf},
    time::Instant,
};

const INDEX_KIND: &str = "code_index";
const FILE_KIND: &str = "code_index_file";
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SYMBOLS_PER_FILE: usize = 400;
const MAP_MAX_CHARS: usize = 6_000;
const PARSER_VERSION: u64 = 4;

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
                | "py"
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

/// Discover editable Python architecture/training modules with the same
/// actual source SHA binding as Rust and TypeScript. Symbol evidence does not
/// execute or trust generated Python; the model must still edit real bytes.
fn python_symbol(line:&str)->Option<(&'static str,String)>{
    let stripped=line.strip_prefix("async ").unwrap_or(line);
    for (prefix,kind) in [("def ","fn"),("class ","class")] {
        if let Some(value)=stripped.strip_prefix(prefix){
            let name=symbol_name(value);
            if !name.is_empty(){return Some((kind,name));}
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

// Lexical targeting must not treat documentation comments as implementation.
// Preserve quoted literals: routes, SQL and prompts can identify opaque code.
fn source_without_comments(line: &str, in_block: &mut bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut quoted: Option<char> = None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if *in_block {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block = false;
            }
        } else if let Some(delimiter) = quoted {
            out.push(ch);
            if escaped { escaped = false; }
            else if ch == '\\' { escaped = true; }
            else if ch == delimiter { quoted = None; }
        } else if ch == '"' || ch == '`' {
            quoted = Some(ch);
            out.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            break;
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            *in_block = true;
        } else {
            out.push(ch);
        }
    }
    out
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
    // Compact lexical evidence allows target discovery even when file and symbol names are opaque.
    // Indexed once per source hash; never sent wholesale to the model.
    let mut body_terms = BTreeSet::<String>::new();
    let mut in_block_comment = false;
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
        } else if language=="py" {
            python_symbol(trimmed)
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
        // Include identifiers and string-literal vocabulary, not comments.
        let code = source_without_comments(line, &mut in_block_comment);
        for word in lexical_terms(&code) {
            if (3..=80).contains(&word.chars().count()) {
                body_terms.insert(word);
            }
        }
        let code = code_only(&code);
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
        "bodyLexicon": body_terms.into_iter().collect::<Vec<_>>().join(" "),
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

/// Keep the index in SQLite. Send a small navigation map, not the entire
/// index plus arbitrary hotspot source on every model/tool round.
pub fn prompt_context(
    state: &AppState,
    source: &Path,
    focus: &[String],
) -> Result<(Value, String), String> {
    let meta = refresh(state, source)?;
    let files = index_files(state, meta["id"].as_str().unwrap_or(""));
    let mut block = format!("## 저장된 코드 인덱스\n파일 {}개 / 심볼 {}개 / 테스트 {}개. 지문 {}\n",
        meta["files"], meta["symbols"], meta["tests"], meta["fingerprint"]);
    block.push_str("코드 원문은 DB의 위치 정보로 찾아 필요한 줄만 읽으세요. 아래 대상 심볼에 이미 있는 위치를 다시 검색할 필요는 없습니다.\n");
    let per_file=(MAP_MAX_CHARS.saturating_sub(block.chars().count()+400))/focus.len().clamp(1,8);
    for relative in focus.iter().take(8) {
        if crate::engine::rsi::hidden_rsi_oracle_path(relative) {continue;}
        let Some(file)=files.iter().find(|file|file["path"].as_str()==Some(relative)) else {continue};
        let mut line=format!("\n대상 {} ({}줄): ",relative,file["lines"]);
        for symbol in file["symbols"].as_array().into_iter().flatten()
            .filter(|symbol|!file["tests"].as_array().is_some_and(|tests|tests.contains(&symbol["name"]))) {
            let item=format!("{}:{}, ",symbol["name"].as_str().unwrap_or(""),symbol["line"]);
            if line.chars().count()+item.chars().count()>per_file {break;}
            line.push_str(&item);
        }
        block.push_str(&line);block.push('\n');
    }
    block.push_str("\n### 모듈 지도\n");
    for module in meta["modules"].as_array().into_iter().flatten() {
        let line=format!("- {}: 파일 {}, 심볼 {}\n",module["module"].as_str().unwrap_or(""),module["files"],module["symbols"]);
        if block.chars().count()+line.chars().count()>MAP_MAX_CHARS {break;}
        block.push_str(&line);
    }
    block.push_str("\n근거 있는 제품 능력을 개선하세요. 큰 파일·unwrap 수·TODO 개수만으로 수정 대상을 정하거나 수정 자체를 목표로 삼지 마세요. 적절한 후보가 없으면 변경 없이 이유를 보고하세요.\n");
    Ok((meta,block))
}


/// Exposes the exact indexed revision for evidence reuse across campaigns.
/// A prior unsuccessful target is never penalized after its source changes.
pub fn indexed_source_hash(state:&AppState,index_id:&str,path:&str)->Option<String>{
    state.store.get(FILE_KIND,&file_id(index_id,path))?["hash"]
        .as_str().map(str::to_string)
}

/// Split identifiers the way source actually names them: snake_case, camelCase
/// and punctuation boundaries. Substring matching on whole identifiers missed
/// "retrieveMemory" when the goal said "memory retrieve".
fn lexical_terms(text: &str) -> BTreeSet<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut terms = BTreeSet::new();
    let mut current = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                terms.insert(current.to_lowercase());
                current.clear();
            }
            continue;
        }
        let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(index + 1).copied();
        let camel = ch.is_uppercase()
            && (previous.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit())
                || (previous.is_some_and(|p| p.is_uppercase()) && next.is_some_and(char::is_lowercase)));
        if camel && !current.is_empty() {
            terms.insert(current.to_lowercase());
            current.clear();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        terms.insert(current.to_lowercase());
    }
    terms
}

/// Shared goal vocabulary for file navigation and source-span selection.
/// No source file names are embedded in the vocabulary.
fn focus_terms(objective:&str,domain:&str)->Vec<String> {
    let mut query = lexical_terms(objective).into_iter()
        .filter(|word| word.chars().count() >= 2)
        .collect::<Vec<_>>();
    // Language bridge is vocabulary, not a file selection table. Paths,
    // symbols and documentation decide which concrete implementation is read.
    let lower = objective.to_lowercase();
    for (word, aliases) in [
        ("기억", &["memory","recall","retrieve","retrieval"][..]),
        ("검색", &["search","rank","query","retrieval"][..]),
        ("망각", &["forget","decay","sweep"][..]),
        ("압축", &["compact","compress","compression"][..]),
        ("문맥", &["context","continuity","prompt"][..]),
        ("학습", &["learning","experience","distill"][..]),
        ("알고리즘", &["algorithm","strategy","policy"][..]),
        ("자기개선", &["rsi","evolve","improve"][..]),
        ("토큰", &["token","budget","usage"][..]),
        ("비용", &["cost","budget","usage"][..]),
        ("대화", &["chat","room","conversation"][..]),
        ("메신저", &["telegram","discord","channel","messenger"][..]),
        ("텔레그램", &["telegram"][..]),
        ("디스코드", &["discord"][..]),
        ("슬랙", &["slack"][..]),
        ("왓츠앱", &["whatsapp"][..]),
        ("모델", &["model","provider","inference"][..]),
        ("허깅페이스", &["huggingface","hugging_face"][..]),
        ("브라우저", &["browser"][..]),
        ("도구", &["tools","tool"][..]),
        ("화면", &["ui","page","component"][..]),
    ] {
        if lower.contains(word) {
            query.extend(aliases.iter().map(|word| word.to_string()));
        }
    }
    if !domain.is_empty() {query.push(domain.to_lowercase());}
    query.retain(|word| !matches!(word.as_str(),
        "개선"|"최적화"|"하기"|"함수"|"코드"|"the"|"and"|"for"|"with"|
        "improve"|"improvement"|"fix"|"code"|"function"|"dominion"));
    query.sort();
    query.dedup();
    query
}

/// Rank implementation files from the actual code index, not a fixed table of
/// paths. The ranking is deterministic, model-free, and remains bounded by the
/// indexed source snapshot. A model-authored targeting expression can replace
/// the scoring equation without replacing the Rust harness.
pub fn rank_focus_files(
    state: &AppState,
    index_id: &str,
    objective: &str,
    domain: &str,
    prior_unproductive: &BTreeMap<String, u64>,
    algorithm: Option<&Value>,
    limit: usize,
) -> Vec<Value> {
    rank_focus_files_with_feedback(state,index_id,objective,domain,
        prior_unproductive,&BTreeMap::new(),algorithm,limit)
}

/// Evidence from previously verified, source-revision-matched improvements
/// is a learned ranking feature. Merely producing a patch or text finding
/// never counts as a success, and the AI can change the scoring expression.
pub fn rank_focus_files_with_feedback(
    state: &AppState,
    index_id: &str,
    objective: &str,
    domain: &str,
    prior_unproductive: &BTreeMap<String, u64>,
    prior_productive: &BTreeMap<String, u64>,
    algorithm: Option<&Value>,
    limit: usize,
) -> Vec<Value> {
    let query=focus_terms(objective,domain);
    if query.is_empty() || limit == 0 {return Vec::new();}

    let files=index_files(state,index_id).into_iter()
        .filter(|file| {
            let path=file["path"].as_str().unwrap_or("");
            let ext=path.rsplit_once('.').map(|(_,ext)|ext).unwrap_or("");
            matches!(ext,"rs"|"ts"|"tsx"|"js"|"jsx"|"mjs"|"css"|"html"|"toml")
                && !path.starts_with("tests/")
                && !path.contains("/tests/")
                && !path.contains("_test.")
                && !path.contains(".test.")
                && !path.contains(".spec.")
                && !path.contains("_fixture.")
                && !crate::engine::rsi::hidden_rsi_oracle_path(path)
                && file["bytes"].as_u64().unwrap_or(0)<=2*1024*1024
        })
        .collect::<Vec<_>>();
    if files.is_empty() {return Vec::new();}

    let fields=files.iter().map(|file| {
        let path=file["path"].as_str().unwrap_or("").to_lowercase();
        let symbols=file["symbols"].as_array().into_iter().flatten()
            .filter_map(|item|item["name"].as_str())
            .collect::<Vec<_>>().join(" ").to_lowercase();
        let summary=file["summary"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).collect::<Vec<_>>().join(" ").to_lowercase();
        let body=file["bodyLexicon"].as_str().unwrap_or("").to_lowercase();
        (path,symbols,summary,body)
    }).collect::<Vec<_>>();

    let document_count=files.len() as f64;
    let frequencies=query.iter().map(|term| {
        let count=fields.iter().filter(|(path,symbols,summary,body)|
            path.contains(term) || symbols.contains(term) || summary.contains(term)
                || body.contains(term)).count();
        (term.as_str(), ((document_count+1.0)/(count as f64+1.0)).ln()+1.0)
    }).collect::<BTreeMap<_,_>>();
    let target_expression=algorithm.and_then(|value|value["parameters"]["targetExpression"].as_str());

    let mut ranked=Vec::new();
    let mut policy_invalid:Option<String>=None;
    for (file,(path,symbols,summary,body)) in files.iter().zip(fields.iter()) {
        let mut path_evidence=0.0;
        let mut symbol_evidence=0.0;
        let mut summary_evidence=0.0;
        let mut body_evidence=0.0;
        for term in &query {
            let idf=frequencies.get(term.as_str()).copied().unwrap_or(0.0);
            if path.contains(term) {path_evidence+=idf;}
            if symbols.contains(term) {symbol_evidence+=idf;}
            if summary.contains(term) {summary_evidence+=idf;}
            if body.contains(term) {body_evidence+=idf;}
        }
        let domain_evidence=if !domain.is_empty()
            && (path.contains(domain)||symbols.contains(domain)) {1.0} else {0.0};
        let evidence=path_evidence+symbol_evidence+summary_evidence+body_evidence+domain_evidence;
        if evidence==0.0 {continue;}
        let prior=prior_unproductive.get(path).copied().unwrap_or(0) as f64;
        let successful=prior_productive.get(path).copied().unwrap_or(0) as f64;
        let productive_evidence=successful.ln_1p();
        let source_cost=(file["bytes"].as_u64().unwrap_or(0) as f64/8_192.0).min(100.0);
        let features=BTreeMap::from([
            ("pathEvidence".to_string(),path_evidence),
            ("symbolEvidence".to_string(),symbol_evidence),
            ("summaryEvidence".to_string(),summary_evidence),
            ("bodyEvidence".to_string(),body_evidence),
            ("domainEvidence".to_string(),domain_evidence),
            ("repeatPenalty".to_string(),prior.min(10.0)),
            ("productiveEvidence".to_string(),productive_evidence),
            ("novelty".to_string(),1.0/(1.0+prior)),
            ("sourceCost".to_string(),source_cost),
        ]);
        let default=3.0*path_evidence+symbol_evidence+0.5*summary_evidence+
            0.8*body_evidence+domain_evidence+1.8*productive_evidence
            -0.8*prior.min(10.0)-0.15*source_cost;
        let attempted=target_expression.map(|expression|
            crate::engine::algorithms::eval_expression(expression,&features));
        let (score,policy_error)=match attempted {
            Some(Ok(value)) if value.is_finite() => (value,None),
            Some(Ok(_)) => (default,Some("nonfinite_target_score".to_string())),
            Some(Err(error)) => (default,Some(error)),
            None => (default,None),
        };
        let mut record=json!({
            "path":file["path"],
            "sourceHash":file["hash"],
            "score":score,
            "baselineScore":default,
            "evidence":{"path":path_evidence,"symbols":symbol_evidence,
                "summary":summary_evidence,"body":body_evidence,"domain":domain_evidence,
                "priorUnproductive":prior,"priorProductive":successful,
                "sourceCost":source_cost},
            "policyRevision":algorithm.map(|policy|policy["revision"].clone()).unwrap_or(Value::Null),
            "scoring":if target_expression.is_some() && policy_error.is_none() {"expression"} else {"corpus-evidence"}
        });
        if let Some(error)=policy_error {
            policy_invalid.get_or_insert_with(||error.clone());
            record["policyError"]=json!(error);
        }
        ranked.push(record);
    }
    // A candidate expression is a single algorithm, not a per-file mixture
    // of arbitrary scores and fallback scores. One runtime failure invalidates
    // the formula for the whole indexed source decision.
    if let Some(error)=policy_invalid {
        for row in &mut ranked {
            row["score"]=row["baselineScore"].clone();
            row["scoring"]=json!("corpus-evidence");
            row["policyError"]=json!(error);
        }
    }
    ranked.sort_by(|left,right|
        right["score"].as_f64().unwrap_or(0.0).total_cmp(&left["score"].as_f64().unwrap_or(0.0))
        .then_with(||left["path"].as_str().cmp(&right["path"].as_str())));
    ranked.truncate(limit.min(8));
    ranked
}

/// Score actual function bodies and their one-hop callers. The graph is a
/// deterministic navigation hint, not proof of runtime control flow.
fn rank_focus_spans(
    symbols:&[Value],tests:Option<&Vec<Value>>,lines:&[&str],terms:&[String]
)->Vec<(f64,usize,usize,String)> {
    let nodes=symbols.iter().enumerate().filter_map(|(index,item)|{
        let name=item["name"].as_str()?;
        if tests.is_some_and(|rows|rows.iter().any(|test|test.as_str()==Some(name))){
            return None;
        }
        let line=item["line"].as_u64()? as usize;
        if line==0 || line>lines.len(){return None;}
        // Symbol positions are collected in source order. Avoid a quadratic
        // scan just to locate each following declaration in large files.
        let end=symbols.get(index+1)
            .and_then(|next|next["line"].as_u64())
            .map(|next|next as usize).unwrap_or(lines.len()+1)
            .saturating_sub(1).min(lines.len());
        let body=lines[line-1..end].iter().take(450).copied()
            .collect::<Vec<_>>().join("\n").to_lowercase();
        Some((name.to_string(),line,end,body))
    }).collect::<Vec<_>>();
    let mut evidence=vec![0.0;nodes.len()];
    for term in terms{
        let df=nodes.iter().filter(|(name,_,_,body)|
            name.to_lowercase().contains(term)||body.contains(term)).count();
        if df==0{continue;}
        let idf=((nodes.len() as f64+1.0)/(df as f64+1.0)).ln()+1.0;
        for (i,(name,_,_,body)) in nodes.iter().enumerate(){
            if name.to_lowercase().contains(term){evidence[i]+=4.0*idf;}
            if body.contains(term){evidence[i]+=idf;}
        }
    }
    let mut ranking=Vec::new();
    for (i,(name,line,end,body)) in nodes.iter().enumerate(){
        let mut neighbor=0.0f64;
        // Reads only the already indexed functions; no new LLM call is needed.
        for (other,(other_name,_,_,_)) in nodes.iter().enumerate(){
            if i==other || other_name.len()<4 || evidence[other]==0.0 {continue;}
            if body.contains(&format!("{}(",other_name.to_lowercase())){
                neighbor=neighbor.max(evidence[other]);
            }
        }
        let score=evidence[i]+0.45*neighbor;
        if score>0.0{ranking.push((score,*line,*end,name.clone()));}
    }
    ranking.sort_by(|a,b|b.0.total_cmp(&a.0).then_with(||a.1.cmp(&b.1)));
    ranking
}

/// Returns compact, source-hash-verified RSI context instead of repeatedly
/// sending an entire large source file to the model on every tool round.
pub fn bounded_focus_context(
    state: &AppState, source: &Path, index_id: &str, path: &str,
    objective: &str, domain: &str,
) -> Option<String> {
    const WHOLE_FILE_CHARS: usize = 6_000;
    const MAX_CONTEXT_CHARS: usize = 8_000;
    const MAX_SPAN_CHARS: usize = 3_000;
    const MAX_SPAN_LINES: usize = 100;
    if crate::engine::rsi::hidden_rsi_oracle_path(path) {return None;}
    // Never fail silently: a missing index entry or a stale hash must tell the
    // model to read the exact range instead of leaving it without a target.
    let Some(indexed) = state.store.get(FILE_KIND, &file_id(index_id, path)) else {
        return Some(format!("\n### 대상 파일: {path}\n색인에 이 파일이 없어 발췌를 만들지 못했습니다. workspace_read_range로 필요한 구간을 확인하세요.\n"));
    };
    let Ok(bytes) = std::fs::read(source.join(path)) else {
        return Some(format!("\n### 대상 파일: {path}\n파일을 읽지 못했습니다. workspace_read_range로 필요한 구간을 확인하세요.\n"));
    };
    let hash = digest(&bytes);
    if indexed["hash"].as_str() != Some(hash.as_str()) {
        return Some(format!("\n### 대상 파일: {path}\n색인과 현재 파일 해시가 달라 발췌를 제공하지 않습니다. workspace_read_range로 필요한 구간을 확인하세요.\n"));
    }
    let content = String::from_utf8(bytes).ok()?;
    let size = content.chars().count();
    // The source excerpt is a model-editing contract, not a research summary.
    // Expose the exact checked file revision so range/function edits are bound
    // to source bytes and do not force the model to rediscover this file.
    let source_sha=indexed["hash"].as_str()?;
    if size <= WHOLE_FILE_CHARS {
        return Some(format!(
            "\n### 대상 파일 전체: {path} ({size}자, 소스 해시 검증)\nsourceSha256: {source_sha}\n원문의 정확한 oldText로 workspace_replace를 쓰거나 실제 줄 번호와 expectedSha256={source_sha}로 120줄 이하의 범위를 수정하세요. Rust 함수 전체를 작성했다면 workspace_replace_function을 선택하세요.\n~~~\n{content}\n~~~\n"
        ));
    }
    let lines = content.lines().collect::<Vec<_>>();
    // The durable navigation map stores at most MAX_SYMBOLS_PER_FILE entries.
    // Once a file is already chosen, recover every declaration from the
    // hash-verified source so late implementation code is not invisible.
    let complete_symbols = (indexed["symbolsTruncated"] == true).then(|| {
        lines.iter().enumerate().filter_map(|(index,line)| {
            let trimmed = line.trim_start();
            let found = if path.ends_with(".rs") {
                rust_symbol(trimmed)
            } else if matches!(path.rsplit_once('.').map(|(_,ext)|ext),
                Some("ts"|"tsx"|"js"|"jsx")) {
                script_symbol(trimmed)
            } else { None };
            found.map(|(kind,name)|json!({"kind":kind,"name":name,"line":index+1}))
        }).collect::<Vec<_>>()
    });
    let symbols = complete_symbols.as_ref()
        .or_else(||indexed["symbols"].as_array())?;
    let tests = indexed["tests"].as_array();
    let terms=focus_terms(objective,domain);
    let ranked=rank_focus_spans(symbols,tests,&lines,&terms);
    let mut result = format!(
        "\n### 대상 파일 발췌: {path} (전체 {size}자, 소스 해시 검증)\nsourceSha256: {source_sha}\n아래 줄 번호는 실제 소스 파일의 줄입니다. workspace_replace(startLine,endLine,newText,expectedSha256={source_sha})로 최대 120줄의 정확한 범위를 수정하세요. 이 발췌에 없는 영역만 workspace_read_range로 확인하세요.\n"
    );
    let mut included = 0usize;
    for (_, line, symbol_end, name) in ranked {
        if included >= 3 || line > lines.len() { break; }
        let start = line - 1;
        let end=symbol_end.min(lines.len()).min(start+MAX_SPAN_LINES);
        let mut body = String::new();
        let mut last = start;
        for (offset, row) in lines[start..end].iter().enumerate() {
            if body.chars().count() + row.chars().count() + 1 > MAX_SPAN_CHARS { break; }
            body.push_str(row); body.push('\n'); last = start + offset + 1;
        }
        if body.is_empty() { continue; }
        let section = format!("#### {name} (줄 {line}~{last})\n~~~\n{body}~~~\n");
        if result.chars().count() + section.chars().count() > MAX_CONTEXT_CHARS { break; }
        result.push_str(&section);
        included += 1;
    }
    if included == 0 {
        result.push_str("목표와 일치하는 함수가 색인에 없습니다. 필요한 위치만 code_search로 찾고 범위를 읽으세요.\n");
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};


    #[test]
    fn model_receives_real_current_rust_bytes_sha_and_absolute_source_lines() {
        let (state,root)=temp_state();
        let source=root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let file=source.join("src/core.rs");
        std::fs::write(&file,"pub fn actual_policy() -> u32 {\n    7\n}\n").unwrap();
        let meta=refresh(&state,&source).unwrap();
        let hash=indexed_source_hash(&state,meta["id"].as_str().unwrap(),"src/core.rs").unwrap();
        let excerpt=bounded_focus_context(&state,&source,
            meta["id"].as_str().unwrap(),"src/core.rs","actual_policy","improver").unwrap();
        assert!(excerpt.contains("pub fn actual_policy()"),"{excerpt}");
        assert!(excerpt.contains(&format!("sourceSha256: {hash}")),"{excerpt}");
        assert!(excerpt.contains("expectedSha256="),"{excerpt}");
        // A stale index can never authorize edits to a new source revision.
        std::fs::write(&file,"pub fn actual_policy() -> u32 {\n    8\n}\n").unwrap();
        let stale=bounded_focus_context(&state,&source,
            meta["id"].as_str().unwrap(),"src/core.rs","actual_policy","improver").unwrap();
        assert!(!stale.contains(&hash),"no stale source hash should appear in a model edit contract");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn lexical_terms_split_camel_case_identifiers() {
        let terms = lexical_terms("retrieveMemoryRanking bm25_score");
        for expected in ["retrieve", "memory", "ranking", "bm25", "score"] {
            assert!(terms.contains(expected), "missing {expected}: {terms:?}");
        }
    }

    #[test]
    fn invalid_target_expression_falls_back_for_all_files_not_a_mixed_ranking() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/memory.rs"),
            "fn memory_dispatch() { println!(\"memory\"); }\n").unwrap();
        std::fs::write(source.join("src/opaque.rs"),
            "fn memory_dispatch() { println!(\"memory\"); }\n").unwrap();
        let index=refresh(&state,&source).unwrap();
        let id=index["id"].as_str().unwrap();
        let empty=BTreeMap::new();
        let baseline=rank_focus_files(&state,id,"memory dispatch","memory",&empty,None,2);
        let malformed=json!({"revision":"trial","parameters":{
            "targetExpression":"1 / pathEvidence"
        }});
        let fallback=rank_focus_files(&state,id,"memory dispatch","memory",
            &empty,Some(&malformed),2);
        assert_eq!(fallback.iter().map(|row|&row["path"]).collect::<Vec<_>>(),
            baseline.iter().map(|row|&row["path"]).collect::<Vec<_>>());
        assert_eq!(fallback.len(),2);
        for row in &fallback {
            assert_eq!(row["scoring"],"corpus-evidence");
            assert!(row["policyError"].as_str().is_some());
            assert_eq!(row["score"],row["baselineScore"]);
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_productive_feedback_changes_target_ranking_without_hardcoded_paths() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        for name in ["alpha","beta"] {
            std::fs::write(source.join(format!("src/{name}.rs")),
                "fn memory_rank() { println!(\"memory retrieval\"); }\n").unwrap();
        }
        let meta=refresh(&state,&source).unwrap();
        let id=meta["id"].as_str().unwrap();
        let empty=BTreeMap::new();
        let before=rank_focus_files(&state,id,"memory rank","memory",&empty,None,2);
        assert_eq!(before[0]["path"],"src/alpha.rs");
        let learned=BTreeMap::from([("src/beta.rs".into(),1)]);
        let after=rank_focus_files_with_feedback(&state,id,"memory rank","memory",
            &empty,&learned,None,2);
        assert_eq!(after[0]["path"],"src/beta.rs");
        assert_eq!(after[0]["evidence"]["priorProductive"],1.0);
        let control=serde_json::json!({"revision":"trial-1",
            "parameters":{"targetExpression":"pathEvidence + symbolEvidence - productiveEvidence"}});
        let trial=rank_focus_files_with_feedback(&state,id,"memory rank","memory",
            &empty,&learned,Some(&control),2);
        assert_eq!(trial[0]["path"],"src/alpha.rs",
            "an evolvable formula still controls the final file selection");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn opaque_implementation_is_discovered_from_body_without_a_filename_hint() {
        let (state, root) = temp_state();
        let source = root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/neutral.rs"),
            "/*\nbm25 retrieval appears only in a block comment\n*/\nfn unrelated() {} // bm25 again\n").unwrap();
        std::fs::write(source.join("src/opaque.rs"),
            "fn hidden_worker() {\n    let api_marker = \"bm25\";\n    println!(\"{api_marker}\");\n}\n").unwrap();
        let index = refresh(&state, &source).unwrap();
        let id = index["id"].as_str().unwrap();
        let candidates = rank_focus_files(&state, id, "bm25", "", &BTreeMap::new(), None, 2);
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(candidates[0]["path"], "src/opaque.rs");
        assert!(candidates[0]["evidence"]["body"].as_f64().unwrap() > 0.0);
        // A file edit invalidates the old body terms on the next index refresh.
        std::fs::write(source.join("src/opaque.rs"),
            "fn hidden_worker() { println!(\"different\"); }\n").unwrap();
        let refreshed = refresh(&state, &source).unwrap();
        assert_eq!(refreshed["changedFiles"], 1);
        assert!(rank_focus_files(&state, id, "bm25", "", &BTreeMap::new(), None, 2).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn large_file_excerpts_recover_implementations_after_the_symbol_index_cap() {
        let (state,root) = temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut code=(0..450).map(|n|format!("fn unrelated_{n}() {{}}\n")).collect::<String>();
        code.push_str("fn late_entrypoint() {\n    let marker = \"deep_retrieval_needle\";\n    println!(\"{marker}\");\n}\n");
        std::fs::write(source.join("src/opaque.rs"),code).unwrap();
        let index=refresh(&state,&source).unwrap();
        let id=index["id"].as_str().unwrap();
        let file=state.store.get(FILE_KIND,&file_id(id,"src/opaque.rs")).unwrap();
        assert_eq!(file["symbolsTruncated"],true);
        let excerpt=bounded_focus_context(&state,&source,id,"src/opaque.rs","needle","").unwrap();
        assert!(excerpt.contains("fn late_entrypoint()"),"{excerpt}");
        assert!(excerpt.contains("deep_retrieval_needle"),"{excerpt}");
        assert!(!excerpt.contains("fn unrelated_0()"),"{excerpt}");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_focus_discovers_opaque_function_body_and_its_caller() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut code=(0..350).map(|i|format!("fn neutral_{i}() {{}}\n")).collect::<String>();
        code.push_str("fn hidden_worker() {\n    let x = \"BM25_SIGNAL_VISIBLE\";\n    println!(\"{x}\");\n}\n");
        code.push_str("fn caller_entrypoint() {\n    hidden_worker(); // DEPENDENT_CALLER_VISIBLE\n}\n");
        std::fs::write(source.join("src/opaque.rs"),&code).unwrap();
        let meta=refresh(&state,&source).unwrap();
        let excerpt=bounded_focus_context(&state,&source,meta["id"].as_str().unwrap(),
            "src/opaque.rs","bm25 retrieval","memory").unwrap();
        assert!(excerpt.contains("BM25_SIGNAL_VISIBLE"),"{excerpt}");
        assert!(excerpt.contains("DEPENDENT_CALLER_VISIBLE"),"{excerpt}");
        assert!(!excerpt.contains("fn neutral_0()"),"{excerpt}");
        assert!(excerpt.chars().count()<8_100);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_focus_prefers_relevant_spans_without_full_file_retransmission() {
        let (state, root) = temp_state();
        let source = root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut code = (0..180).map(|n| format!("fn unrelated_{n}() {{ println!(\"nope\"); }}\n")).collect::<String>();
        code.push_str("fn retrieve_memory() {\n    let target = \"RELEVANT_MARKER\";\n    println!(\"{target}\");\n}\n");
        code.push_str(&(0..100).map(|n| format!("fn tail_{n}() {{}}\n")).collect::<String>());
        std::fs::write(source.join("src/memory.rs"), &code).unwrap();
        let meta = refresh(&state, &source).unwrap();
        let excerpt = bounded_focus_context(&state, &source, meta["id"].as_str().unwrap(),
            "src/memory.rs", "기억 검색 정확도", "memory").unwrap();
        assert!(excerpt.contains("RELEVANT_MARKER"));
        assert!(!excerpt.contains("fn unrelated_0()"));
        assert!(excerpt.chars().count() < 8_100);
        std::fs::write(source.join("src/memory.rs"), format!("{code}\n// changed")).unwrap();
        let stale = bounded_focus_context(&state, &source, meta["id"].as_str().unwrap(),
            "src/memory.rs", "기억 검색", "memory").unwrap();
        assert!(stale.contains("해시가 달라"), "a stale index must not fail silently: {stale}");
        assert!(stale.contains("workspace_read_range"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_focus_keeps_small_source_complete() {
        let (state, root) = temp_state();
        let source = root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/small.rs"), "fn small() { println!(\"hello\"); }\n").unwrap();
        let meta = refresh(&state, &source).unwrap();
        let excerpt = bounded_focus_context(&state, &source, meta["id"].as_str().unwrap(),
            "src/small.rs", "small", "memory").unwrap();
        assert!(excerpt.contains("대상 파일 전체"));
        assert!(excerpt.contains("println!(\"hello\")"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rsi_navigation_reuses_the_index_without_injecting_source_or_hotspots() {
        let (state,root)=temp_state();let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let content=format!("pub fn target() {{\n// {}\n}}\n","SOURCE_ONLY_NOT_A_TASK ".repeat(4000));
        std::fs::write(source.join("src/large.rs"),content).unwrap();
        let (first,overview)=prompt_context(&state,&source,&[]).unwrap();
        assert_eq!(first["files"],1);assert!(!overview.contains("SOURCE_ONLY_NOT_A_TASK"));
        assert!(!overview.contains("정적 발견 우선순위"));
        let (next,focused)=prompt_context(&state,&source,&["src/large.rs".into()]).unwrap();
        assert_eq!(next["changedFiles"],0);assert_eq!(next["reusedFiles"],1);
        assert!(focused.contains("target:1"));assert!(focused.contains("src/large.rs"));
        assert!(!focused.contains("SOURCE_ONLY_NOT_A_TASK"));
        assert!(focused.chars().count()<1000);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn focused_navigation_includes_late_implementation_symbols_before_module_overview(){
        let (state,root)=temp_state();let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut text=(0..25).map(|i|format!("fn helper_{i}() {{}}\n")).collect::<String>();
        text.push_str("fn actual_search_hot_path() {}\n#[test]\nfn test_should_not_fill_navigation() {}\n");
        std::fs::write(source.join("src/focused.rs"),text).unwrap();
        let (_,block)=prompt_context(&state,&source,&["src/focused.rs".into()]).unwrap();
        assert!(block.contains("actual_search_hot_path:26"));
        assert!(!block.contains("test_should_not_fill_navigation"));
        assert!(block.find("actual_search_hot_path").unwrap()<block.find("### 모듈 지도").unwrap());
        let _=std::fs::remove_dir_all(root);
    }

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
        let (_, block) = prompt_context(&state, &source, &["src/engine/rsi_evolve.rs".into()]).unwrap();
        assert!(block.contains("src/engine/rsi_evolve.rs"));
        assert!(block.contains("run_loop"));
        let _ = std::fs::remove_dir_all(root);
    }
}
