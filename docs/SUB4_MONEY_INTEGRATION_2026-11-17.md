# Sub4 Money integration (2026-11-17)

## Source of integration
Actual comparison: money branch src/engine/money_intelligence.rs, money_mechanism.rs, money_strategy.rs, money_sources.rs, and src/api/money_find.rs. Market classification, economic pairing operators, source rotation and one-action discovery were adapted into Sub4.

The money branch's old framework, separate job/DB layer, existing UI, retired media/RSI and provider loops were NOT merged. Native Engine, journal, OAuth, Rust SDK and React design remain authoritative.

## Durable flow
1. RSS/Atom intake: compiled-in 23 public sources across Korean, Chinese, Japanese and Western languages. Three feeds per scan. No arbitrary URL fetch, automatic redirect, model-token expenditure or external action.
2. SQLite money.sqlite records source IDs/links, title, publisher, observed tags and capture/publication fields. Writes are transactional and source IDs deduplicated.
3. A bounded query of 180 recent source rows performs cross-sector economic tension matching. Independent publisher domains and change roles constrain two-source pairs. Single-source news is labeled a weaker research hypothesis, not proof of actual demand.
4. Three selected leads and report metadata save in one transaction. Prior rejected opportunities retain rejection. Each card shows buyer hypothesis, revenue mechanism, implementation, first paid test, falsification condition and original evidence links.
5. Optional model analysis is exactly one tool-free native Engine Run. API calls share the same provider routing, durable events, model accounting and restart non-replay policy. Generated ideas must reuse captured source IDs; unverified claims are not promoted to sales.
6. Thirty-minute collector mode is explicit opt-in, persists the preference, and contains no automatic paid inference. Business implementation remains a separate user decision.

## API and SDK
GET /api/money/find — latest saved report and native inference status; POST /api/money/find — mode db or agent
GET /api/money/stats — database counts and scan metadata; POST /api/money/collect — manual source scan; POST /api/money/automation — enabled boolean
POST /api/money/candidates/{id}/reject — reject a saved lead; POST /api/money/inference/cancel — cancel native model call
All endpoints also available under /api/v1/money/*. TypeScript sdk.money wraps native routes; Rust SDK exposes money_latest, money_signals, and money_collect.

## Current scope and gaps
Unlike a literal full-port, this does not yet include Money's buyer-document excerpt crawler, GitHub/Hugging Face metric collector, Korean bigram/FTS5 archive search, long-running paid inference campaign scheduler, actual payment ledger, or full profit/conversion experiment tracking. It records RSS summaries with lower evidence confidence. The observed source and economic inference are never claimed to be a verified sale.

Testing scope: Rust parser/semantic matching and durable SQLite tests; TypeScript authenticated SDK contract tests. Latest-HEAD CI outcome is a separate artifact from this code change, and 27B 24-hour throughput has not been measured.