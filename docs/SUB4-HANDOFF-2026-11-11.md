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


### 5세대 추가 2차 개선 — 실모델의 읽기 전용 종료를 실행 결정으로 전환

- `25d212778c7a2ef70616771e7a0dc52665516440`: `src/engine/chat/runtime.rs`의 RSI 런타임에 `rsi_read_only_completion` 도입. 모델이 실제 `workspace_read_range`를 성공적으로 끝내고 `workspace_replace/workspace_write/rsi_method_propose/algorithm_propose`를 전혀 시도하지 않은 채 최종 답변을 내면, 동일 턴의 소스 근거를 보존해 딱 1회 더 **실행 선택**을 요청. 충분한 근거가 없으면 그 뒤 최종 보고 허용; 모델 대신 패치·성공 생성 없음. `execution.rsiActionRetry` 영수증과 단위 테스트 추가.
- `b90f7abc`: `scripts/sub4-live-rsi.mjs` 실모델 검증 스크립트 작성. 도구 호출, 실제 소스 변경, 방법 제안, paired holdout 판정, 세대별 성적을 별도로 기록. 
- `da71da359306567e65000c42b1e94ca26de49fc5`: `.github/workflows/sub4-live-rsi.yml`로 외부 무료 `openai-fast`를 실제 DOMINION 바이너리와 연결한 격리된 RSI 캠페인 자동 실험 시작. 해당 실행: https://github.com/hushmind3/dominion-rust/actions/runs/42951068895 . 작성 시 probe 성공 후 native Rust 바이너리 빌드 단계 진행 중. **실모델 수정·채택·계승 성과는 결과가 나오기 전까지 미확인**.
- 변경 범위를 모두 포함한 신규 CI: https://github.com/hushmind3/dominion-rust/actions/runs/42951068772 . 코어 교정 경로 `98379a7` CI는 https://github.com/hushmind3/dominion-rust/actions/runs/42950244534 에서 성공, Rust 16+646+3+99=764건 통과. 중간 `7a2d8cd` 한 번의 브라우저 원격 디버깅 포트 실패는 다음 같은 핵심 코드의 CI에서 정상 통과했으므로 코드 개선의 근거로 취급하지 않음.
- 다음 세션은 실모델 `42951068895`의 artifact `sub4-genuine-model-proof`에서 `generations[].toolsUsed`와 `sourceWriteCalls` 및 `acceptedCommit`를 반드시 대조. `rsiActionRetry` 발생 여부는 각 execution 영수증으로 확인. model endpoint 실패는 개선 실패와 구별. 실제 수정과 held-out 품질 개선은 별도 증명해야 함.


### 5세대 후속 CI 피드백과 수정

- `25d2127`의 실모델 소스 read→edit 추가 선택이 일반 조사용 `rsiRun`에도 적용되어 기존 E2E `rsi_context_is_not_reinterpreted_as_tasks_or_forced_into_a_write`가 **실패**했음. 정확히 모델 호출 3회 예상→4회 실제로 늘어나는 회귀였고, 실제 로그 `42950867421`의 647 Rust 단위 테스트 전부 성공 뒤 99 E2E 중 1개 실패로 확인.
- `1d7cf90662e3d1198b69fb51adb7aab1403b4f78`: 이미 선택된 RSI 탐색 도구 제한 상태를 모델의 모든 후속 라운드에도 유지. 한번 제한한 code_search가 다음 모델 호출에서 재개되는 결함을 고침.
- `da026d29da62a1ec3456000f8e0fdf0668b77c11`: `rsi_generation_has_action_target` 추가. 추가 편집 결정은 실제 `rsiCampaignId`, `rsiGenerationJobId`, 안전한 `rsiFocusFile`이 있는 native source-improvement worker로 한정; 일반 조사 전용 RSI 채팅은 3회 후 그대로 종료. 유닛 회귀 검증도 추가. **이 수정의 전체 CI 통과는 GitHub Actions 확인 전까지 미확인**.
- 최신 수정의 CI URL: https://github.com/hushmind3/dominion-rust/actions/runs/42951710133 . 별도 real-model workflow `42951068895`는 이전 `da71da3` 기준 실험이므로 새 수정에 대한 실모델 증명으로 세지 않는다.
- 실제 구현 후 바로 해야 할 일: 새 `da026d2` E2E 전체 통과 확인, 실패 시 동일 테스트부터 원인 분석. 현재 연구 전용 세션의 무작정 추가 호출을 다시 도입하지 않는다. 실모델 결과가 오면 `sourceWriteCalls`와 실제 후보 holdout 채택을 먼저 본다.


