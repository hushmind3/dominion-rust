# DOMINION Sub2 — Editable Experiment Engine Architecture

**Date:** 2026-11-13 · **Branch:** `dominion-sub2` · **Evidence type:** source design only, no build/tests/CI or paid model run.

## 1. Runtime layers and source owners

| Layer | Owner / main files | Authority |
|---|---|---|
| Intelligence | `src/engine/{chat,memory,models,tools,algorithms,experience}.rs` | Model calls, task solving, memories and tools |
| Mutable task/improvement procedures | `rsi/planner/{compiled,operator_policy,edit_policy,actions}.rs`, `rsi_open_program.rs`, `rsi_meta_controller.rs`, `rsi_decision_rules.rs` | Newly compiled descendant chooses source-bound goals, rules, model routes, experimental operators |
| Independent execution | `rsi/execution.rs`, `rsi/execution/{generator,compiled_plan}.rs`, `rsi/worker.rs`, `rsi/worker/planning.rs` | Actual generation, separate plan process, live model/tool editing |
| Evidence/adoption | `rsi_worktree/{candidate,comparison}.rs`, `rsi/lineage.rs`, `rsi_system/history.rs`, `rsi/closure.rs` | Frozen paired evaluation, candidate/commit source fingerprints, revalidated history, full-recursion proof |
| Cross-boundary contract | `rsi/contract.rs` | Typed source/frozen-case/recipe identity; editable method cannot assign itself an evaluation score |

## 2. Actual source call graph

1. `src/api/rsi.rs::start` submits a continuous `rsi_evolve` job (`rsiSystemEvolution`, user-selected provider/model, frozen cases, budget).
2. `src/engine/jobs.rs` dispatches to `rsi_evolve::campaign::run`. The coordinator enters `rsi/execution.rs::run_loop` and owns attempt/budget boundaries.
3. `rsi/execution/generator.rs::ensure` builds the detached descendant commit and checks the actual compiled-binary identity; if the last independently adopted edit affected executable method code it preserves the old binary for comparison.
4. `rsi/execution/compiled_plan.rs::run` calls `--rsi-plan-generation` on the newly built binary. `rsi/worker/planning.rs` executes `compiled_system_generation_plan`, reading source graph + replayed history.
5. The compiled `rsi/planner/operator_policy.rs::select` chooses an experiment designer from source-bound program, principles, meta controller, inherited programs or topology exploration. Its reward observations are revalidated original source evaluations, not reported model success.
6. `rsi/contract.rs::derive` constructs `ExperimentContract {version,generatorCommit,campaignId,sourceTreeSha256,indexId,historyCount,targetDomain,focus,frozenSuiteSha256,recipeOwner,recipeRevisionSha256,recipeFamily,evaluatorBoundary}`. `compiled_plan.rs` verifies it against model/provider/source evidence, and populates actual child inputs.
7. `--rsi-worker-job` executes `rsi/worker.rs::worker`: validates the contract a second time, reconciles source focus, invokes the actual tool-using model. `jobs.rs` creates an isolated checkout; tools and code-index enforce source boundary.
8. `rsi_worktree::finish` captures diff/candidate; `rsi_worktree::compare_at` executes the frozen cases on candidate/base worktrees. Only eligible, independently verified candidates pass into `rsi/lineage.rs::adopt` for the next detached revision.
9. `rsi_system/history.rs::revalidated_method_history` reconstructs measured outcomes and worker-executed operator identity from candidates, jobs and frozen paired receipts. The next compiled descendant reads these observations for the next experiment-design policy.
10. `rsi/closure.rs::evidence` checks complete A→B→A′→B′ independently adopted, source-matched controls for the **full recursion milestone**. The campaign stop and API use `rsi::campaign_recursive_evidence`. This is not the ordinary research search stage.

## 3. Two different types of evolution

- **Task-system changes:** improve memory, tools, harness, models' *routing*, reasoning policies, code generation and knowledge reuse using source edits and subsequent descendants.
- **Improvement-method changes:** modify the compiled experiment chooser itself (`rsi/planner/operator_policy.rs`), model-authored experiment graphs, program interpretation, meta controller, edit rules and search policy. Source SHA inheritance makes the revised improvement mechanism a real input to later task changes.
- Every generation uses the same immutable evaluator for that experiment. The experiment recipe can change and select among independent hypotheses, but the evaluator cannot be edited by a candidate. Changes to evaluation methodology across future campaigns would require a separate verified version-migration authority.

## 4. Why this is not just another A→B→A′ graph

The experiment chooser is a compiled and editable algorithm whose recommendations affect **real source focus and domain**, and whose future selection is influenced by historical held-out outcomes. `experimentOperator` is written only when the native worker actually focused that proposal. `history.rs` removes unbound/self-claimed operator outcomes before they train the next choice. A 4-stage recursive closure is a strict certification lens, not a hardcoded sequence that controls which experiment hypothesis is tried next. Some inherited transfer/repair programs still have stricter evidence-specific routing; the generalized chooser is a step toward a completely composable operator graph rather than a claim to already have one.

## 5. Remaining structural deficits

