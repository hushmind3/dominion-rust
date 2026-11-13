# DOMINION Sub1 — 실제 RSI 호출 경로와 경계

2026-11-13 기준, `dominion-sub1` 활성 Rust 소스의 개편 구조를 설명합니다.
이 파일은 현재 코드에 연결된 경로만 기술하며 실행 검증 결과 보고서가 아닙니다.

## 1. 사용자 API에서 후손 실행까지

| 단계 | 실제 진입점 | 역할 |
| --- | --- | --- |
| 요청·선택 | `src/api/rsi.rs::start` | provider/model, 자동 시스템 진화, 대조 실험 기본값, 소스 기준 커밋 |
| 작업 실행 | `src/engine/jobs.rs::run_prompt`, `rsi_evolve::run` | 내구성 있는 작업 및 별도 생성 후보 worktree |
| 세대 조정 | `src/engine/rsi/execution.rs::run_loop` | 생성기 빌드·동일 커밋 계획·자원 예산·측정·채택 |
| 후손 계획 | `src/engine/rsi/worker.rs::worker_plan` → `rsi/descendant_plan.rs::native_generation_plan` | 재컴파일된 후손이 목표, 파일, 메타 정책을 직접 결정 |
| 원본 도구 편집 | `rsi/worker.rs::worker` → `jobs::run_prompt` → `chat/runtime/turn.rs` → `tools::execute` | 버전/경로 검사, 실제 모델 도구 실행, 작업 공간 수정 |
| 후보 실측 | `rsi_worktree::compare_at` → `rsi/worktree/evaluation.rs` | 원본 A/B 소스 빌드 및 동결 사례 평가 영수증 |
| 검증된 채택 | `rsi::verify_paired_receipt` → `rsi/adoption.rs::adopt` | 검증을 통과한 후보만 실험 worktree에 커밋, production ref 불변 |
| 세대 계승 | `rsi/method.rs`, `rsi/programs/*`, `rsi/research.rs`, `rsi/meta_controller/*` | 개선 프로그램·경험·탐색 정책 및 후손 방향 재설계 |

## 2. A→B→A2→B2 개선법 폐회로

1. **A — 개선법 수정:** `rsi/proof/method_lineage.rs::method_snapshot`은 독립 평가를 통과한 단일 실행 개선법 파일의 SHA-256, 채택 커밋, 원본 평가 영수증을 묶습니다.
2. **B — 변경된 후손 실행:** 실제 재컴파일 바이너리가 A 방법을 상속하고 다른 능력의 소스를 수정·평가합니다. `rsi/method_control.rs::run_method_control`은 같은 소스 커밋과 초기 작업 조건의 별도 OFF 워커를 실행합니다.
3. **B 대조군 검증:** `rsi::verify_control_receipt`은 원본 OFF 평가를 (실패/중립 포함) 다시 계산합니다. `rsi/proof/method_contrast.rs::evidence`는 OFF/ON 양쪽 원본 candidate/job, source commit, 사례·provider/model, 실제 방법 실행과 holdout 승패를 비교합니다. OFF 후보는 절대 채택하지 않습니다.
4. **C(A2) — 피드백으로 개선법 재수정:** `rsi/proof/method_lineage.rs::downstream_feedback`이 B의 *독립 대조 우위*를 검증하고 C의 계획에 보냅니다. `rsi/descendant_plan.rs`는 해당 SHA의 개선법 파일을 재수정 대상으로 고릅니다. `rsi/worker.rs`의 실제 모델 지시에 B control ID, 평가 SHA, 품질 승차와 수정 가설을 전달합니다.
5. **D(B2) — 수정된 개선법으로 재실행:** A2의 실제 소스를 이어받은 후손이 B 계열 능력을 다시 수정·평가하고 **새로운 별도 OFF/ON 실험**을 수행합니다.
6. **최종 판정:** `rsi/proof/full_cycle.rs::report`가 A/B/A2/B2 네 세대, 실제 worker, 두 개의 OFF/ON 영수증과 개선법 소스 동일성, 채택 및 holdout 결과를 다시 재생합니다. `execution.rs`의 캠페인 종료와 `api/rsi.rs::apply_verified_capability`의 공개 상태가 이 하나의 판정을 사용합니다. 3세대 증거는 `threeGenerationProof` 참고 정보만 제공합니다.

