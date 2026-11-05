//! Action acceptance for ordinary turns. Only the actual request is examined;
//! examples, peer quotations and provider completion claims cannot prove work.
use crate::{engine::goals,state::AppState};
use serde_json::Value;

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
    if !(file_action&&file_target||command_action){return None;}
    let checks=goals::prepare_acceptance_checks(state,&request);Some((request,checks))
}
pub(super) fn evidence_for_source(state:&AppState,session:&str,source:&str,execution:&str)->Vec<Value>{
    let mut runs=std::collections::HashSet::new();runs.insert(execution.to_string());
    // Earlier native room speakers may already have performed this exact
    // owner's request. Their persisted operations prevent duplicate execution.
    for message in state.store.messages(session){if message["metadata"]["replyToMessageId"].as_str()==Some(source){if let Some(run)=message["metadata"]["executionId"].as_str(){runs.insert(run.to_string());}}}
    state.store.all("execution_operation").into_iter().filter(|operation|operation["status"]=="completed"&&operation["runId"].as_str().is_some_and(|id|runs.contains(id))).collect()
}