- `rsi/execution.rs` still coordinates a large source-evaluation/learning finalization tail; continue splitting into typed result objects for source capture, frozen measurement and commit selection.
- The backend store, `AppState` and shared `Value` persist outside the newly typed experiment contract. Replace them incrementally with explicit read-only evidence ports and generation result types.
- The operator selector begins with a limited set of proposal producers. Truly arbitrary new executable operator types need a source-bound discovery/validation registry rather than manual candidate registration.
- No real four-generation plus matched-control success has been measured. No compile/build/test/benchmark was run during this source-design session.

## 6. Current 2026 reference context (design principles, not copied code)

- Meta AI, [HyperAgents](https://ai.meta.com/research/publications/hyperagents/), 2026-03-24 — improvement procedure itself is mutable.
- [MetaSkill-Evolve](https://arxiv.org/abs/2607.05297), 2026-07-06 — task and meta-skills evolve at two timescales with a fixed model backbone.
- Microsoft Research, [Rethinking Self-Evolving Agents / Open-Ended Optimization](https://www.microsoft.com/en-us/research/publication/rethinking-self-evolving-agents-do-we-still-need-prescribed-optimization-pipelines/), 2026-08 — mutable optimizer controls the route, immutable evaluator/resources bound the experiment.
- [RSIHub](https://arxiv.org/abs/2610.32253), 2026-10-23 — composable operator recipes, source lineage, frozen evaluations and independent selection.

## CI 전 감사: 선택기 학습의 실제 실행 귀속

실행 정책이 선택한 파일을 `worker`에서 단순 focus로 지정했다고 성과를 인정하면 실제 코드 편집 없이 정책을 강화할 수 있습니다. `rsi_system/history.rs::replayed_experiment_operator`가 **원본 `rsi_candidate`의 소속 세대, model-driven origin, 변경된 `paths`, 원본 도구 호출의 `written` 경로**를 요구하도록 강화했습니다. 이후에만 기존 원본 paired 평가 재생과 연결됩니다. `rsi/contract.rs` 단위 테스트는 변조된 동결 평가 및 누락된 소스 편집 대상의 실패도 다룹니다. 사용자 요청에 따라 `CI`는 GitHub Actions의 `push`로 실행하며 실제 상태는 실행 기록을 기준으로 표시합니다.

## 실행기 결합도 추가 해소

`rsi/execution.rs`에서 개선법·후손 선택 결과 분석을 `execution/learning_feedback.rs::assess`로, 영속화/원본 증명 통합을 `execution/proof_ledger.rs::persist`로 분리했습니다. 전자는 독립 평가와 실제 도구 영수증만 읽는 평가/학습 후처리이며 후자는 세대 기록 저장, 공통 증명기 호출, 다음 후손 생산성 결과 보관에 대한 경계입니다. 캠페인 실행기는 이 두 구체적인 모듈을 순서대로 호출합니다. 타입 계약과 독립 평가 자체는 여전히 구분되며, `execution.rs`가 다루는 보관/증명 책임이 줄었습니다.

## 정적 도메인 enum 밖의 소스 기반 실험 후보 자동 발견

`rsi/planner/operator_policy.rs::select`는 컴파일 후손의 `architectureFrontier` 중 유효한 소스 SHA와 안전 경로를 가진 항목을 전수 확인하고, `(role,path)`를 SHA-256으로 안정적으로 식별한 `source_graph_*` 후보를 동적으로 생성합니다. 그 후보는 다른 제안자와 동일한 원본 paired 평가·실행된 소스 편집 증명·퇴행 벌점을 받습니다. 모델이 별도 신규 지능 역할을 발명해 소스에 채택했을 때, 고정된 첫 8개 역할에 한정해 연구하지 않고 후속 세대가 그 새 역할을 개선 대상으로 선정할 수 있습니다. 각 후보에 보조 cold-start 우선순위를 부여하며 명시적 모델 작성 프로그램과 기존 메타 계승을 우회하지 않습니다.

또한 `rsi_mutation.rs::stream_search_policy_statistics`가 이전 평면 모듈이 아닌 현재 실행 경로인 `rsi/search.rs`에서 전체 공개 범위(`pub(super)`)를 보존한 채 다음 개선 알고리즘을 생성하도록 소스 패턴을 다시 맞췄습니다. `rsi_principle_lab.rs::instruction`도 베이스 모델 학습이나 어댑터 변경을 요구하지 않고 고정 가중치의 추론·검색·도구 정책을 개선하도록 수정했습니다.

### 개선법 파일 소유권과 계승 경계

`rsi_system/method_lineage.rs::executable_method_source`가 실제 소스 변경 후보 분류와 후손 method inheritance 영수증의 공통 경계입니다. 이제 현재 실행 중인 `rsi/search.rs` 및 원리·편집 규칙 생성기들이 이 목록에 포함됩니다. 새로 채택된 `rsi/planner/*` 및 `rsi/learning/*` 소스는 컴파일 런타임 파일·안전한 경로가 확인되면 즉시 후보가 됩니다. 계승 확인은 고정 목록을 도는 대신 `systemDesign.targetFiles`에 명시된 독립 채택 파일을 순회하여 과거/현재 SHA를 대조합니다. 동결 증명기·원본 평가 계약은 후보 범위 밖에 남습니다.
