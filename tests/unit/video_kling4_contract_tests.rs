use super::*;

#[test]
fn clips_are_in_model_4_native_length_limits() {
    for seconds in 3..=180 {
        let clips = clip_durations(seconds).unwrap();
        assert_eq!(clips.iter().sum::<u64>(), seconds);
        assert_eq!(clips.len(), seconds.div_ceil(30) as usize);
        assert!(clips.iter().all(|s| (3..=30).contains(s)));
    }
}

#[test]
fn text_and_image_routes_have_different_ratio_contracts() {
    let mut req = request("9:16", 30, "Moving image of a red bicycle");
    assert_eq!(req["model"], "kling_4");
    assert_eq!(req["ratio"], "1080:1920");
    assert!(req.get("audio").is_none());
    assert!(req.get("providerSettings").is_none());
    attach_prior_frame(&mut req,"data:image/jpeg;base64,ZGF0YQ==".to_string());
    assert_eq!(req["ratio"], "auto_1080p");
    assert_eq!(req["promptImage"], "data:image/jpeg;base64,ZGF0YQ==");
}

#[test]
fn prompt_carries_the_global_cast_and_target_action() {
    let prompt = direction("a silver robot holding an umbrella",
        "walking from porch into rain", 1, 3);
    assert!(prompt.contains("silver robot"));
    assert!(prompt.contains("walking from porch"));
    assert!(prompt.contains("Clip 2/3"));
    assert!(prompt.chars().count() <= 4600);
}
