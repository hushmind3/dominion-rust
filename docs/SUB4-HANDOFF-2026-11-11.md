# DOMINION 4세대 Sub4 — 2026-11-11 인수인계

## 현재 세대 / 역할 / 브랜치 / 기준 커밋
- 역할: 4세대 Sub4 개발자
- 담당 브랜치: `dominion-sub4` (다른 Sub/Champion 브랜치 변경 금지)
- 시작점: `8bc72acd60f980900cfdaed9602f6700753bc7e0`
- 실제 구현: `e1e47d53b9a3eb58fe483f90454387a5860dc8a2`, `ab3ba64edeb37b369d0e210ab86eaa058083a81c`
- 공통 기준: `DOMINION_COMMON_SOURCE.md`, 핵심은 모델이 아니라 DOMINION 자체가 경험·전략·알고리즘을 계승해 목표를 완주하는 것

## 최우선 미완성 능력
21개 캠페인/41개 세대/채택 1회(기존 `docs/rsi-completion.md` 측정)로, 독립 holdout 이익과 3세대 연속 채택은 입증되지 않았다. 실행 연구의 반복, 모델이 알고리즘을 제안해야만 탐색이 진화하는 의존성, 다음 세대로 배운 절차를 연결하는 과정의 불일치가 병목이다.

## 실제 구현
1. `e1e47d5`: `src/engine/rsi_evolve.rs`에 `method_application_context` 추가. 실제 선택된 파일·분야와 사용자의 RSI 목표로 절차 적용 조건을 평가하며, `configure_method_session`에서 이전 세대의 `research_only` 선택을 현재 파일 인덱스 기준으로 재계산하고 `method_strategies`의 동일 문맥까지 연결했다. 선택된 파일이 바뀌어도 기존 결정에 갇히지 않는 테스트 추가.
2. `ab3ba64`: `adaptive_search_expression`과 `queue_adaptive_search_trial` 추가. 검증된 채택 기록과 `server_invoked_local_paired_git_worktrees` 출처, 평가 행의 `eligible`·holdout 이득·회귀 없음에 연결된 세대에서만 실측 보상을 계산한다. 실패율·토큰 비용까지 반영해 새 `selfimprove.search` 표현식을 만든 다음 기존 `queue_search_trial`에 등록한다. 이 경로는 새 LLM 호출을 요구하지 않으며 등록을 승격으로 간주하지 않는다.
3. `ab3ba64`: 자동 캠페인에서 실험 등록과 다음 세대의 후보 조회가 `selection_objective(input)`을 공통으로 사용하도록 고쳤다. 소스가 동일한 이전 캠페인의 기록도 정책 가설 생성을 위한 입력으로 사용한다. 실제 비교 평가·채택의 기존 조건을 유지했다.
4. Rust 테스트: 새로 선택된 파일의 재계획, 입력 변경 시 적용 해제, 검증된 holdout 가설 생성, 비용 기반 다음 식 변화, 증거 없는 수용 플래그 배제, 다음 세대의 동일 목표 식별자 조회를 추가했다.

## 직접 확인할 수 있는 실행 결과
- 첫 커밋 CI 성공: https://github.com/hushmind3/dominion-rust/actions/runs/42934411128
- 두 번째 커밋 CI: https://github.com/hushmind3/dominion-rust/actions/runs/42934835808 (반드시 최종 conclusion 확인)
- CI 작업은 `npm ci --prefix web`, 타입 검사, Vite 빌드, 스크립트 검사, `cargo test --release`를 실행한다.
- 연결 가능한 로컬 Rust 툴체인·모델 실행 환경이 이 세션의 도구 환경에는 없었다. CI 테스트 통과와 실제 모델의 장기 RSI 성과를 혼동하지 말 것.

## 다른 브랜치에서 확인한 장점
- Sub3: 실측 보상 기반 탐색식 자동 생성(`d29d509`), 실험 큐와 다음 세대의 동일 목표 식별자(`b9d2598`).
- Sub1: 진전 없는 세대가 실제 재시도 전에 끊기는 문제 수정(`780b456`).
- Sub2: 캠페인 간 실패 경험 warm start(`1fedc7f`).
- Champion: 동일 세대 인덱스 재활용 및 다음 세대 재계획(`f77f457`); Sub4는 그에 상응하는 인덱스 지문을 보유한다.
장점만 참조했고 타 개발 브랜치는 수정하지 않았다.

## 남은 핵심 작업 — 우선순위
1. `ab3ba64` Rust CI 통과 여부 확인. 실패하면 구체적 컴파일 오류/테스트를 해결한 뒤 같은 브랜치에 수정.
2. **실제 모델 캠페인**에서 자동 탐색식이 제안 → 다음 세대 선택 → 독립 holdout 비교로 이어지는지 관측. 먼저 무료/로컬 제공자로 소수 세대, 이후 같은 소스·모델·동결 평가 사례의 반복 실행으로 차이를 측정.
3. 검색 정책은 현재 비무작위 순차 비교다. 동일 입력에서 정책 A/B를 임의 배정한 독립 쌍과 동결 holdout의 이득·토큰 대비 이득을 쌓아 승격하라. 단순 replay 적중률, 채택 플래그, 정책 등록 건수는 지능 증가 증거가 아니다.
4. 3세대 연속 채택 + 매 세대 재빌드·독립 holdout 이득·다음 세대 증거 계승을 실제로 완주하고 `docs/rsi-completion.md`의 측정값을 갱신할 것.