### 5세대 실모델 결과, 로컬 fallback 및 회귀 재검증 (최종 인수인계)

- **외부 실제 모델 결과**: GitHub Actions https://github.com/hushmind3/dominion-rust/actions/runs/42951068895 는 실모델 probe 성공, DOMINION 실제 모델 호출 **4회**, `workspace_read_range` **3회**, `workspace_replace` **0회**, 방법 제안 0, 채택 0. 상태 `needs_review`, 최종 오류 `최근 응답이 너무 느렸던 모델이라 잠시 건너뜁니다.`. CI 자체의 성공을 RSI 완주 성공이라고 표시하지 말 것. Artifacts `sub4-genuine-model-proof` 실측 JSON 보존.
- **E2E 회귀 수정 검증**: `da026d2` GitHub Actions https://github.com/hushmind3/dominion-rust/actions/runs/42951710133 **성공**. Rust 총 **765건**(benchmark 16, unit 647, lib 3, E2E 99) 통과. 특히 `rsi_context_is_not_reinterpreted_as_tasks_or_forced_into_a_write` 통과; 일반 RSI 조사 대화의 숨겨진 추가 모델 호출 회귀 해결.
- **모델 비종속성 결함 수정**: `821385edf91373ba1673081a3a41ee54e4b8441e`에서 `src/engine/gateway.rs`의 Ollama 기본 host `:11434`, `/api`, 기존 `/v1`를 모두 추론용 `/v1/chat/completions` 경로로 귀결. 소스 모델 검색 경로는 별도로 유지. 클라우드 경로 변경 없음. 챔피언 브랜치에서 확인한 모델 연결 결함에 대한 Sub4 직접 구현.
- **독립 2차 실제 모델 시도**: `65a7aa2`에서 `scripts/sub4-live-rsi.mjs`가 원격 공개 모델과 실제 local Ollama 모델을 모두 지원. `fe7257ce4393901539ee54f525fdaaaa0d5702fa`에서 workflow `.github/workflows/sub4-live-ollama.yml` 추가, Qwen `qwen3:4b-instruct` 로컬 API 실험 실행. https://github.com/hushmind3/dominion-rust/actions/runs/42952326808 **진행 중**(기록 시점). 실제 모델이 파일을 수정·채택했는지는 해당 실행 로그와 `sub4-real-local-model-proof` artifact 결과가 나온 뒤 판정할 것.
- 현재 브랜치 전용 HEAD `fe7257ce4393901539ee54f525fdaaaa0d5702fa`, 다른 개발 브랜치 및 main 무수정. 후속 CI https://github.com/hushmind3/dominion-rust/actions/runs/42952326684 에서 Ollama gateway 패치와 스크립트 변경 포함 전체 검증 결과 확인 필요.
- **지금 다음 세션의 최우선**: 1) Ollama 실모델 run `42952326808`과 최신 CI 확인 2) 양 모델의 실제 tool trace에서 `workspace_replace` 0회인 구간 찾아 해당 source-excerpt/읽기→편집 선택 수정 3) 실제 개선이 발생하면 동일 범위 baseline/candidate 독립 holdout 및 방법 타분야 전이의 다음 세대까지 반복. 테스트 수·모델 호출 수로 실제 품질 개선을 대체하지 않음.


## 2026-11-11 심판 43점 피드백 이후 — 실제 편집 경로 연결

심판 재채점: Sub4 43/100, 3위. 강점은 분야 간 방법 전이와 실패 기반 후손 교정. 가장 큰 결점은 실제 모델의 수정 0회/채택 0회. 최신 Qwen run `42956442299`의 작업 로그에서 `code_search` 3회, `workspace_read_range` 3회 이후 `스트림 오류: error decoding response body`로 종료한 것을 확인했다. 해당 run의 GitHub Actions 성공은 학습 성공이 아니다.

