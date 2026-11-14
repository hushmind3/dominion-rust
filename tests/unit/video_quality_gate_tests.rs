use super::*;
#[test]
fn validates_technical_media_evidence_and_rejects_bad_clips(){
    let valid=json!({"streams":[
        {"codec_type":"video","width":1080,"height":1920},
        {"codec_type":"audio"}],
        "format":{"duration":"15.08"}});
    let report=evaluate(&valid,15,"9:16",true).unwrap();
    assert_eq!(report["verified"],true);
    assert_eq!(report["audioStream"],true);
    let wrong_ratio=json!({"streams":[{"codec_type":"video","width":1920,"height":1080},
        {"codec_type":"audio"}],"format":{"duration":"15.0"}});
    assert!(evaluate(&wrong_ratio,15,"9:16",true).unwrap_err().contains("화면비"));
    let too_short=json!({"streams":[{"codec_type":"video","width":1080,"height":1920},
        {"codec_type":"audio"}],"format":{"duration":"7.0"}});
    assert!(evaluate(&too_short,15,"9:16",true).unwrap_err().contains("시간"));
    let silent=json!({"streams":[{"codec_type":"video","width":1080,"height":1920}],
        "format":{"duration":"15.0"}});
    assert!(evaluate(&silent,15,"9:16",true).unwrap_err().contains("오디오"));
    assert!(evaluate(&silent,15,"9:16",false).is_ok());
    assert!(evaluate(&json!({"streams":[],"format":{"duration":"15.0"}}),15,"9:16",false).is_err());
    let invalid=json!({"streams":[{"codec_type":"video","width":0,"height":0}],
        "format":{"duration":"NaN"}});
    assert!(evaluate(&invalid,15,"9:16",false).is_err());
}