## 반복하지 말아야 할 실패
- 부모의 낡은 `reuseDecision`을 현재 파일 선택의 권위로 사용하지 말 것.
- 사용자의 포괄적 자율 개선 안내 문구를 `rsiAutonomous`의 탐색 목표 제한/실험 해시로 잘못 취급하지 말 것.
- 모델의 성공 주장, 조사 텍스트 증가, 알고리즘 등록만으로 개선 효과를 증명하지 말 것.
- 다른 Sub·Champion 브랜치 직접 수정 금지.


## 5세대 Sub4 (2026-11-11) — 실행 실패에서 다음 세대 교정으로

역할: Sub4 / 전용 브랜치 `dominion-sub4`. 시작 `3c974e8`; 구현 HEAD `98379a7d4de2687baeead73f93c9317fe9bff217`. 직전 심판: **35/100, 4위**. 새로운 심판 점수는 없음.

### 왜 이 변경을 했는가

기존 `$rsiFocusPath` 절차 전이는 서로 다른 코드 파일에서 실제 도구 실행과 후속조건 평가까지 수행했지만, **다른 분야에서 실행이 실패한 영수증이 다음 세대의 교정 지시로 이어지지 않았다**. 2회 무성과 종료가 실제 교정 이전에 걸릴 수도 있었다. 외부 Sub3 실모델 run `42947169857` 역시 3세대에 걸쳐 실제 코드 읽기 후 수정 0회/채택 0회로 종료됐다. 이 결과를 Sub4의 개선 성능으로 세지 않는다.

### 이번에 실제 구현·푸시한 코드

- `060137549`: `src/engine/rsi_evolve.rs`에서 실제 cross-domain procedure 작업 `operationIds`와 검증된 `violated` 후속조건의 영수증을 다음 세대의 `transfer_failure_repair` 계획으로 전환. 실패 method ID/revision, 현재 대상 파일, 위반 사항을 해당 자손 worker의 **실제 실행 프롬프트**에 전달. 동일 소스 커밋의 인덱스 경로를 선택하고, 이전 성공 분야의 방법은 보존.
- `7a2d8cd`: 실제 `tools::execute(workspace_replace)`로 별도 `tools.rs` 테스트 파일을 수정 → 고의로 서로 다른 후속조건 위반 → `collect_method_executions` → `record_method_feedback` → 교정 세대 지시 생성까지 통과해야 하는 Rust 테스트 추가. 실행 없는 가짜 방법이나 검증 불능 증거는 배제.
- `98379a7`: 실제 실패가 남아 있으면 한 번의 교정 세대가 `no_progress` 종료 및 미검증 새 정책 가설보다 앞서 실행되도록 연결. 이전 교정 세대에서 또 실패하면 반복 생성하지 않는다.

### 실제 실행·검증 현황

이전 Sub4 `3c974e8`의 GitHub Actions `42946630782`는 **성공**. 신규 HEAD Rust/프론트 CI는 `42949584690`, `42950032403`, `42950244534`로 트리거됨. 이 문서 작성 시에는 모두 완료 전이었으며, 결과 확인 없이 신규 테스트 성공이라고 주장하지 않는다. 로컬 작업 컨테이너에는 `cargo`/`rustc`가 없어 Rust 실행은 GitHub Actions CI에서 확인한다. 실제 모델이 스스로 더 나은 수정·holdout 채택·후손 계승까지 달성한 것은 **아직 미확인**. main·Champion·Sub1~3는 수정하지 않았다.

### 다음 실행자가 즉시 할 일

1. `98379a7`의 CI 결과와 실패 시 **실제 Rust 에러 로그**를 확인해 Sub4에서 고친다.
2. 실모델 격리 캠페인에서 **검증된 원본 방법 → 다른 분야 실행 실패 → 도구 영수증 → 재빌드 후손 교정 실행 → 독립 holdout**을 같은 계보로 추적한다. `methodExecutions`, `rsiTransferRepair`, `rsiFocusSelection`, `domainSelection`, `toolsUsed`, `candidateId`, `acceptedCommit`, `evaluation.metrics`를 함께 보존한다.
3. 모델이 소스 읽기만 했다면 마지막 도구 호출을 기준으로 왜 `workspace_replace`가 나오지 않았는지 수정한다. API 동작 성공/CI 통과를 실제 RSI 품질 상승으로 치환하지 않는다.
4. 실제 개선 패치·독립 holdout 순이익·후손 바이너리 계승·다른 도메인에서의 재개선을 3세대에 걸쳐 실측한다. 종전 21캠페인/41세대/1채택은 새로운 측정 없이 갱신하지 않는다.