### 이번에 변경한 코드 (담당 브랜치만 수정)
- `7131d3c`: `src/engine/tools.rs`에 `workspace_replace_range` 독립 구현. 소형 모델이 이전 원문 `oldText`를 되풀이하지 않고 줄 번호·`newText`·`expectedSha256`만 제출한다. 쓰기 전에 원본 해시를 확인하며 실제 `checked_write` 잠금에서 다시 검사한다. 로컬 모델의 숫자 문자열 인자도 처리하고 실제 도구 테스트를 추가했다.
- `10d46c4`, `e8952a9`: RSI 런타임 집중 편집 메뉴, 도구 실행/환경 버전 증가/읽기 전 단계, native model `tool_choice=required`를 범위 편집으로 연결했다. 연구 전용 대화는 자동 편집으로 바꾸지 않는다.
- `aff0f2e9`: `rsi_evolve`의 실제 source write 허용, 실행 영수증, 다음 세대 도구 목록과 프롬프트에 범위 편집을 포함했다.
- `17fcbdd`, `3d6cf38`: 실모델 측정 스크립트와 Ollama workflow의 tool trace에 범위 편집을 포함했다.
- `1312cba`: 파일을 실제로 읽고 도구 호출 누적 4회가 되면 한 번의 집중 편집 결정을 조기에 요청. `rsiFocusFile`과 같은 실제 작업 대상 파일을 읽은 영수증이 있어야 발동하며, 모델이 수정 내용을 작성한다.
- `b143ab4`, `76d3d27`, `3009754`: 완료된 도구 호출과 실제 파일 수정을 분리. `actualWrites`는 파일 쓰기 영수증의 `written:true`에서만 증가하고, 실제 수정이 확인되지 않으면 새 모델 실험의 `sourceWriteCalls`에 포함하지 않는다.

### 확인된 결과와 아직 남은 증명
- `7131d3c` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42963462694 — success.
- `10d46c4` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42963657454 — success.
- `e8952a9` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42963687461 — success.
- 전체 통합 HEAD `3009754` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42964592996 — 문서 작성 시 queued. 따라서 통합 CI 성공으로 표시하지 않는다.
- 실제 Qwen 통합 모델 증거: https://github.com/hushmind3/dominion-rust/actions/runs/42964592996 의 CI와는 별도의 `Sub4 actual local Ollama RSI attempt` 최신 실행 및 `sub4-real-local-model-proof` artifact를 확인해야 한다. 모델 수정·독립 holdout 채택·3세대 연속 개선은 아직 증명되지 않았다.

### 다음 최우선 작업
1. `3009754` 이후의 최신 CI 컴파일/테스트 결과를 확인한다. 실패 시 `dominion-sub4`만 수정한다.
2. 실제 `qwen3:4b-instruct` 최신 run에서 `rsiActionRetry.reason=focused_source_observed`, `workspace_replace_range` operation, `actualWrites`, 실 git diff, `candidateId`, `acceptedCommit`를 함께 대조한다. 호출만 한 경우는 성과 0으로 기록한다.
3. 파일 수정이 실제 발생하면 독립 frozen paired holdout, 수용, 재빌드 후 다음 세대의 다른 문제 개선을 수행한다. 후속 세대가 독립 품질 개선을 이루기 전에는 43점 기준을 유지한다.
4. 타 Sub·Champion·main은 변경하지 않았다.


## 2026-11-11 최신 심판 46점·3위 이후 — 실제 source-write 검증과 세대 실행 복구

새 심판 기준 Sub4는 **46/100, 3위**(구현 24/30, 실제 RSI 4/40, 계승 12/20, 서비스 6/10). 특히 기존 강점은 `$rsiFocusPath` 방법 전이와 실패 기반 자손 교정이며, 핵심 미완료는 실모델이 학습된 방법으로 다른 분야의 독립 holdout 품질까지 개선하는 것이다.

### 먼저 확인한 실제 모델 결과

- Qwen 실모델 `42963802448`: 도구 검색/읽기만 진행하다 `연결 실패: error sending request`; 소스 수정 0, 채택 0.
- Qwen `42964066897`: 실제 모델 호출 5회, 소스 수정 0, 채택 0. 2세대 요청이 **5,761토큰**인데 실제 Ollama가 **4,096토큰 컨텍스트**만 수용해 HTTP 400으로 중단됨. 워크플로의 `OLLAMA_CONTEXT_LENGTH=16384` 설정만으로는 OpenAI 호환 호출의 모델 컨텍스트가 보장되지 않았다.
- Qwen `42964106396`: 선택된 `decompose.rs`를 읽고 `focused_source_observed` 실행 선택이 발생했지만, 10분 턴 제한을 초과해 수정 0, 채택 0. `rsiActionRetry` 로그는 있었으나 변경 영수증은 없었다.
- GitHub Actions job `success`는 실모델 품질 향상이 아니라 실험/아티팩트 업로드 완료에 불과하다.

### 실제 구현: `dominion-sub4` 전용