**실험 해석:** 대조군은 학습된 실행 방법과 유도된 action policy를 제거하고 같은 컴파일된 planner를 사용합니다. 따라서 측정 대상은 **방법 실행의 관측상 기여도**이며, 모든 소스 차이의 순수 인과효과는 아닙니다. 동일 조건 1회씩의 모델 변동성 때문에 `causalAblationProven=false`를 유지합니다. 실제 성공 캠페인 영수증은 이번 소스 수정만으로 생성되지 않습니다.

## 3. 독립 평가기와 수정 가능한 후손의 경계

- **동결:** `src/engine/rsi.rs`, `rsi_worktree.rs`, `rsi/worktree/*`, `rsi/proof.rs`, `rsi/proof/*`, `rsi_metrics.rs`, `rsi_profile.rs`의 검증 로직. `rsi::safe_path`가 후손 코드 편집에서 차단합니다.
- **수정 가능:** `rsi/meta_controller/*`, `rsi/system/*`, `rsi/descendant_plan.rs`, `rsi/programs/*`, `rsi/method_feedback.rs`, 기억·도구·하네스. 새 방법은 후손에 실제 편입된 뒤 독립 성과를 제출해야 합니다.
- **실험 독립:** `rsi_method_control` 원본 레코드의 비채택 상태, 별도 worktree, control session marker 및 `jobs::run_prompt`/chat finalizer의 연구·경험·전략 학습 억제. 양쪽 모델 토큰 한도는 유한 예산에서 동일하게 분배됩니다.
- **기반 모델:** API/로컬 모델 선택은 바뀔 수 있어도 가중치 학습/LoRA/파인튜닝 실행 경로는 RSI에 연결하지 않습니다.

## 4. 전체 플랫폼 모듈 분해

| 경계 | 구현 |
| --- | --- |
| 도구 | `tools.rs` → `tools/{dispatch,gateway,workspace,edit,process,network}.rs` |
| 채팅 턴 | `chat/runtime.rs` → `runtime/{rsi_guard,entry,adaptation,tool_selection,program,tool_dispatch,evaluation,finalization,session,turn}.rs` |
| 모델 제공자 | `chat/provider.rs` → `provider/{resolve,stream,protocols,codex,anthropic}.rs` |
| 모델 관리 | `models.rs` → `models/{installed,install,runtimes,storage,load,catalog,download}.rs` |
| RSI 코드 생성·채택 | `rsi_evolve.rs` → `rsi/{execution,worker,descendant_plan,method,method_control,selection,source_control,focus,adoption,programs}.rs` |
| 동결 평가 | `rsi_worktree.rs` → `rsi/worktree/{candidate,evaluation,promotion}.rs`; `rsi/proof.rs` → `rsi/proof/{method_lineage,method_contrast,full_cycle}.rs` |

테스트 파일은 `rsi/worktree/tests.rs`, `tools/tests.rs`,
`chat/provider/tests.rs`, `models/tests.rs`, `tests/support/engine/`의 기존
fixture를 중심으로 구분합니다.

## 5. 남은 구조적 제약

- 이번 세션에서 **테스트·컴파일·빌드·CI·벤치마크·유료 모델 실행은 수행하지 않았습니다**. 따라서 새 코드는 런타임 실행 검증 전 단계입니다.
- 단일 OFF/ON 페어는 모델 샘플링 변동과 공유 DB의 일부 간접 영향까지 완전히 제거하지 못합니다. 반복된 조건 실험, 백그라운드 상태 스냅샷, 원본 바이트 재검증 강화가 필요합니다.
- `rsi/execution.rs`와 `chat/runtime/turn.rs`의 남은 대형 책임, 시스템 메타 계획과 평가 기록 사이 결합도, 복구·취소 이후 통계 일관성은 추가 구조 점검 대상입니다.
- 4세대 폐회로 성공 영수증은 이번 소스 수정만으로 만들어지지 않습니다. 모델 학습·가중치 변경 경로 추가도 하지 않았습니다.

