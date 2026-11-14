//! Deterministic goal-repair diagnostics from witnessed tool and acceptance
//! failures. Model prose is never a signal that an operation was completed.
//!
//! This is the native Rust diagnostic/replanning layer, not another LLM,
//! external SDK, or hidden agent executor. Only real tool events and frozen
//! acceptance gaps influence the failure fingerprint.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::BTreeSet;

#[derive(Debug,Clone)]
pub(super) struct Diagnosis {
    pub record: Value,
    pub guidance: String,
    pub replan_now: bool,
}

fn clip(text:&str,max:usize)->String{
    let mut content=text.chars().take(max).collect::<String>();
    if text.chars().count()>max{content.push('…');}
    content
}

fn classify(details:&[Value],turn_error:Option<&str>)->&'static str{
    if details.iter().any(|d|d["effectUnknown"]==true){
        return "external_effect_unknown";
    }
    let stderr=details.iter().map(|d|{
        format!("{} {}",d["error"].as_str().unwrap_or(""),
        d["stderr"].as_str().unwrap_or(""))
    }).collect::<Vec<_>>().join("\n").to_lowercase();
    if stderr.contains("error[e") || stderr.contains("compilation failed")
        || stderr.contains("syntaxerror") || stderr.contains("type error")
        || stderr.contains("could not compile")
    {
        return "compiler_or_typecheck";
    }
    if stderr.contains("test failed") || stderr.contains("tests failed")
        || stderr.contains("assertion failed") || stderr.contains("failed tests")
        || stderr.contains("failures:")
    {
        return "failed_tests";
    }
    if stderr.contains("not found") || stderr.contains("no such file")
        || stderr.contains("unknown tool") || stderr.contains("invalid")
        || stderr.contains("존재하지")
    {
        return "tool_arguments_or_environment";
    }
    if stderr.contains("permission denied") || stderr.contains("승인") 
        || stderr.contains("권한")
    {
        return "permission_or_approval";
    }
    if !details.is_empty(){return "tool_failure";}
    if turn_error.is_some(){return "model_or_transport";}
    "acceptance_stall"
}

fn next_action(category:&str)->&'static str{
    match category {
        "external_effect_unknown" =>
            "도구의 외부 결과가 미확정입니다. 같은 외부 변경을 재발행하지 말고 대상 상태를 먼저 읽어 확인하세요.",
        "compiler_or_typecheck" =>
            "실제 컴파일 오류 위치와 타입을 읽고 최소 범위로 수정한 뒤 동일 검사를 다시 실행하세요.",
        "failed_tests" =>
            "실패한 실제 테스트의 assertion, 입력과 예상 결과를 조사하고 구현을 고쳐 같은 테스트로 검증하세요.",
        "tool_arguments_or_environment" =>
            "도구 인자·파일 위치·실행환경을 먼저 조회해 수정하세요. 실패한 동일 호출을 그대로 반복하지 마세요.",
        "permission_or_approval" =>
            "도구 실행 권한 상태를 확인하세요. 기존에 거부된 변경을 동일하게 재전송하지 마세요.",
        "model_or_transport" =>
            "모델 호출과 입력 크기·응답 형식을 확인하고 필요하면 다른 제공자로 전환하세요.",
        "acceptance_stall" =>
            "미충족 검증 조건의 구체적 원인을 파일·테스트·작업 기록으로 확인하고 접근을 변경하세요.",
        _ =>
            "실패한 실제 도구의 오류 출력과 인자를 조사하고 다른 수정 방법으로 진행하세요.",
    }
}