1. `97b25b7` — `src/engine/effect.rs`: `workspace_replace`와 `workspace_replace_range`를 동일 소스 리소스에 매핑하고 실제 파일을 다시 읽어 `postSha256`과 대조. 모델의 성공 주장이나 완료 도구 호출을 검증 영수증으로 오인하지 않는다. 실제 readback 검증 유닛 테스트 추가.
2. `586b37e` — `src/engine/tools.rs`: 실제 `checked_write` 성공 후에만 `postSha256` 기록. 글자만 교체했다고 기록되는 경우 방지.
3. `3c87ba2` — `src/engine/chat/runtime.rs`: 정확히 선택된 대상 경로에 모델이 수정하고 `toolReceipt.verified`가 참인 경우 **추가 모델 호출 없이 `rsi_worktree::finish`의 실제 Git diff 후보 수집·검사로 이어지도록** 라운드 종료. 단순 편집 호출은 채택으로 간주하지 않음.
4. `afc4975` — 실제 Qwen 실험은 `qwen3:4b-instruct`로부터 `PARAMETER num_ctx 16384`를 가진 별도의 `sub4-qwen3-16k:latest` 모델을 Modelfile로 생성. Ollama 공식 OpenAI 호환 경로의 설정 방식에 맞췄으며, 생성된 설정을 조회해 검증한다.
5. `d8d7d28` — `scripts/sub4-live-rsi.mjs`: 실모델 제공자에 16K 컨텍스트를 선언하고 실제 `workspace_replace_range` 도구를 사용하도록 명시한 목표로 실험.
6. `67f8012` — 4B CPU 실행에서 읽기→편집 결정이 진행되도록, 실제 RSI 자손 세대에 한정한 턴 제한을 840초로 확대하고 집중 소스 읽기 뒤 편집 결정을 조기에 요청. 일반 대화의 제한은 그대로 유지.
7. `8854077` — 모델이 편집 인자에서 해시를 누락했을 때 **같은 실행에서 동일 대상 파일·포함된 범위를 실제로 읽은 결과**가 있다면 그 해시를 보충. 모델이 공급한 해시는 덮어쓰지 않고 새 코드를 임의로 생성하지 않는다.

### 검증 수준과 다음 세션의 우선순위

- 이 일괄 변경 직전의 최신 통합 CI `42964592996` 성공. 이후 변경의 전체 CI/실모델 결과는 작성 시 대기·진행 중이며 성공으로 기록할 수 없다.
- 최신 패치 `8854077`의 CI: https://github.com/hushmind3/dominion-rust/actions/runs/42967958254
- 최신 실모델 시도: https://github.com/hushmind3/dominion-rust/actions/runs/42967958256
- **다음 최우선:** 실제 Qwen 실행에서 `workspace_replace_range` 실행 → `written:true` → `toolReceipt.verified:true` → `diffBytes>0` → `candidateId` → `checksPassed` → frozen paired holdout `eligible` 및 `acceptedCommit` 순서대로 증거를 확인한다.
- 첫 독립 개선 채택이 생기면 `rsiInheritedImplementations`와 실제 다음 세대 바이너리에서 **다른 분야 적용/실패 교정/재채택**을 확인하고 세대 계보를 고정한다. 모델 없는 합성 테스트 성공을 교차 분야 실모델 개선으로 보고하지 않는다.
- Champion, Sub1~3, main은 수정하지 않았다. Sub4 외부 수정 없음.


## 심판 50점(2위) 피드백 이후 — DOMINION 스스로 방법을 합성·수정하는 학습 커널

### 기준 변경

모델은 코딩·추론 도구이며 지능 향상의 소유자는 DOMINION이다. 소스 편집·holdout은 수단/관측기이다. 재귀적 자기개선의 대상은 `어떤 도구를 어떤 순서로 선택하고 어떻게 실패에서 갱신하는가`라는 DOMINION의 개선 *방법* 자체다. 최종 심판은 Sub4 **50/100, 2위**(핵심 엔진 20, RSI 10, 경험·계승 14, 제품 6). Champion 51점으로 자리 교환 없음.

### 이번 새로 구현한 알고리즘

