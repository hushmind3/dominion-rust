//! Import the owner's character data into DOMINION records. No foreign engine,
//! configuration, credentials, memories or tools are loaded or executed.
use crate::engine::memory;
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_PERSONA: u64 = 96 * 1024;
const MAX_AVATAR: u64 = 4 * 1024 * 1024;
const SOURCE: &str = "local-user-profiles";
const PROFILES: [(&str, &str, &str); 9] = [
    ("ainz", "아인즈", "#4B3B73"),
    ("albedo", "알베도", "#7B56A0"),
    ("shalltear", "샤르티아", "#AA3154"),
    ("cocytus", "코퀴토스", "#318CA6"),
    ("demiurge", "데미우르고스", "#AD4F34"),
    ("aura", "아우라", "#8AA33A"),
    ("mare", "마레", "#55885F"),
    ("sebas", "세바스", "#63707C"),
    ("pandora-actor", "판도라 액터", "#B58C36"),
];
struct Profile {
    slug: String,
    name: String,
    color: String,
    persona: String,
    heading: String,
    description: String,
    role: String,
    avatar: Option<(Vec<u8>, String)>,
}

fn source_root() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".dominion/personas")
}
fn cached_avatar_root() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".dominion/avatars")
}
fn valid_avatar_basename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && Path::new(name).file_name().and_then(|name| name.to_str()) == Some(name)
        && Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp"
                )
            })
}
fn cached_avatar(root: &Path, name: &str) -> Result<(Vec<u8>, String), String> {
    if !valid_avatar_basename(name) {
        return Err("아바타 원본은 안전한 이미지 파일 이름이어야 합니다.".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "사용자 이미지 보관 폴더를 찾을 수 없습니다.")?;
    let bytes = bounded_read(&root, &root.join(name), MAX_AVATAR)?;
    let (ext, _) = image_type(&bytes).ok_or("지원하지 않는 아바타 원본 형식입니다.")?;
    Ok((bytes, ext.into()))
}
fn profile_spec(slug: &str) -> Result<(&'static str, &'static str, &'static str), String> {
    PROFILES
        .iter()
        .copied()
        .find(|item| item.0 == slug)
        .ok_or_else(|| "허용되지 않은 페르소나 프로필입니다.".into())
}
fn within(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "프로필 파일을 찾을 수 없습니다.")?;
    if !canonical.starts_with(root) {
        return Err("프로필의 심볼릭 링크가 허용 범위를 벗어났습니다.".into());
    }
    Ok(canonical)
}
fn bounded_read(root: &Path, path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let path = within(root, path)?;
    let file = std::fs::File::open(path).map_err(|_| "프로필 파일을 읽을 수 없습니다.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "파일 정보를 읽을 수 없습니다.")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("프로필 파일의 형식 또는 크기가 허용 범위를 벗어났습니다.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "프로필 파일을 읽을 수 없습니다.")?;
    if bytes.len() as u64 > limit {
        return Err("프로필 파일 크기 제한을 초과했습니다.".into());
    }
    Ok(bytes)
}
fn image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some(("jpg", "image/jpeg"))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(("webp", "image/webp"))
    } else {
        None
    }
}
fn read_profile(
    root: &Path,
    slug: &str,
    avatar_source: Option<(&Path, &str)>,
) -> Result<Profile, String> {
    let (_, name, color) = profile_spec(slug)?;
    let root = root
        .canonicalize()
        .map_err(|_| "사용자 페르소나 폴더를 찾을 수 없습니다.")?;
    let profile = within(&root, &root.join(slug))?;
    if !profile.is_dir() {
        return Err("프로필이 폴더가 아닙니다.".into());
    }
    let bytes = bounded_read(&profile, &profile.join("SOUL.md"), MAX_PERSONA)?;
    let persona = String::from_utf8(bytes).map_err(|_| "SOUL.md가 UTF-8 문서가 아닙니다.")?;
    if persona.trim().is_empty() {
        return Err("페르소나 문서가 비어 있습니다.".into());
    }
    let heading = persona
        .lines()
        .find_map(|line| line.strip_prefix("# 정체성:").map(str::trim))
        .unwrap_or(name)
        .to_string();
    let role = persona
        .lines()
        .find_map(|line| line.strip_prefix("담당:").map(str::trim))
        .unwrap_or("독립 에이전트")
        .chars()
        .take(240)
        .collect::<String>();
    let description = persona
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or(name)
        .chars()
        .take(240)
        .collect::<String>();
    let avatar_path = profile.join("assets/avatar.png");
    let avatar = if let Some((root, name)) = avatar_source {
        Some(cached_avatar(root, name)?)
    } else if avatar_path.exists() {
        let bytes = bounded_read(&profile, &avatar_path, MAX_AVATAR)?;
        let (ext, _) = image_type(&bytes).ok_or("지원하지 않는 아바타 형식입니다.")?;
        Some((bytes, ext.into()))
    } else {
        None
    };
    Ok(Profile {
        slug: slug.into(),
        name: name.into(),
        color: color.into(),
        persona,
        heading,
        description,
        role,
        avatar,
    })
}
fn agent_id(slug: &str) -> String {
    format!("dominion-{slug}")
}
fn preview_at(state: &AppState, root: &Path) -> Value {
    preview_with_roots(state, root, &cached_avatar_root())
}
fn preview_with_roots(state: &AppState, root: &Path, avatar_root: &Path) -> Value {
    let profiles=PROFILES.iter().map(|(slug,name,_)|{
        let id=agent_id(slug);
        let existing=state.store.get("agent",&id);
        let avatar_source=existing.as_ref().and_then(|agent|agent["sourceAvatarFile"].as_str());
        match read_profile(root,slug,avatar_source.map(|name|(avatar_root,name))){
            Ok(profile)=>json!({"sourceProfile":slug,"id":id,"name":profile.name,"description":profile.description,"role":profile.role,
                "hasAvatar":profile.avatar.is_some(),"personaPreview":profile.persona.chars().take(1600).collect::<String>(),
                "exists":existing.is_some(),"available":true,"sourceAvatarFile":avatar_source}),
            Err(error)=>json!({"sourceProfile":slug,"id":id,"name":name,"exists":existing.is_some(),"available":false,"hasAvatar":false,"error":error}),
        }
    }).collect::<Vec<_>>();
    json!({"source":SOURCE,"count":profiles.iter().filter(|profile|profile["available"]==true).count(),"profiles":profiles})
}
pub fn preview(state: &AppState) -> Value {
    preview_at(state, &source_root())
}

