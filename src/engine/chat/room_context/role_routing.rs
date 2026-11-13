//! Role-conditioned room evidence routing without extra model calls.
//! A relevant peer observation gets a small role-specific ordering boost,
//! never authority, and never overrides mandatory owner/peer context.
use super::*;
use crate::engine::memory;

pub(super) fn role_terms(state:&AppState, request:&TurnRequest)->BTreeSet<String>{
    let Some(agent)=request.agent_id.as_deref()
        .and_then(|id|state.store.get("agent",id)) else{return BTreeSet::new()};
    let mut description=String::new();
    for key in ["role","description","name","systemPrompt"]{
        if let Some(value)=agent[key].as_str(){
            // Long generated personas have no right to dominate evidence
            // retrieval. The same cap keeps selection O(bounded persona).
            description.extend(value.chars().take(450));
            description.push(' ');
        }
    }
    memory::terms(&description)
}

pub(super) fn overlaps(candidate:&BTreeSet<String>,role:&BTreeSet<String>)->usize{
    if role.is_empty(){return 0;}
    candidate.intersection(role).take(3).count()
}

pub(super) fn role_boost(query_score:f64, hits:usize)->f64{
    // Absent query relevance, persona similarity cannot resurface unrelated
    // old messages. It only helps allocate the remaining input budget.
    if query_score <= 0.0 {0.0} else {query_score * (hits.min(3) as f64) * 0.16}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn role_only_reranks_evidence_relevant_to_current_query(){
        let candidate=["architecture","tool"].into_iter().map(str::to_string).collect();
        let architect=["architecture"].into_iter().map(str::to_string).collect();
        let unrelated=["finance"].into_iter().map(str::to_string).collect();
        assert_eq!(overlaps(&candidate,&architect),1);
        assert_eq!(overlaps(&candidate,&unrelated),0);
        assert!(role_boost(2.0,1)>role_boost(2.0,0));
        assert_eq!(role_boost(0.0,3),0.0);
    }
}
