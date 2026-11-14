//! Action acceptance for ordinary turns. Only the actual request is examined;
//! examples, peer quotations and provider completion claims cannot prove work.
use crate::{engine::goals,state::AppState};
use serde_json::Value;

/// An imperative implementation request means real tool work is expected,
/// even when the owner did not spell out a filename. Descriptive and
/// hypothetical requests are not automatically treated as commands.
fn explicit_implementation_request(text: &str) -> bool {
    // Match actual commands, not descriptive phrases such as "구현할지",
    // "구현하는 방식" or "어떻게 고치면". A goal-level request may have no
    // filename; this is its only route into evidence-based execution.
    const IMPERATIVES: &[&str] = &[
        "구현해", "구현하라", "구현하라고", "구현하세요", "구현하십시오",
        "개발해", "개발하라", "개발하라고", "개발하세요",
        "고쳐", "고치세요", "고치라고", "고치십시오",
        "수정해", "수정하라", "수정하라고", "수정하세요",
        "적용해", "적용하라", "적용하라고", "적용하세요",
        "반영해", "반영하라", "반영하라고", "반영하세요",
        "추가해", "추가하라", "추가하라고", "추가하세요",
        "연결해", "연결하라", "연결하라고", "연결하세요",
        "완성해", "완성하라", "완성하라고", "완성하세요",
        "개선해", "개선하라", "개선하라고", "개선하세요",
        "고도화해", "고도화하라", "고도화하라고", "고도화하세요",
        "implement ", "fix ", "patch ", "refactor ", "complete ", "improve ",
    ];
    IMPERATIVES.iter().any(|verb| text.contains(verb))
}

pub(crate) fn prepare_owner_action(state:&AppState,input:&str)->Option<(String,Value)>{
    let mut fenced=false;let mut lines=Vec::new();
    for line in input.lines(){let trimmed=line.trim();if trimmed.starts_with("```"){fenced=!fenced;continue;}if fenced||trimmed.starts_with('>'){continue;}
        let lower=trimmed.to_lowercase();if ["예시:","예제:","example:","for example:","quoted:"].iter().any(|prefix|lower.starts_with(prefix)){continue;}lines.push(line);
    }
    let request=lines.join("\n");let lower=request.to_lowercase();
    if ["설명만","실행하지 마","실행하지마","수정하지 마","만들지 마","don't execute","do not execute","do not create","do not modify"].iter().any(|term|lower.contains(term)){return None;}
    let direct=["해줘","해 줘","해라","해주세요","실제로","직접","실행해","저장해","만들어","수정해","삭제해","설치해","빌드해","배포해","커밋해"].iter().any(|term|lower.contains(term));
    if !direct&&["어떻게","방법","설명","예시","예제","how to","how do","explain","what is","example","would happen"].iter().any(|term|lower.contains(term)){return None;}
    let english=lower.trim_start_matches("please ").trim_start_matches("can you ").trim_start_matches("could you ");
    let file_action=["create ","write ","save ","edit ","delete ","remove ","update "].iter().any(|prefix|english.starts_with(prefix))
        ||["만들","생성","저장","수정","편집","삭제","제거"].iter().any(|term|lower.contains(term));
    let file_target=[".txt",".md",".rs",".json",".py",".js",".ts",".csv",".toml",".html",".yml",".yaml","파일","폴더","저장소","project","repository","directory","folder"," file"].iter().any(|term|lower.contains(term));
    let command_action=["run ","install ","build ","deploy ","commit ","download ","copy ","move "].iter().any(|prefix|english.starts_with(prefix))
        ||["실행해","테스트해","빌드해","설치해","배포해","커밋해","복사해","이동해","다운로드해"].iter().any(|term|lower.contains(term));
    // Implementation commands do not require an explicit filename. The
    // existing verifier still demands observed changes; mere prose does not
    // constitute successful work.
    if !(file_action&&file_target||command_action||explicit_implementation_request(&lower)){return None;}
    let checks=goals::prepare_acceptance_checks(state,&request);Some((request,checks))
}
#[cfg(test)]
mod owner_action_tests {
    use super::explicit_implementation_request;

    #[test]
    fn korean_implementation_imperatives_require_real_action() {
        for text in [
            "내말잘듣는새끼로구현해라", "이 기능을 고쳐라", "Rust에 적용해라",
            "커넥터를 반영해라", "프론티어급으로 완성하라고",
            "에이전트 성능 개선하라", "구조를 고도화해주세요",
        ] {
            assert!(explicit_implementation_request(text), "{text}");
        }
    }
    #[test]
    fn discussing_a_design_does_not_force_a_source_mutation() {
        for text in [
            "어떻게 구현할지 판단하라고", "구현 방법을 알려줘",
            "구현하는 방식의 장단점", "구현하면 성능이 좋아지나요",
            "개발하는 법을 설명해", "고치면 어떤 일이 생기나",
            "이 설계의 문제는 뭐지",
        ] {
            assert!(!explicit_implementation_request(text), "{text}");
        }
    }
}

pub(super) fn evidence_for_source(state:&AppState,session:&str,source:&str,execution:&str)->Vec<Value>{
    let mut runs=std::collections::HashSet::new();runs.insert(execution.to_string());
    // Earlier native room speakers may already have performed this exact
    // owner's request. Their persisted operations prevent duplicate execution.
    for message in state.store.messages(session){if message["metadata"]["replyToMessageId"].as_str()==Some(source){if let Some(run)=message["metadata"]["executionId"].as_str(){runs.insert(run.to_string());}}}
    state.store.all("execution_operation").into_iter().filter(|operation|operation["status"]=="completed"&&operation["runId"].as_str().is_some_and(|id|runs.contains(id))).collect()
}
