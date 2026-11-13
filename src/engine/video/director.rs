//! Turnkey cinematography: stable subject cues, native LTX multi-shot.
//! No planning model, additional tokens or user-side GPU required.
pub(super) fn shot_prompt(
    brief: &str, notes: &str, index: usize, total: usize, cinematic: bool,
) -> Result<String, String> {
    let brief = brief.trim();
    if brief.is_empty() { return Err("영상 주제가 비어 있습니다.".into()); }
    if notes.chars().count() > 2_000 { return Err("장면 지시는 2,000자 이하입니다.".into()); }
    let lines = notes.lines().map(str::trim).filter(|s|!s.is_empty()).collect::<Vec<_>>();
    if lines.len() > 6 { return Err("장면 지시는 최대 6개입니다.".into()); }
    let note=lines.get(index).copied().unwrap_or("");
    if !cinematic && note.is_empty() {
        if brief.chars().count()>5_000 { return Err("LTX 프롬프트는 5,000자 이하입니다.".into()); }
        return Ok(brief.into());
    }
    let camera = match index % 4 {
        0 => "wide establishing shot, precise composition, natural depth",
        1 => "medium tracking shot, motivated camera movement",
        2 => "expressive close-up, detailed texture, controlled focus",
        _ => "resolving wide shot, spatial continuity and balanced motion",
    };
    let mut prompt = brief.to_string();
    if cinematic {
        prompt.push_str("\n\nCinematic direction: photorealistic detail, coherent subject identity, consistent wardrobe, location, lighting and color grade; natural motion, stable anatomy, deliberate blocking and matching ambient audio.");
        prompt.push_str(&format!(" Shot {}/{}: {}.",index+1,total,camera));
    }
    if !note.is_empty() { prompt.push_str("\nShot brief: "); prompt.push_str(note); }
    if prompt.chars().count()>5_000 {
        return Err("연출 지시를 포함한 프롬프트가 5,000자를 넘었습니다.".into());
    }
    Ok(prompt)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_original_brief_and_explicit_scene() {
        let text=shot_prompt("한 인물이 걷는다","골목\n창가",1,2,true).unwrap();
        assert!(text.contains("한 인물이 걷는다"));
        assert!(text.contains("Shot 2/2"));
        assert!(text.contains("창가"));
        assert_eq!(shot_prompt("원본","",0,1,false).unwrap(),"원본");
    }
}
