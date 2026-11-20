//! Effect-free ReAct turn controller. This is replaceable independently
//! from the gateway, filesystem/DB persistence, remote tools and scheduler.
#![forbid(unsafe_code)]
use dominion_agent_core::{AgentLoop,LoopStep,TurnSnapshot};

#[derive(Debug,Default,Clone,Copy)]
pub struct ReActLoop;
impl AgentLoop for ReActLoop {
    fn name(&self)->&'static str{"react-2026-turn-controller"}
    fn next(&self,s:TurnSnapshot)->LoopStep {
        // Recovery/approval outrank model planning. A model is never called
        // while an already-authorized effect is pending settlement.
        if s.terminal||!s.running{return LoopStep::Stop}
        if s.approval_pending{return LoopStep::ResolveApproval}
        if s.program_ready{return LoopStep::AdvanceProgram}
        if s.has_queued_tools{return LoopStep::Dispatch}
        if !s.plan_exists {
            return if s.separate_planning {LoopStep::RequestPlan}
                else {LoopStep::FusePlan};
        }
        if s.steps>=s.max_steps{return LoopStep::MaxStepsExceeded}
        LoopStep::ModelTurn
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn default_state()->TurnSnapshot{
        TurnSnapshot{running:true,plan_exists:true,max_steps:10,..Default::default()}
    }
    #[test] fn pending_approval_is_never_bypassed_by_strategy(){
        let a=ReActLoop;
        let s=TurnSnapshot{approval_pending:true,has_queued_tools:true,
            plan_exists:false,steps:999,..default_state()};
        assert_eq!(a.next(s),LoopStep::ResolveApproval);
    }
    #[test] fn active_program_precedes_new_model_and_queued_tools(){
        let a=ReActLoop;
        let s=TurnSnapshot{program_ready:true,plan_exists:false,..default_state()};
        assert_eq!(a.next(s),LoopStep::AdvanceProgram);
    }
    #[test] fn queued_calls_resume_before_planning_and_step_limits(){
        let a=ReActLoop;
        let s=TurnSnapshot{has_queued_tools:true,plan_exists:false,
            steps:20,..default_state()};
        assert_eq!(a.next(s),LoopStep::Dispatch);
    }
    #[test] fn fused_vs_dedicated_planning_and_model_are_selected_without_io(){
        let a=ReActLoop;
        let base=TurnSnapshot{plan_exists:false,..default_state()};
        assert_eq!(a.next(base),LoopStep::FusePlan);
        assert_eq!(a.next(TurnSnapshot{separate_planning:true,..base}),LoopStep::RequestPlan);
        assert_eq!(a.next(default_state()),LoopStep::ModelTurn);
        assert_eq!(a.next(TurnSnapshot{steps:10,..default_state()}),LoopStep::MaxStepsExceeded);
    }
    #[test] fn stopped_or_terminal_run_never_schedules_effect(){
        let a=ReActLoop;
        assert_eq!(a.next(TurnSnapshot{running:false,has_queued_tools:true,
            ..default_state()}),LoopStep::Stop);
        assert_eq!(a.next(TurnSnapshot{terminal:true,approval_pending:true,
            ..default_state()}),LoopStep::Stop);
    }
}