fn avatar_dir(state: &AppState) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&state.config.data_dir)
        .map_err(|_| "데이터 폴더를 만들 수 없습니다.")?;
    let root = state
        .config
        .data_dir
        .canonicalize()
        .map_err(|_| "데이터 폴더를 확인할 수 없습니다.")?;
    let target = root.join("avatars");
    std::fs::create_dir_all(&target).map_err(|_| "아바타 폴더를 만들 수 없습니다.")?;
    let target = target
        .canonicalize()
        .map_err(|_| "아바타 폴더를 확인할 수 없습니다.")?;
    if !target.starts_with(&root) {
        return Err("아바타 폴더가 데이터 경로를 벗어났습니다.".into());
    }
    Ok(target)
}
fn save_avatar(state: &AppState, id: &str, bytes: &[u8], ext: &str) -> Result<String, String> {
    let directory = avatar_dir(state)?;
    let filename = format!("{id}-{}.{ext}", uuid::Uuid::new_v4());
    let temporary = directory.join(format!(".avatar-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, bytes).map_err(|_| "아바타를 저장할 수 없습니다.")?;
    if std::fs::rename(&temporary, directory.join(&filename)).is_err() {
        let _ = std::fs::remove_file(temporary);
        return Err("아바타를 적용할 수 없습니다.".into());
    }
    Ok(filename)
}
fn import_at(state: &AppState, root: &Path, body: &Value) -> Result<Value, String> {
    import_with_roots(state, root, &cached_avatar_root(), body)
}
fn import_with_roots(
    state: &AppState,
    root: &Path,
    avatar_root: &Path,
    body: &Value,
) -> Result<Value, String> {
    if !body.is_object() || body["apply"].as_bool() != Some(true) {
        return Err("적용하려면 apply:true가 필요합니다.".into());
    }
    if ["root", "path", "avatarRoot", "avatarPath"]
        .iter()
        .any(|key| body.get(key).is_some())
    {
        return Err("가져오기 경로는 서버에서 정합니다.".into());
    }
    if let Some(sources) = body.get("avatarSources") {
        let sources = sources
            .as_object()
            .filter(|sources| sources.len() <= 9)
            .ok_or("avatarSources는 프로필별 이미지 파일 이름 객체여야 합니다.")?;
        for (slug, name) in sources {
            profile_spec(slug)?;
            if !name.as_str().is_some_and(valid_avatar_basename) {
                return Err(
                    "avatarSources에는 안전한 이미지 파일 이름만 지정할 수 있습니다.".into(),
                );
            }
        }
    }
    if !state.begin_session("personas:import") {
        return Err("페르소나 가져오기가 이미 실행 중입니다.".into());
    }
    let _guard = state.session_guard("personas:import");
    let requested = if let Some(value) = body.get("profileIds") {
        let values = value
            .as_array()
            .filter(|values| !values.is_empty() && values.len() <= 9)
            .ok_or("profileIds는 1~9개 프로필 목록이어야 합니다.")?;
        let mut requested = Vec::new();
        for value in values {
            let slug = value.as_str().ok_or("프로필 ID가 문자열이 아닙니다.")?;
            profile_spec(slug)?;
            if !requested.iter().any(|known| known == slug) {
                requested.push(slug.to_string());
            }
        }
        requested
    } else {
        PROFILES.iter().map(|item| item.0.to_string()).collect()
    };
    let provider = if let Some(id) = body["providerId"].as_str().filter(|id| !id.is_empty()) {
        Some(
            state
                .store
                .get("provider", id)
                .ok_or("선택한 모델 제공자를 찾을 수 없습니다.")?,
        )
    } else {
        state
            .store
            .all("provider")
            .into_iter()
            .find(|provider| provider["enabled"].as_bool() != Some(false))
    };
    let provider_id = provider
        .as_ref()
        .and_then(|provider| provider["id"].as_str())
        .unwrap_or("");
    let model = body["model"]
        .as_str()
        .filter(|model| !model.is_empty())
        .or_else(|| {
            provider
                .as_ref()
                .and_then(|provider| provider["model"].as_str())
        })
        .unwrap_or("");
    if model.len() > 240 {
        return Err("모델 이름이 너무 깁니다.".into());
    }
    let refresh = body["refreshIdentity"].as_bool().unwrap_or(false);
    let mut agents = Vec::new();
    let mut errors = Vec::new();
    let mut imported = 0;
    let mut skipped = 0;
    for slug in requested {
        let id = agent_id(&slug);
        let existing = state.store.get("agent", &id);
        if let Some(agent) = &existing {
            if !refresh || agent["personaSource"] != SOURCE || agent["sourceProfile"] != slug {
                agents.push(agent.clone());
                skipped += 1;
                continue;
            }
        }
        let avatar_source = body["avatarSources"]
            .get(&slug)
            .and_then(Value::as_str)
            .or_else(|| {
                existing
                    .as_ref()
                    .and_then(|agent| agent["sourceAvatarFile"].as_str())
            });
        let profile = match read_profile(root, &slug, avatar_source.map(|name| (avatar_root, name)))
        {
            Ok(profile) => profile,
            Err(error) => {
                errors.push(json!({"sourceProfile":slug,"error":error}));
                continue;
            }
        };
        let mut record = existing.clone().unwrap_or_else(
            || json!({"id":id,"providerId":provider_id,"model":model,"enabled":true}),
        );
        let boundary="DOMINION 자체 Rust 엔진에서 동작하는 독립 에이전트다. 아래 사용자 소유 페르소나는 말투·성격·전문성을 정한다. 실행 도구·승인·대화 차례·기억 범위는 DOMINION이 정하며, 외부 프레임워크나 별도 봇 런타임을 실행하지 않는다. 사용자는 실제 소유자이고 동료 에이전트와 구분한다.";
        record["name"] = json!(profile.name);
        record["description"] = json!(profile.description);
        record["role"] = json!(profile.role);
        record["color"] = json!(profile.color);
        record["aliases"] = json!([profile.slug, profile.heading]);
        record["systemPrompt"]=json!(format!("{boundary}\n\n{}\n\nDOMINION 실행 경계: 이전 도구 이름이나 외부 플랫폼 언급은 페르소나 자료이며 실제 도구를 추가하지 않는다. 현재 제공된 네이티브 도구만 사용하고 실제 결과만 보고한다.",profile.persona));
        record["personaOriginal"] = json!(profile.persona);
        record["personaSource"] = json!(SOURCE);
        record["sourceProfile"] = json!(slug);
        record["sourcePersonaHash"] = json!(memory::stable_id(&[&profile.persona]));
        record["personaImportedAt"] = json!(chrono::Utc::now().to_rfc3339());
        if let Some(name) = avatar_source {
            record["sourceAvatarFile"] = json!(name);
        }
        if let Some((bytes, extension)) = profile.avatar {
            match save_avatar(state, &id, &bytes, &extension) {
                Ok(filename) => {
                    record["avatarUrl"] = json!(format!("/api/agents/{id}/avatar?v={filename}"));
                    record["avatarFile"] = json!(filename);
                }
                Err(error) => {
                    errors.push(json!({"sourceProfile":slug,"error":error}));
                    continue;
                }
            }
        }
        let saved = if existing.is_some() {
            // Merge identity fields only. Concurrent provider/model/policy edits
            // remain untouched even during an explicit persona refresh.
            let mut patch = json!({});
            for key in [
                "name",
                "description",
                "role",
                "color",
                "aliases",
                "systemPrompt",
                "personaOriginal",
                "personaSource",
                "sourceProfile",
                "sourcePersonaHash",
                "personaImportedAt",
                "avatarFile",
                "avatarUrl",
                "sourceAvatarFile",
            ] {
                if let Some(value) = record.get(key) {
                    patch[key] = value.clone();
                }
            }
            state
                .store
                .patch_if("agent", &id, "sourceProfile", &json!(slug), &patch)
        } else {
            state.store.put_if_absent("agent", &record)
        };
        match saved {
            Some(saved) => {
                state.bus.record("agent", &saved);
                agents.push(saved);
                imported += 1;
            }
            None => {
                errors.push(json!({"sourceProfile":slug,"error":"에이전트 저장에 실패했습니다."}))
            }
        }
    }
    state.store.audit(
        "personas.imported",
        json!({"source":SOURCE,"imported":imported,"skipped":skipped,"errors":errors.len()}),
    );
    Ok(
        json!({"source":SOURCE,"agents":agents,"imported":imported,"skipped":skipped,"errors":errors}),
    )
}
pub fn import(state: &AppState, body: &Value) -> Result<Value, String> {
    import_at(state, &source_root(), body)
}
pub fn avatar(state: &AppState, id: &str) -> Result<(Vec<u8>, String), String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
    {
        return Err("유효하지 않은 에이전트 ID입니다.".into());
    }
    let agent = state
        .store
        .get("agent", id)
        .ok_or("에이전트를 찾을 수 없습니다.")?;
    let filename = agent["avatarFile"].as_str().ok_or("아바타가 없습니다.")?;
    if filename.contains('/')
        || filename.contains('\\')
        || !(filename.starts_with(&format!("{id}.")) || filename.starts_with(&format!("{id}-")))
    {
        return Err("아바타 경로가 유효하지 않습니다.".into());
    }
    let directory = avatar_dir(state)?;
    let bytes = bounded_read(&directory, &directory.join(filename), MAX_AVATAR)?;
    let (_, mime) = image_type(&bytes).ok_or("아바타 형식을 확인할 수 없습니다.")?;
    Ok((bytes, mime.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (AppState, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-personas-{}", uuid::Uuid::new_v4()));
        let profiles = root.join("profiles");
        std::fs::create_dir_all(profiles.join("ainz/assets")).unwrap();
        std::fs::write(
            profiles.join("ainz/SOUL.md"),
            "# 정체성: 아인즈 울 고운\n\n침착한 사용자 소유 성격\n담당: 전략과 분석\n",
        )
        .unwrap();
        std::fs::write(
            profiles.join("ainz/assets/avatar.png"),
            b"\x89PNG\r\n\x1a\nowner-avatar",
        )
        .unwrap();
        std::fs::write(
            profiles.join("ainz/config.env"),
            "DO_NOT_IMPORT_SECRET=secret-value",
        )
        .unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        (state, root, profiles)
    }
    #[test]
    fn import_copies_only_persona_and_avatar_and_preserves_edits() {
        let (state, root, profiles) = setup();
        state
            .store
            .put("provider", &json!({"id":"native","model":"own-model"}));
        let preview = preview_at(&state, &profiles);
        assert_eq!(preview["count"], 1);
        assert_eq!(state.store.count("agent"), 0);
        let body = json!({"apply":true,"profileIds":["ainz"]});
        let result = import_at(&state, &profiles, &body).unwrap();
        assert_eq!(result["imported"], 1);
        let agent = &result["agents"][0];
        assert_eq!(agent["name"], "아인즈");
        assert_eq!(agent["model"], "own-model");
        assert!(!agent.to_string().contains("secret-value"));
        assert!(agent["systemPrompt"]
            .as_str()
            .unwrap()
            .contains("Rust 엔진"));
        let (bytes, mime) = avatar(&state, "dominion-ainz").unwrap();
        assert!(bytes.starts_with(b"\x89PNG"));
        assert_eq!(mime, "image/png");
        state.store.patch_if(
            "agent",
            "dominion-ainz",
            "id",
            &json!("dominion-ainz"),
            &json!({"model":"custom-model","systemPrompt":"주인이 수정한 성격","temperature":0.2}),
        );
        assert_eq!(import_at(&state, &profiles, &body).unwrap()["skipped"], 1);
        assert_eq!(
            state.store.get("agent", "dominion-ainz").unwrap()["systemPrompt"],
            "주인이 수정한 성격"
        );
        let refreshed = import_at(
            &state,
            &profiles,
            &json!({"apply":true,"profileIds":["ainz"],"refreshIdentity":true}),
        )
        .unwrap();
        assert_eq!(refreshed["imported"], 1);
        let agent = state.store.get("agent", "dominion-ainz").unwrap();
        assert_eq!(agent["model"], "custom-model");
        assert_eq!(agent["temperature"], 0.2);
        assert!(import_at(
            &state,
            &profiles,
            &json!({"apply":true,"profileIds":["../../config.env"]})
        )
        .is_err());
        assert!(avatar(&state, "../../config.env").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn selected_cached_character_image_is_preserved_on_preview_and_refresh() {
        let (state, root, profiles) = setup();
        let cache = root.join("image-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let original = b"\xff\xd8\xffowner-character-image";
        std::fs::write(cache.join("portrait.jpg"), original).unwrap();
        let first = import_with_roots(
            &state,
            &profiles,
            &cache,
            &json!({"apply":true,"profileIds":["ainz"],"avatarSources":{"ainz":"portrait.jpg"}}),
        )
        .unwrap();
        assert_eq!(first["imported"], 1);
        let agent = &first["agents"][0];
        assert_eq!(agent["sourceAvatarFile"], "portrait.jpg");
        assert!(agent["avatarUrl"].as_str().unwrap().contains("/avatar?v="));
        let (bytes, mime) = avatar(&state, "dominion-ainz").unwrap();
        assert_eq!(bytes, original);
        assert_eq!(mime, "image/jpeg");
        // The old profile image can be unsuitable; an explicit saved image
        // selection remains authoritative and does not fall back to it.
        std::fs::write(
            profiles.join("ainz/assets/avatar.png"),
            "placeholder-not-an-image",
        )
        .unwrap();
        let preview = preview_with_roots(&state, &profiles, &cache);
        assert_eq!(preview["profiles"][0]["sourceAvatarFile"], "portrait.jpg");
        assert_eq!(preview["profiles"][0]["hasAvatar"], true);
        let second = import_with_roots(
            &state,
            &profiles,
            &cache,
            &json!({"apply":true,"profileIds":["ainz"],"refreshIdentity":true}),
        )
        .unwrap();
        assert_eq!(second["imported"], 1);
        assert_eq!(second["agents"][0]["sourceAvatarFile"], "portrait.jpg");
        assert_ne!(agent["avatarUrl"], second["agents"][0]["avatarUrl"]);
        assert_eq!(avatar(&state, "dominion-ainz").unwrap().0, original);
        for bad in [
            "../outside.jpg",
            "/absolute.jpg",
            "folder/image.jpg",
            "folder\\image.jpg",
            ".hidden.jpg",
            "credentials.env",
        ] {
            assert!(import_with_roots(
                &state,
                &profiles,
                &cache,
                &json!({"apply":true,"profileIds":["ainz"],"avatarSources":{"ainz":bad}})
            )
            .is_err());
        }
        assert!(import_with_roots(
            &state,
            &profiles,
            &cache,
            &json!({"apply":true,"avatarRoot":"/tmp"})
        )
        .is_err());
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[test]
    fn cached_image_symlink_escape_and_invalid_magic_do_not_replace_selected_photo() {
        use std::os::unix::fs::symlink;
        let (state, root, profiles) = setup();
        let cache = root.join("image-cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(root.join("outside.jpg"), b"\xff\xd8\xffoutside").unwrap();
        symlink(root.join("outside.jpg"), cache.join("escape.jpg")).unwrap();
        std::fs::write(cache.join("not-image.jpg"), "not-an-image").unwrap();
        for bad in ["escape.jpg", "not-image.jpg"] {
            let result = import_with_roots(
                &state,
                &profiles,
                &cache,
                &json!({"apply":true,"profileIds":["ainz"],"avatarSources":{"ainz":bad}}),
            )
            .unwrap();
            assert_eq!(result["imported"], 0);
            assert_eq!(result["errors"].as_array().unwrap().len(), 1);
        }
        assert_eq!(state.store.count("agent"), 0);
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[test]
    fn persona_and_avatar_symlinks_cannot_escape_source_or_runtime() {
        use std::os::unix::fs::symlink;
        let (state, root, profiles) = setup();
        std::fs::write(root.join("outside.md"), "outside private data").unwrap();
        std::fs::remove_file(profiles.join("ainz/SOUL.md")).unwrap();
        symlink(root.join("outside.md"), profiles.join("ainz/SOUL.md")).unwrap();
        assert!(read_profile(&profiles, "ainz", None).is_err());
        state.store.put(
            "agent",
            &json!({"id":"dominion-ainz","avatarFile":"dominion-ainz.png"}),
        );
        let directory = avatar_dir(&state).unwrap();
        symlink(root.join("outside.md"), directory.join("dominion-ainz.png")).unwrap();
        assert!(avatar(&state, "dominion-ainz").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
