# DOMINION Sub1 — 빠른 소스 탐색

**목표:** 모델 가중치를 고정한 API 기반 RSI. 개선 대상은 도미니언의 소스·하네스·알고리즘·기억·도구·경험 학습·개선 방법과 후손의 재개선 능력이다. LoRA, PEFT, 파인튜닝, 모델 재학습, 사용자 PC의 대규모 추론은 작업 범위가 아니다.

## RSI 수정 시 읽는 순서

1. **요청/상태:** `src/api/rsi.rs`, `src/engine/jobs.rs`
2. **실행·계승 중심:** `src/engine/rsi_evolve.rs`는 조정자입니다. 실제 캠페인 반복은 `src/engine/rsi/execution.rs`, 재컴파일 후손의 계획·실행은 `src/engine/rsi/worker.rs`, 계승 방법은 `rsi/method.rs`, 연구 기록은 `rsi/research.rs`, 토큰 예산은 `rsi/budget.rs`, 방법 프로그램은 `rsi/programs.rs`, 관측은 `rsi/telemetry.rs`부터 읽습니다.
3. **후손의 구조 인식·소스 후보:** `src/engine/rsi_system.rs`는 재노출용 진입점입니다. 실구현은 `src/engine/rsi/system/`의 inventory(실제 소스 해시), blueprint(구조도), discovery(연구·코드 분류), frontier(수정 후보), agenda(후손 작업 계획), design(실제 시스템 설계 등록)에서 확인합니다.
4. **개선 방법 자체의 진화:** `src/engine/rsi_meta_controller.rs`는 진입점입니다. 실제 개선기는 `src/engine/rsi/meta_controller/`의 provenance(채택 증거), reciprocal(A→B→A), selection(선택 정책), transfer(계보 전이), feedback(예측 검증·후손 지시)에서 변경합니다. 실모델에게 가리키는 소스 경로도 새 모듈을 사용해야 합니다.
5. **전체 능력의 상호 개선:** `src/engine/rsi_whole_agent.rs`, `src/engine/rsi_causal_ecology.rs`, `src/engine/rsi_recursive_laboratory.rs`
6. **계승되는 개선 원리:** `src/engine/rsi_principle_learner.rs`, `src/engine/rsi_edit_genome.rs`
7. **독립 검증·폐회로:** `src/engine/rsi.rs`의 후보 영수증, `src/engine/rsi_worktree.rs::classify/compare_at`의 동결 A/B, `src/engine/rsi/proof.rs`의 메타→능력→메타 3세대 검증, `src/api/rsi.rs::apply_verified_capability`의 API 원본 영수증 재검증, `src/engine/rsi_archive.rs`의 참고 계보.

기능 변경 전에는 **호출자 → 실제 실행 → 독립 평가 → 채택된 후손의 재실행** 경로를 추적한다. 독립 평가기 `rsi.rs`, `rsi_worktree.rs`, `rsi_metrics.rs`, `rsi_profile.rs`, `rsi/proof.rs`는 후손 후보가 수정하는 경로에서 제외하고 메타개선기·자기 모델은 계속 진화시킨다. 모델이 말한 성공과 독립 검증된 코드 채택을 혼동하지 않는다. 새 메커니즘은 하네스·알고리즘·기억 등 전체 지능으로 전이될 수 있어야 한다.

## 읽기·실행 비용 억제

- 소스는 파일명으로 좁힌 뒤 함수명·호출부를 먼저 검색한다. 채팅 턴 실행은 `src/engine/chat/runtime/turn.rs`, 기존 런타임 계약은 `src/engine/chat/runtime.rs`를 읽는다. 테스트는 `tests/support/engine/`과 `tests/support/e2e/`에서 책임별로 확인한다.
- `docs/DOMINION-HARNESS-MASTER-DESIGN.md`는 설계 배경, `docs/rsi-completion.md`는 평가 기준이다. `benchmark/`와 `docs/research/`는 필요할 때만 실험 근거로 연다.
- 과거 진행 로그와 중복 실모델 실험 스크립트는 활성 트리에서 정리했으며, 이전 버전은 Git 이력으로 확인한다.
- 일반 CI는 `.github/workflows/ci.yml`. 서브1의 모델·계보 실험 워크플로는 **명시적 수동 실행**만 허용한다. 코드를 읽거나 푸시하는 행위만으로 비싼 모델 추론을 시작하지 않는다.
- 모델 학습이 아니라 **시스템 경험·검증된 개선 방법의 축적과 재사용**을 설계한다.

## 구조적 계약

- 하위 모듈의 공개 인터페이스는 상위 조정자에서 명시적으로 연결한다. 내부 증거·평가·실행에 대한 책임은 섞지 않는다.
- 테스트는 원래 호출 계약과 시나리오를 유지하며 이동한다. 상대 경로 `include_str!`는 이동한 실제 소스 위치에 맞춰 갱신한다.
- 새 실행 로직을 다시 `rsi_evolve.rs`나 `chat/runtime.rs`에 몰아넣지 않는다. 기능 변경 시 기존 모듈을 먼저 수정한다.

## Sub1 coordinator file map (2026-11-13)

In `src/engine/rsi_evolve.rs` the active compiled helpers are split into
`rsi/selection.rs` (strategy), `rsi/source_control.rs` (source/budget/choice),
`rsi/focus.rs` (evidence and source focus), `rsi/adoption.rs` (verified adoption),
and `rsi/descendant_plan.rs` (rebuilt child planning). Follow the API →
jobs → run → execution → compiled worker_plan → native_generation_plan →
paired worktree → adoption → new executable → proof call path. External
function exports and the original tests include remain at the parent level.

**Paired evaluator map:** `src/engine/rsi_worktree.rs` re-exports
`rsi/worktree/candidate.rs` (worktree/checks/repair),
`rsi/worktree/evaluation.rs` (paired evaluator/receipt), and
`rsi/worktree/promotion.rs` (confirmed branch apply).
Their tests are isolated in `rsi/worktree/tests.rs`.

**Method evolution map:** `rsi/programs.rs` routes to
`rsi/programs/action.rs` (tool traces and recovery plans),
`rsi/programs/selection.rs` (observed execution and comparable programs),
and `rsi/programs/transfer.rs` (evidence-backed mutation operator inheritance).
The compiled worker uses the original `rsi_evolve` method exports.

**Immutable instrumentation:** `rsi::safe_path` excludes
`src/engine/rsi/worktree/` along with legacy evaluator sources.
`rsi::improver_source_path` permits the evolvable
`rsi/selection.rs`, `rsi/descendant_plan.rs`, `rsi/programs/*`
and other safe compiled improvement modules. Keep these independent:
an editable method must never become the verifier of its own success.

**Observed method feedback:** `rsi/method_feedback.rs` checks authentic predecessor source and compiled B job before the next meta rewrite is proposed. Only `rsi/proof.rs` certifies source adoption.