pub(super) fn observe(
    iteration:usize,
    failures:&[Value],
    turn_error:Option<&str>,
    acceptance_gaps:&[String],
    advanced:bool,
    prior:&[Value],
)->Option<Diagnosis>{
    if failures.is_empty() && turn_error.is_none()
        && (advanced || acceptance_gaps.is_empty())
    {return None;}
    let category=classify(failures,turn_error);
    // Tool names and bounded tool outputs form a stable, replayable
    // fingerprint. Timestamp/model prose are deliberately excluded.
    let mut parts=failures.iter().take(8).map(|d|{
        format!("{}|{}|{}|{}",
            d["name"].as_str().unwrap_or(""),
            d["exitCode"].as_i64().unwrap_or(-1),
            clip(d["error"].as_str().unwrap_or(""),140),
            clip(d["stderr"].as_str().unwrap_or(""),240))
    }).collect::<Vec<_>>();
    parts.sort();
    parts.dedup();
    if parts.is_empty(){
        // The acceptance predicates themselves are frozen by the harness.
        // A second unchanged gap is an actual persistent stall signal.
        parts=acceptance_gaps.iter().take(8).map(|g|clip(g,180)).collect();
        parts.sort();
        parts.dedup();
        if parts.is_empty(){parts.push(clip(turn_error.unwrap_or(""),240));}
    }
    let fingerprint=format!("{:x}",Sha256::digest(
        format!("{category}\n{}",parts.join("\n")).as_bytes()
    ));
    let repeat_count=prior.iter().rev().take(6)
        .filter(|item|item["fingerprint"].as_str()==Some(fingerprint.as_str()))
        .count()+1;
    let tool_examples=failures.iter().take(3).map(|detail|{
        json!({
            "tool":detail["name"],
            "callId":detail["callId"],
            "exitCode":detail["exitCode"],
            "error":clip(detail["error"].as_str().unwrap_or(""),450),
            "stderr":clip(detail["stderr"].as_str().unwrap_or(""),900),
        })
    }).collect::<Vec<_>>();
    let record=json!({
        "iteration":iteration,"class":category,
        "fingerprint":fingerprint,"repeatCount":repeat_count,
        "failedToolCalls":failures.len(),
        "acceptanceGapCount":acceptance_gaps.len(),
        "advanced":advanced,
        "tools":tool_examples,
        "modelError":turn_error.map(|e|clip(e,450)),
    });
    let mut guidance=format!(
        "관찰된 실패 분류: {category}; 같은 증거 반복: {repeat_count}회. {}",
        next_action(category));
    if !failures.is_empty(){
        for detail in failures.iter().take(3){
            guidance.push_str("\n도구=");
            guidance.push_str(detail["name"].as_str().unwrap_or(""));
            if let Some(exit)=detail["exitCode"].as_i64(){
                guidance.push_str(&format!(" exit={exit}"));
            }
            let cause=detail["stderr"].as_str()
                .filter(|v|!v.trim().is_empty())
                .or(detail["error"].as_str()).unwrap_or("");
            if !cause.is_empty(){
                guidance.push_str(" 원인=");
                guidance.push_str(&clip(cause,650));
            }
        }
    }
    if let Some(e)=turn_error{
        guidance.push_str("\n실행 오류=");
        guidance.push_str(&clip(e,400));
    }
    if category=="external_effect_unknown"{
        // The existing effect-reconciliation path owns external side-effect
        // uncertainty. Never use model replan to blindly retry the operation.
        return Some(Diagnosis{record,guidance,replan_now:false});
    }
    // Objective progress still counts, but repeated errors need different
    // method selection; otherwise the model can loop a failing command.
    let replan_now=(repeat_count>=2 && !failures.is_empty())
        || (failures.len()>=2 && !advanced)
        || (repeat_count>=3 && !advanced);
    Some(Diagnosis{record,guidance,replan_now})
}

pub(super) fn push_bounded(history:&mut Vec<Value>,row:Value){
    history.push(row);
    if history.len()>32{history.drain(..history.len()-32);}
}

pub(super) fn distinct_failure_modes(history:&[Value])->usize{
    history.iter().filter_map(|v|v["fingerprint"].as_str())
        .collect::<BTreeSet<_>>().len()
}

#[cfg(test)]
mod tests{
    use super::*;

    fn failed(tool:&str,stderr:&str)->Value{
        json!({"name":tool,"exitCode":101,"error":"test failed","stderr":stderr})
    }
    #[test]
    fn real_test_failure_stays_bounded_and_repeated_observation_triggers_replan(){
        let failure=failed("shell_exec",&format!(
            "error[E0425]: cannot find value\n{}","x".repeat(5000)));
        let first=observe(1,&[failure.clone()],None,&["tests not passing".into()],
            false,&[]).unwrap();
        assert_eq!(first.record["class"],"compiler_or_typecheck");
        assert!(first.guidance.contains("error[E0425]"));
        assert!(first.guidance.chars().count()<1500);
        assert!(!first.replan_now);
        let second=observe(2,&[failure],None,&["tests not passing".into()],
            false,&[first.record]).unwrap();
        assert_eq!(second.record["repeatCount"],2);
        assert!(second.replan_now);
    }
    #[test]
    fn check_gaps_are_not_generated_from_a_model_success_claim(){
        assert!(observe(1,&[],None,&[],true,&[]).is_none());
        let first=observe(1,&[],None,&["file not created".into()],false,&[]).unwrap();
        assert_eq!(first.record["class"],"acceptance_stall");
        assert!(!first.replan_now);
        let second=observe(2,&[],None,&["file not created".into()],false,
            &[first.record.clone()]).unwrap();
        assert_eq!(second.record["repeatCount"],2);
        let third=observe(3,&[],None,&["file not created".into()],false,
            &[first.record,second.record]).unwrap();
        assert!(third.replan_now);
    }
    #[test]
    fn unknown_remote_effect_is_never_authorized_for_blind_replan(){
        let d=json!({"name":"sdk__pay","effectUnknown":true,
            "error":"timeout"});
        let result=observe(1,&[d.clone(),d],None,&[],false,&[]).unwrap();
        assert_eq!(result.record["class"],"external_effect_unknown");
        assert!(!result.replan_now);
        assert!(result.guidance.contains("재발행하지"));
    }
    #[test]
    fn different_test_errors_have_different_signatures(){
        let a=observe(1,&[failed("shell_exec","AssertionError: a")],None,
            &["failing test".into()],false,&[]).unwrap();
        let b=observe(2,&[failed("shell_exec","AssertionError: b")],None,
            &["failing test".into()],false,&[a.record.clone()]).unwrap();
        assert_ne!(a.record["fingerprint"],b.record["fingerprint"]);
        assert_ne!(b.record["repeatCount"],2);
        let mut history=vec![a.record,b.record];
        assert_eq!(distinct_failure_modes(&history),2);
        for i in 0..40{push_bounded(&mut history,json!({"fingerprint":i.to_string()}));}
        assert_eq!(history.len(),32);
    }
}
