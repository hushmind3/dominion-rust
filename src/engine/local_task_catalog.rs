//! 2026 task-aware provider tool disclosure. The original Run, policy,
//! approval journal and full tool catalog remain the sole execution authority.
//! A trusted one-file output contract initially advertises only the concrete
//! direct read/edit/verify tools. After failed attempts the larger tool catalog
//! is restored automatically. This is NOT a second planning agent.
use crate::domain::Run;
use serde_json::Value;
const LOCAL:&[&str]=&[
    "list_files","read_file","write_file","edit_file",
    "search_files","git_status","git_diff","shell","tool_search",
];
fn small_local_task(run:&Run)->bool{
    if run.repository.is_none()||run.read_only||run.model_only
        ||run.required_changed_paths.len()!=1||run.parent_run_id.is_some()
        ||run.instruction.chars().count()>1800
        ||run.steps>=4{return false;}
    let text=run.instruction.to_lowercase();
    // User requests requiring external facts, connected account actions or
    // multi-repository operations must retain the full catalog.
    !["http://","https://","research","browse","external api",
        "search the web","github issue","mcp","웹 검색","인터넷",
        "깃허브 이슈","외부 서비스"].iter().any(|needle|text.contains(needle))
}
pub fn project(run:&Run,tools:Vec<Value>)->(Vec<Value>,usize){
    if !small_local_task(run){return (tools,0);}
    let total=tools.iter().map(|t|t.to_string().len()).sum::<usize>();
    let kept=tools.into_iter().filter(|tool|{
        tool.pointer("/function/name").and_then(Value::as_str)
            .is_some_and(|name|LOCAL.contains(&name))
    }).collect::<Vec<_>>();
    let retained=kept.iter().map(|t|t.to_string().len()).sum::<usize>();
    (kept,total.saturating_sub(retained))
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::{domain::CreateRun,tools};
    fn task()->Run{
        let mut run=Run::new(CreateRun{
            instruction:"Edit sum.py so it prints 15".into(),
            budget_krw:Some(10.0),max_steps:Some(8)
        },8).unwrap();
        run.repository=Some("fixture".into());
        run.required_changed_paths=vec!["sum.py".into()];
        run
    }
    #[test]
    fn one_file_owner_contract_advertises_real_edit_tools_not_unnecessary_program_dags(){
        let run=task();
        let full=tools::definitions(true);
        let (visible,saved)=project(&run,full.clone());
        let names=visible.iter().filter_map(|v|
            v.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"git_diff"));
        assert!(names.contains(&"shell"));
        assert!(!names.contains(&"run_program"));
        assert!(!names.contains(&"spawn_subagent"));
        assert!(saved>0);
        assert!(visible.len()<full.len());
    }
    #[test]
    fn failed_attempts_reexpose_full_catalog_without_permanent_tool_loss(){
        let mut run=task();
        let full=tools::definitions(false);
        assert!(project(&run,full.clone()).1>0);
        run.steps=4;
        assert_eq!(project(&run,full.clone()).0,full);
        run.steps=0;
        run.instruction="Research https://example.com then update sum.py".into();
        assert_eq!(project(&run,full.clone()).0,full);
        run.instruction="Edit sum.py".into();
        run.required_changed_paths.push("other.py".into());
        assert_eq!(project(&run,full.clone()).0,full);
        run.required_changed_paths.pop();
        run.repository=None;
        assert_eq!(project(&run,full.clone()).0,full);
    }
}