1. `879028c` — `src/engine/rsi_evolve.rs`의 `witnessed_method_trace`와 `learn_method_transitions`. 소스 코드 위치·oldText·모델 이름을 외우지 않고 실제 `execution_operation`의 도구 흐름을 추상화한다. `workspace_replace*`는 실제 쓰기와 독립 readback 영수증이 있어야 학습된다. 절차 실행 후속조건 `verified+followed`가 확인되면 **운영 단계의 학습 근거 1점**, 자체 알고리즘의 실제 채택과 동결 독립 paired holdout 개선이 검증되면 **능력 개선 근거 3점**으로 누적한다. 후속조건 위반은 부정 가중치로 축적해 같은 도구 전이가 자동 추천되지 않도록 개정한다. 이는 모델의 제안/텍스트를 학습 성과로 부풀리지 않는다.
2. `f813062` 및 `5618fea` — 같은 jobId 중복 학습 금지, 오래된 근거를 제거하면 현재 32개 유효 영수증으로 전이 가중치를 재산출, `rsi_method_policy` 영구 보관, 새 내용이 나올 때 revision을 갱신. 합성 절차는 특정 코드 경로를 베껴 반복 실행하지 않고 도구 전이 규칙으로 저장되므로 서로 다른 분야에서 재사용할 수 있다. 가짜 상태/모델 자체 선언으로는 양성 근거가 생성되지 않도록 검증을 추가했다.
3. `f2e0ea2` — `src/engine/chat/runtime.rs`의 `rsi_induced_next_tool`: 실제 RSI 자손 워커가 이전 소스 관찰에 도달한 순간 전이 가중치로 다음 도구를 선택해 **모델에 제공되는 실제 도구 목록의 우선순위를 변경**한다. 같은 실행·같은 대상 파일의 관찰만 유효하며 읽기 전의 임의 쓰기 추론은 허용하지 않는다. `execution.rsiInducedMethodDecision`에 모델 호출 이전 적용된 정책 ID/revision/추천 도구를 남긴다.
4. `1f52ea0` — 모델이 실제 근거를 읽고 마지막 편집 결정을 할 때, 해당 도구 선택 정책의 학습 결과에 따라 `workspace_replace` 또는 `workspace_replace_range` 중 **사용할 편집 원시 동작 자체를 전환**한다. 학습 근거가 없으면 기존 SHA 기반 범위 편집을 기본으로 유지.
5. `c6c4983` — 실제 다음 세대에서 권장한 도구가 수행됐는지와 읽기 검증을 통과한 실제 소스 변경이 있었는지를 `inducedMethodApplications`에 별도로 기록. 모델이 해당 도구를 이용했다고 주장하거나 우선순위만 바뀐 경우를 실제 개선으로 세지 않는다.
6. `ed55220`, `9de90fe` — 앞선 CI에서 확인된 블록 닫힘 누락과 기존 소형 모델 SHA 보존 테스트의 정규화 비교 오류를 수정했다.

### 증명 수준

- **구현:** 소스 기반 동작 알고리즘·지속 기록·다른 분야 정책 선택·증거 기반 개정 경로 연결. 일반 RSI 대화에서는 적용되지 않고 실제 자손 개선 세대만 사용한다.
- **검증 중:** 최신 커밋 CI와 해당 브랜치의 Qwen 모델 캠페인. 테스트를 로컬에서 직접 실행한 적은 없으며 CI 결과를 확인해야 한다.
- **성공으로 집계하지 않음:** 모델이 새 정책을 썼다고 발언한 사실, 저장된 policy revision 개수, 도구 선택만 바뀐 경우, 자동 holdout 채택 기록이 없는 실행. 실제 모델의 연속된 2~3세대 독립 품질 개선은 아직 증명되지 않았다.
- `cross_domain_observed`는 **서로 다른 2개 분야에서 독립 개선과 유효한 절차 사용이 각각 관측되었다**는 의미이지, 정책 자체의 인과 효과가 측정됐다는 뜻은 아니다. 정책의 인과 효과는 동일 모델/동결 평가에서 정책 A·B를 비교해야 판단한다.

### 후속 개발에서 실제로 이어갈 중요 작업

새 정책으로 시작된 실제 Qwen 세대의 `execution.rsiInducedMethodDecision` → `generation.inducedMethodApplications` → `rsi_run.diffBytes` → `candidateId` → frozen paired holdout → `acceptedCommit`을 하나의 계보로 비교한다. 다른 분야에서 성과와 실패가 나온 뒤 변경된 policy revision으로 다시 같은 종류의 자기개선을 수행하고 능력 변화를 실측한다. 변경 전후 모델 코딩 결과만 비교하지 말고 DOMINION의 **방법 선택/실패 수정 능력**을 평가해야 한다.

다른 개발 브랜치와 main은 수정하지 않았다.
