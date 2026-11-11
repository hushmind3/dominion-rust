# DOMINION — Sub3 인수인계 (2026-11-11)

## 역할·브랜치·기준
- 역할: Sub3 개발자. 담당 브랜치 **`dominion-sub3`**만 수정한다.
- 이번 시작 기준: `b9d2598cd166045fed6980df1419fcd3e5c7b451` (기존 CI 성공).
- 구현 커밋: `c3d05b2`(실패 피드백 기반 정책 실험), `2d6e1b5`(비교 대상 고정), `8ddf66d`(기존 테스트 컴파일 수정).
- 공통 원본: 프로젝트 `DOMINION_COMMON_SOURCE.md`의 모델 비종속·실행 완주·협력·기억 일반화·자기개선·세대 계승. 검사 항목 증가가 목표가 아니며 실제 개선 실행이 우선이다.

## 핵심 미완성 능력과 이번 세션의 판단
- 최근 심판 피드백: 연속 3세대 실질 개선과 인과적 holdout 증명이 가장 부족하다. 연구와 정책 제안이 실제 실행으로 이어지지 않는 문제를 우선 선정했다.
- 기존 Sub3는 유효한 paired holdout 성과가 이미 있어야만 다음 탐색식을 만들었고, 성과 없는 1세대에서 일찍 중단했다.
- `research_continuation`이 대기 중인 탐색 정책 실험을 선점했고, 평가가 없는 제어 세대가 계속 반복되는 경우 후보가 **등록만 되고 실제로 실행되지 않았다**.
- 비교 제어를 만든 뒤 후보 점수가 바뀌어 다른 영역을 선택하면 실험이 영원히 비교 불가능해지는 문제도 있었다.

## 실제 구현
1. **실패 세대에서 새로운 실행 가능한 정책 생성** (`rsi_evolve.rs`):
   - 서로 다른 실제 `jobId`, 실패 결과, `tokens>0`, 새 발견 없음 등에서 대체 `selfimprove.search` 계산식을 합성한다.
   - 이미 검증된 paired holdout 이득이 있으면 기존 성과 기반 계산식을 우선한다.
   - 둘 다 동일한 `queue_search_trial` 경로를 지나며 **가설 등록은 성과 증명이 아니다**.
2. **차세대 실험 실제 실행 경로**:
   - 미완료된 탐색 정책 후보가 있는 경우 무한 연구 지속보다 정책 제어/시험을 먼저 실행한다.
   - 같은 생성기 커밋·같은 대상에서 실험 가능한 제어가 없으면 측정용 제어를 실행한다.
   - 평가 가능한 제어가 끝내 나오지 않을 때에는 실패 제어 2회 이후 후보 탐색 정책을 실제로 1회 실행한다. 이 성과는 독립 paired 비교 없이 채택되지 않는다.
   - 제어에서 선택했던 도메인을 `controlForCandidateId`/revision으로 고정해 다음 후보 실행과 같은 문제 영역에서 비교한다.
   - 자율 실행은 무작정 반복되지 않으며 회복을 위한 최대 6 연속 무진전 세대 제한이 있다. 비자율 모드는 기존 조기 종료를 유지한다.
3. **회귀 테스트 추가**:
   - `autonomous_barren_generations_execute_their_evolved_policy_without_false_promotion`: 실패 누적 → 정책 합성 → 동일 영역 제어 → 후보 탐색 실행 → 증거 없는 승격 차단.
   - `autonomous_recovery_budget_is_bounded_but_does_not_stop_before_trial`: 실제 실험 전에 종료되지 않으면서 6회에 종료.
   - 기존 3인자 종료조건 테스트 7곳을 5인자 시그니처로 갱신.

## 다른 브랜치에서 직접 참고한 장점
- Sub1 `780b456`: 캠페인 간 미완성 연구를 구체적인 실행 계획으로 인계, 실패한 패치를 대상 파일에서 교정.
- Sub2 `7cee3e7`: 무생산 제어가 반복될 때 신규 정책 탐색의 실험 기회를 확보하는 접근.
- Sub4 `8bc72ac`: 검증된 방법의 업무/도메인 간 전이와 실패한 전이의 피드백 보존.
- Champion `401cd0f`: frozen holdout receipt와 세대 계보 보존, 자기개선 주장에 대한 독립 측정.

## 직접 확인한 실행 결과 / 검증 제한
- 기존 기준 커밋 `b9d2598`: GitHub Actions `42928968277` **success**.
- 첫 구현 커밋의 GitHub Actions `42934339145`: **failure**, Rust test 컴파일에서 종전 테스트 7개가 수정된 함수 인자 수와 불일치(E0061). 원인을 찾아 `8ddf66d`로 모두 수정.
- 수정 직후 GitHub Actions [`42934891498`](https://github.com/hushmind3/dominion-rust/actions/runs/42934891498): **SUCCESS**. npm ci / 타입 검사 / 웹 빌드 / bash 검사 성공, `cargo test --release` 성공: 16 benchmark + 621 unit + 3 + 99 e2e = **739개 통과, 실패 0개**. 새 `autonomous_barren_generations_execute_their_evolved_policy_without_false_promotion`, `autonomous_recovery_budget_is_bounded_but_does_not_stop_before_trial`, 기존 `goal_capability_loop_inherits_research_adopts_and_applies_next_decision`도 모두 실제 CI에서 성공.
- 테스트 코드와 실제 GitHub CI를 사용했다. **실제 유료 모델 캠페인에서 3세대 연속 코드 품질 개선을 실행한 것으로 주장하지 않는다.**
- 이 채팅 실행 컨테이너에는 `cargo`/`rustc`가 없어 로컬 Rust 직접 빌드가 불가하다. Rust 실행 검증 경로는 GitHub Actions다.

## 다음 세션이 바로 해야 할 일
1. 통과한 전체 CI `42934891498`의 회귀 기준을 유지할 것. 새 변경의 핵심 과제는 수동 단위 테스트 이후 **전체 캠페인에서 실험 후보가 정말 실행되는지** 증명하는 것이다.
2. 실제 모델 호출 없이도 6세대 이상의 전체 작업 큐/DB/바이너리 경유로 **실패 피드백 → 새 정책 실행**이 관측되는 제어된 e2e fixture를 추가하고, `generation.domainSelection`, `searchStrategy.id`, 선택한 파일, 변경 내용과 `trialOutcome`을 끝까지 확인할 것.
3. 실제 모델과 고정된 패치/holdout 평가로 3세대 연속 채택 + 계승 + 실사용 개선을 검증할 것. 결과가 없으면 부풀리지 말고 원인을 분리할 것.
4. 캠페인 간 반복 무생산을 줄이도록 Sub1의 실행 가능한 연구 인계, Sub4의 전이 학습을 실제 세대 검색 경로에 접목할 것. 채택 조건은 Champion의 불변 paired 증거 기준을 참고할 것.
5. 다른 Sub 또는 Champion 브랜치는 **수정하지 말고** 자신 브랜치에만 구현할 것.

## 실패 재발 방지
- `should_stop_no_progress` 함수 시그니처 변경 시 기존 테스트 호출도 반드시 함께 업데이트할 것.
- 새 정책이 실제 사용되기 전에 후보를 활성화하거나, 빈 제어를 paired 이득으로 간주하거나, 학습 자료 수만으로 개선 성공을 주장하지 말 것.
- 생성기 커밋과 선택 도메인, 실제 사용한 정책 revision이 서로 어긋나면 실험 성과 귀속이 불가능해진다.
