use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Skill catalog: scans SKILL.md trees and installs a copy into the data dir.
/// No Node process is involved; this is plain Rust filesystem work.
pub fn roots(state: &AppState) -> Vec<(String, PathBuf)> {
    let home = std::env::var("HOME").unwrap_or_default();
    vec![
        ("DOMINION".to_string(), state.config.data_dir.join("skills")),
        (
            "워크스페이스".to_string(),
            state.config.workspace.join("skills"),
        ),
        (
            "DOMINION 사용자".to_string(),
            PathBuf::from(&home).join(".dominion/skills"),
        ),
        (
            "플러그인".to_string(),
            state.config.data_dir.join("plugins"),
        ),
    ]
}

fn id_for(path: &Path) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, depth + 1, found);
        } else if name == "SKILL.md" {
            found.push(path);
        }
    }
}

fn frontmatter(text: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut lines = text.lines();
    if let Some(first) = lines.next() {
        if first.trim() == "---" {
            for line in lines {
                let line = line.trim();
                if line == "---" {
                    break;
                }
                if let Some(value) = line.strip_prefix("name:") {
                    name = value.trim().trim_matches('"').to_string();
                } else if let Some(value) = line.strip_prefix("description:") {
                    description = value.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    (name, description)
}

fn item(path: &Path, source: &str) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let (mut name, description) = frontmatter(&text);
    if name.is_empty() {
        name = path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "Skill".into());
    }
    Some(json!({
        "id": id_for(path),
        "name": name,
        "description": description,
        "source": source,
        "path": path.to_string_lossy(),
        "dir": path.parent().map(|parent| parent.to_string_lossy().to_string()),
    }))
}

pub fn catalog(state: &AppState) -> Value {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (source, root) in roots(state) {
        if !root.exists() {
            continue;
        }
        let mut found = Vec::new();
        walk(&root, 0, &mut found);
        for path in found {
            if let Some(item) = item(&path, &source) {
                if let Some(id) = item.get("id").and_then(|value| value.as_str()) {
                    if seen.insert(id.to_string()) {
                        items.push(item);
                    }
                }
            }
        }
    }
    json!({
        "items": items,
        "total": items.len(),
        "updatedAt": chrono::Utc::now().to_rfc3339(),
        "roots": roots(state).into_iter().map(|(source, path)| json!({
            "source": source,
            "path": path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    })
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn install(state: &AppState, source_dir: &Path) -> Result<Value, String> {
    let skill_file = source_dir.join("SKILL.md");
    let text = std::fs::read_to_string(&skill_file)
        .map_err(|error| format!("SKILL.md 읽기 실패: {error}"))?;
    let (mut name, description) = frontmatter(&text);
    if name.is_empty() {
        name = source_dir
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "Skill".into());
    }
    let slug: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let target = state.config.data_dir.join("skills").join(&slug);
    copy_dir(source_dir, &target).map_err(|error| format!("스킬 복사 실패: {error}"))?;

    let record = state.store.put(
        "skill",
        &json!({
            "name": name,
            "description": description,
            "instructions": text,
            "enabled": false,
            "status": "draft",
            "source": "catalog",
            "path": target.to_string_lossy(),
        }),
    );
    if let Some(record) = &record {
        state.bus.record("skill", record);
    }
    record.ok_or_else(|| "스킬을 저장할 수 없습니다.".into())
}

pub fn import(state: &AppState, id: &str) -> Result<Value, String> {
    let catalog = catalog(state);
    let item = catalog
        .get("items")
        .and_then(|value| value.as_array())
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(id))
        })
        .ok_or_else(|| "카탈로그에서 스킬을 찾을 수 없습니다.".to_string())?;
    let dir = item
        .get("dir")
        .and_then(|value| value.as_str())
        .ok_or("스킬 경로가 없습니다.")?;
    install(state, Path::new(dir))
}

pub fn import_repo(state: &AppState, repo: &str) -> Result<Value, String> {
    let repo = repo.trim();
    if !(repo.starts_with("https://") || repo.starts_with("git@")) {
        return Err("https:// 또는 git@ 저장소 URL만 지원합니다.".into());
    }
    let temp = std::env::temp_dir().join(format!("dominion-skills-{}", uuid::Uuid::new_v4()));
    let output = std::process::Command::new("git")
        .args(["clone", "--depth", "1", repo])
        .arg(&temp)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let mut found = Vec::new();
    walk(&temp, 0, &mut found);
    let mut added = 0;
    let mut names = Vec::new();
    for file in found {
        if let Some(dir) = file.parent() {
            if install(state, dir).is_ok() {
                added += 1;
                names.push(
                    dir.file_name()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&temp);
    if added == 0 {
        return Err("저장소에서 SKILL.md를 찾지 못했습니다.".into());
    }
    Ok(json!({ "repo": repo, "added": added, "skills": names }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn scans_and_imports_a_skill() {
        let (state, dir) = temp_state();
        let skill_dir = dir.join("skills").join("reviewer");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Reviewer\ndescription: Reviews diffs\n---\nBe strict.\n",
        )
        .unwrap();

        let catalog = catalog(&state);
        let items = catalog["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "Reviewer");
        let id = items[0]["id"].as_str().unwrap().to_string();

        let installed = import(&state, &id).expect("import");
        assert_eq!(installed["name"], "Reviewer");
        assert!(dir.join("skills/Reviewer/SKILL.md").exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_non_http_repo() {
        let (state, dir) = temp_state();
        assert!(import_repo(&state, "file:///etc/passwd").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
