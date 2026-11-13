# DOMINION Sub1 — 빠른 소스 탐색

**목표:** 모델 가중치를 고정한 API 기반 RSI. 개선 대상은 도미니언의 소스·하네스·알고리즘·기억·도구·경험 학습·개선 방법과 후손의 재개선 능력이다. LoRA, PEFT, 파인튜닝, 모델 재학습, 사용자 PC의 대규모 추론은 작업 범위가 아니다.

## RSI 수정 시 읽는 순서

1. **요청/상태:** `src/api/rsi.rs`, `src/engine/jobs.rs`
2. **실행·계승 중심:** `src/engine/rsi_evolve.rs`는 조정자입니다. 실제 세대 실행은 `src/engine/rsi/execution.rs`, 계승 방법은 `rsi/method.rs`, 연구 기록은 `rsi/research.rs`, 토큰 예산은 `rsi/budget.rs`, 방법 프로그램은 `rsi/programs.rs`, 관측은 `rsi/telemetry.rs`부터 읽습니다.
3. **후손의 구조 인식·소스 후보:** `src/engine/rsi_system.rs`
4. **개선 방법 자체의 진화:** `src/engine/rsi_meta_controller.rs`
5. **전체 능력의 상호 개선:** `src/engine/rsi_whole_agent.rs`, `src/engine/rsi_causal_ecology.rs`, `src/engine/rsi_recursive_laboratory.rs`
6. **계승되는 개선 원리:** `src/engine/rsi_principle_learner.rs`, `src/engine/rsi_edit_genome.rs`
7. **독립 소스 검증:** `src/engine/rsi.rs`, `src/engine/rsi_worktree.rs`, `src/engine/rsi_archive.rs`

기능 변경 전에는 **호출자 → 실제 실행 → 독립 평가 → 채택된 후손의 재실행** 경로를 추적한다. 모델이 말한 성공과 독립 검증된 코드 채택을 혼동하지 않는다. 새 메커니즘은 하네스·알고리즘·기억 등 전체 지능으로 전이될 수 있어야 한다.

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