## 관련 2026년 공개 연구

- [AIDE² — Recursive self-improvement of AI research agents (2026-09)](https://arxiv.org/abs/2609.26457): 독립 평가 뒤 개선된 agent가 후속 agent가 되는 형태
- [Recursive Self-Improvement on a Budget (2026-10)](https://arxiv.org/abs/2610.29121): 한정된 개선 예산과 검증 가능한 선택
- [RRSI (2026-09)](https://github.com/google-research/rrsi): frozen model과 열린 하네스 개선 공간의 구분

## Adaptable experiment allocation contract

`experiment_policy::design` (editable and executed by compiled descendant) consumes typed source/target data and original generation observations. `experiment_policy::apply` changes `systemAgenda.target/focus`. The stable `proof/experiment_contract.rs::verify_experiment_design` validates immutable source hashes and allowed evaluator controls in parent and child. Actual `rsiMethodAblation` depends on the descendant-selected protocol and user authorization. A live generation records `experimentOutcome`; the next compiled policy observes that outcome and may rewrite **its own source** via the same ordinary model editing + frozen paired worktree + adoption route. This is an architectural live feedback cycle; there are no demonstrated successful multi-generation experimental results from source changes alone.

**Generation build boundary:** `src/engine/rsi/execution/build.rs::ensure_compiled_generator` owns only release binary creation and source-commit/SHA identity checks. `execution.rs::run_loop` calls this before each generation; no independent oracle/adoption logic moved to the build stage. A descendant planner and experiment allocator still run from the built binary.

### Stable experiment contract independent of planner and proof

The typed `rsi/contracts/experiment.rs` lives under an immutable `rsi/contracts.rs` composition root. `experiment_policy.rs` and `proof.rs` no longer import each other's private implementation. Both compiled descendant and supervisor call the same read-only `verify_experiment_design` through `rsi_evolve::contracts`. `rsi::safe_path` freezes the entire `rsi/contracts/` subtree; evolution modifies the experiment allocation algorithm, never its own source authentication or accepted evaluator.

### Policy provenance and cost-aware experimental consent

`verify_experiment_design` now receives actual persisted generation history (rather than a caller-supplied count alone), rejects an invented `parentExperimentRevision`, and does not import the parent's mutable domain selector. An experimental protocol requiring a real ON/OFF worker is selectable only when that control is enabled in the user's campaign request; supervisor and worker both reject mismatched plans. The frozen interface accepts future domain identifiers that are verified against the agenda by the existing orchestration layer instead of freezing today's domain taxonomy into the typed shared schema.

**Child execution evidence boundary:** `src/engine/rsi/execution/evidence.rs::collect_generation_evidence` reads original child job/session events, method tool traces, model usage, elapsed time, and recorded source edits. The parent loop receives the same `generation` record, then performs independent frozen comparison and adoption. This separates observation from selection, proof and mutation policy. It changes no weights and has not been compiled or tested.

## 2026 research informing this division

- [MetaSkill-Evolve (2026-07-06)](https://arxiv.org/abs/2607.05297): improve task procedures and the procedure-improving policy on different timescales while retaining one frozen foundation model. DOMINION's experiment allocator is compiled editable source rather than a new weight-training component.
- [Meta^n (2026-08-25)](https://arxiv.org/abs/2608.24735): meta-depth need not be a prescribed two-level chain. DOMINION's experiment target selection is not driven by its four-generation success-proof order.
- [AIDE² (2026-09-22)](https://arxiv.org/abs/2609.26457): inherited accepted code becomes the next improving agent; independent holdouts remain the gate for adoption.
- [Skill∞ (2026-10-26)](https://arxiv.org/abs/2610.35434): meta-procedures can be revised using execution traces and independent reviews. DOMINION's typed experiment decisions are source/revision-bound and auditable.

These works inform the architecture rather than constituting evidence that a four-generation DOMINION campaign has succeeded.
