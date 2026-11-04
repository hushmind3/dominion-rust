/// Free Google Translate endpoint used only for display text (reasoning and
/// selected text). Chunked so long reasoning does not exceed URL limits.
pub async fn to_korean(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if current.len() + line.len() > 1200 && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    if chunks.len() > 16 {
        return None;
    }

    let client = reqwest::Client::new();
    let mut out = String::new();
    for chunk in chunks {
        let url = format!(
            "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=ko&dt=t&q={}",
            urlencode(&chunk)
        );
        let response = client
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let value: serde_json::Value = response.json().await.ok()?;
        let translated = value
            .get(0)?
            .as_array()?
            .iter()
            .filter_map(|row| row.get(0).and_then(|cell| cell.as_str()))
            .collect::<String>();
        if translated.trim().is_empty() {
            return None;
        }
        out.push_str(&translated);
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    Some(out.trim().to_string())
}

pub fn needs_korean(text: &str) -> bool {
    let total = text.chars().count();
    if total < 40 {
        return false;
    }
    let hangul = text
        .chars()
        .filter(|ch| ('\u{ac00}'..='\u{d7a3}').contains(ch))
        .count();
    let latin = text.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
    latin > 40 && (hangul as f32 / total as f32) < 0.25
}

fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_english_reasoning() {
        assert!(needs_korean(
            "We need answer in Korean. The user asks about Rust and wants two sentences."
        ));
        assert!(!needs_korean("러스트는 소유권 덕분에 메모리 오류를 컴파일 단계에서 잡습니다."));
        assert!(!needs_korean("short"));
    }
}
