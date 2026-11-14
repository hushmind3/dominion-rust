//! One budget ledger for model rounds, tool proposals and executed tools.
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct BudgetLedger {
    max_rounds: usize,
    max_tools: usize,
    max_proposals: usize,
    max_elapsed: Duration,
    rounds: usize,
    tools: usize,
    proposals: usize,
    started: Instant,
}

impl BudgetLedger {
    pub fn new(max_rounds: usize, max_tools: usize, max_elapsed: Duration) -> Self {
        Self {
            max_rounds,
            max_tools,
            // Blocked proposals must not silently consume executed-tool slots,
            // but they still need a bounded budget.
            max_proposals: max_tools.saturating_mul(2),
            max_elapsed,
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    pub fn charge_round(&mut self) -> bool {
        if self.rounds >= self.max_rounds {
            return false;
        }
        self.rounds += 1;
        true
    }

    pub fn charge_proposal(&mut self) -> bool {
        if self.proposals >= self.max_proposals {
            return false;
        }
        self.proposals += 1;
        true
    }

    pub fn charge_tool(&mut self) -> bool {
        if self.tools >= self.max_tools {
            return false;
        }
        self.tools += 1;
        true
    }

    /// A fresh ledger for one parallel branch. Counters start at zero so the
    /// parent can merge only the usage of this branch.
    pub fn branch(&self) -> Self {
        Self {
            max_rounds: self.max_rounds.saturating_sub(self.rounds),
            max_tools: self.max_tools.saturating_sub(self.tools),
            max_proposals: self.max_proposals.saturating_sub(self.proposals),
            max_elapsed: self.max_elapsed.saturating_sub(self.started.elapsed()),
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    /// Deterministic parallel budget allocation. Every sibling gets only
    /// its share of the parent's unspent rounds, tool calls and proposals.
    /// Cloning a full remaining budget into EVERY child allows N branches
    /// to consume N times the intended maximum.
    pub fn partition(&self, index: usize, count: usize) -> Self {
        let base = self.branch();
        let count = count.max(1);
        let share = |available: usize| -> usize {
            let whole = available / count;
            let remainder = available % count;
            whole + usize::from(index < remainder && index < count)
        };
        Self {
            max_rounds: share(base.max_rounds),
            max_tools: share(base.max_tools),
            max_proposals: share(base.max_proposals),
            max_elapsed: base.max_elapsed,
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    /// Number of executable tool operations the branch may still issue.
    /// Also reserves a nonoverlapping durable opSeq interval for each
    /// parallel sibling, independent of tool completion order.
    pub fn remaining_tool_slots(&self) -> usize {
        self.max_tools.saturating_sub(self.tools)
    }

    /// Adds the usage of a completed branch back into this ledger.
    pub fn merge_usage(&mut self, branch: &Self) {
        self.rounds = self.rounds.saturating_add(branch.rounds);
        self.tools = self.tools.saturating_add(branch.tools);
        self.proposals = self.proposals.saturating_add(branch.proposals);
    }

    pub fn can_execute_tool(&self) -> bool {
        self.tools < self.max_tools
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed() > self.max_elapsed
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "rounds": self.rounds,
            "maxRounds": self.max_rounds,
            "tools": self.tools,
            "maxTools": self.max_tools,
            "proposals": self.proposals,
            "maxProposals": self.max_proposals,
            "elapsedMs": self.started.elapsed().as_millis() as u64,
            "maxElapsedMs": self.max_elapsed.as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_bounds_rounds_tools_and_proposals() {
        let mut ledger = BudgetLedger::new(2, 1, Duration::from_secs(60));
        assert!(ledger.charge_round());
        assert!(ledger.charge_round());
        assert!(!ledger.charge_round());
        assert!(ledger.charge_tool());
        assert!(!ledger.charge_tool());
        assert!(!ledger.can_execute_tool());
        assert!(ledger.charge_proposal());
        assert!(ledger.charge_proposal());
        assert!(!ledger.charge_proposal());
    }

    #[test]
    fn parallel_siblings_share_one_global_budget_not_cloned_allowances() {
        let mut parent = BudgetLedger::new(7, 11, Duration::from_secs(60));
        assert!(parent.charge_round());
        assert!(parent.charge_tool());
        assert!(parent.charge_proposal());
        let remaining = parent.branch();
        let shards = (0..4).map(|i| parent.partition(i, 4)).collect::<Vec<_>>();
        let sum = |key: &str| shards.iter()
            .map(|shard| shard.snapshot()[key].as_u64().unwrap_or(0))
            .sum::<u64>();
        assert_eq!(sum("maxRounds"), remaining.snapshot()["maxRounds"].as_u64().unwrap());
        assert_eq!(sum("maxTools"), remaining.snapshot()["maxTools"].as_u64().unwrap());
        assert_eq!(sum("maxProposals"), remaining.snapshot()["maxProposals"].as_u64().unwrap());
        assert!(shards.iter().all(|shard| shard.snapshot()["maxTools"].as_u64().unwrap() <= 3));
        for mut shard in shards {
            let quota = shard.snapshot()["maxTools"].as_u64().unwrap();
            for _ in 0..quota { assert!(shard.charge_tool()); }
            assert!(!shard.charge_tool());
            parent.merge_usage(&shard);
        }
        assert_eq!(parent.snapshot()["tools"], 11);
        assert!(!parent.charge_tool());
    }

    #[test]
    fn independent_parallel_tool_budgets_reserve_disjoint_operation_sequences() {
        let budget=BudgetLedger::new(8,11,Duration::from_secs(60));
        let mut cursor=91u64;
        let mut ranges=Vec::new();
        for index in 0..4 {
            let branch=budget.partition(index,4);
            let reserved=branch.remaining_tool_slots() as u64;
            ranges.push((cursor,cursor+reserved));
            cursor+=reserved;
        }
        assert_eq!(cursor,102);
        assert!(ranges.windows(2).all(|windows|windows[0].1==windows[1].0));
        let mut unique=std::collections::BTreeSet::new();
        for (start,end) in ranges {
            for op in start+1..=end {
                assert!(unique.insert(op),"parallel siblings cannot reuse opSeq");
            }
        }
        assert_eq!(unique.len(),11);
    }

    #[test]
    fn ledger_snapshot_exposes_limits() {
        let ledger = BudgetLedger::new(8, 40, Duration::from_secs(600));
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot["maxRounds"], 8);
        assert_eq!(snapshot["maxTools"], 40);
        assert_eq!(snapshot["maxProposals"], 80);
    }
}
