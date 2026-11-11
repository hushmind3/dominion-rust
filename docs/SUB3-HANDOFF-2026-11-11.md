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

## 다음 회차 Sub3 — 심판 재채점 36점/100점 반영 (2026-11-11)

### 목표와 중요한 문제
- 직전 심판: Sub3 3위, **36점**. 21개 캠페인·41세대에서 채택 1회, 최대 연속 채택 1세대. 첫 실제 모델 자율 개선·후손 세대 계승 미입증.
- 기존 코드에서 기초 검사 수보다 중요한 것은 **실제 모델이 실패를 해결하는 소스 패치를 만들어 독립 holdout의 문제 해결 품질을 높이는 것**.
- 조사 결과 `chat/runtime.rs`은 실제 실행에서 `task.decomposition` 커널을 선택하지만, 새 에이전트 기본값이 `single-step`이라 개선된 다중 단계 추출 알고리즘이 실제 대화에 적용되지 않는 경로가 있었다.

### 이번 세션에서 실제 구현해 푸시한 내용
- `be637525`: `src/engine/decompose.rs`에 한국어 동작 접속 `-고`, `다음`, `나서` 분석. 본문 단어를 유지하면서 순서 있는 하위 실행 단계로 분할하는 코드와 회귀 테스트.
- `6e3289dc`: 운영 RSI 평가에 해당하는 동결 사례 13개를 별도 A/B 계획으로 보관.
- `192f0f81`: 변경 전후의 실제 Git worktree를 별도로 빌드하고, baseline의 고정 oracle을 **양쪽에 동일하게** 주입해 실행하는 A/B GitHub Actions.
- `25687e1d`/`836dede2`: 무료 모델 엔드포인트 접속 후 실제 DOMINION 서버에 모델을 연결하고 `rsi/start` → 도구 호출/코드 변경 → 독립 A/B까지 시도하는 실행기·별도 워크플로. 가짜 모델 fixture는 사용하지 않음.
- `53dba6fa`/`0c932596`: **제품 실제 채팅 경로**에서도 여러 동작이 명시된 목표에는 품질이 더 나은 실행 커널을 선택하도록 `algorithms.rs`와 `chat/runtime.rs`를 연결. 잘못된 UCB 커널에 실행 성과를 귀속하지 않게 실제 선택 revision을 `algorithm_usage`에 기록하고 회귀 테스트를 추가.
- `656457f5`: 실측된 정확도·소요시간·비교 방식·증거 출처를 `benchmark/SUB3-FIRST-GAIN-MEASURED-2026-11-11.json`에 영구 보존.

### 실제 결과 — 코드 품질 개선의 독립 실측
- GitHub Actions: https://github.com/hushmind3/dominion-rust/actions/runs/42944474795 **SUCCESS**.
- 원본: `1d94e1da1ed4f47243a793ab0625182f5a2c9656`; 새 코드 커밋: `be6375259d1dfe6e9317d14707dbc7d360fb0922`.
- **고정 13개 사례 모두 baseline/candidate 통과**. 5개 성능 시나리오에 대해 양쪽 6회 반복 측정.
- 한국어 계획 분해 정확도: **0.75 → 1.00 (+25%p, 상대 33.3%)**. 실행시간 중앙값: **58.59ms → 60.20ms (약 2.74% 증가)**. 기존 평가의 시간 회귀 한도 25% 이내. 기억·탐색·그룹 문맥 정확도는 1.00 유지.
- 이 변경은 GPT-6 개발자 모델이 직접 작성해 GitHub에 저장한 코드이므로 **모델 생성 코드의 독립 개선 실증**이다. **DOMINION 자체의 내부 RSI 모델이 작성·채택한 결과라고 주장하지 않는다.** 그 별도 실행 증거는 아래 워크플로에서 구한다.
- 실측 워크플로는 source A/B 결과만 검사했고 `rsi::evaluate_cases`·`rsi::adopt`의 실제 운영 승격 경로를 호출하지 않았다. 성능 수치가 나왔다고 자동 RSI 채택 1회를 추가하지 말 것.

### 실모델 내부 RSI 캠페인
- workflow: https://github.com/hushmind3/dominion-rust/actions/runs/42945129829
- 무료 모델 프로브는 성공했고, DOMINION release binary 빌드까지 완료. 진행 중인 `Let DOMINION execute a genuine model-written source improvement` 단계에서 실제 `rsi/start`를 수행한다.
- JSON 출력 아티팩트: `sub3-real-model-attempt` 안의 `benchmark/sub3-live-campaign-result.json`. 반드시 **진짜 코드 diff·`candidateId`·독립 평가·`acceptedCommit`**를 확인해 실제로 된 것만 기록한다.

### 남은 최우선 미완성
1. 실모델 내부 캠페인의 종료 결과를 확인하고 무패치·모델 도구 호출 실패·평가 무개선 중 **실제 실패 지점**을 수정한다.
2. 통과한 코드 A/B 실증을 운영 `rsi_candidate`와 동결 평가·채택·후손 재빌드까지 연결해 첫 **DOMINION 자율 완주** 증거를 만든다.
3. 원래 41세대·채택 1회 기록에 새로 성공한 내부 캠페인만 추가해 정확한 성공률을 갱신한다.
4. 후손 세대가 같은 능력을 재사용해 다른 문제에서 추가 성과를 내는지, 독립 holdout으로 검증한다.

### 1차 실모델 캠페인의 종료 결과 — 2026-11-11 11시대(KST)
- CI run: https://github.com/hushmind3/dominion-rust/actions/runs/42945129829 (**workflow SUCCESS는 실모델 RSI 채택 성공을 의미하지 않음**).
- 진짜 원격 `openai-fast` 5회 호출·prompt 17,865 tokens·completion 2,274 tokens(합계 20,139).
- 실제 도구 `code_search` 1회, `workspace_read_range` 1회. 코드 편집 0회, 후보 0건, accepted 0세대, holdout 향상 0.
- 1세대에는 새 연구 1건을 저장했고, 2세대는 모델 호출 0회에서 `rsi_token_budget` 중단. `maxAttempts=2`, `tokenBudget=24000`.
- 이 실패는 `benchmark/SUB3-FIRST-INTERNAL-RSI-ATTEMPT-2026-11-11.json`에 관측치·출처·실험 구분을 남김.
- 원인 해소 구현:
  - `b0bccc14` `src/api/rsi.rs`: 사용자가 파일과 결함을 직접 정한 작업은 연구만으로 종료하지 말고 실제 `workspace_replace` 호출을 시도하도록 변경.
  - `a9f28318` `src/engine/rsi_evolve.rs`: 이전 세대에서 소스 연구가 저장되었으면 다음 세대에 **연구→실행** 전환 지시.
  - `6e836dbb` `scripts/sub3-live-model-campaign.mjs`: 모델 작업 지시를 실제 편집에 맞게 구체화, 최대 3세대·90,000토큰·2,700초로 변경.
  - `5b3dba23` CI 실모델 실험 상한 연장.
- 위 수정으로 실행한 다음 워크플로: https://github.com/hushmind3/dominion-rust/actions/runs/42947169857 (조회 시 진행 중). 그 결과는 **별도 재검증 필요**.
- `0c932596` 실제 대화 선택 경로와 `53dba6fa` 알고리즘 선택 커널의 GitHub Actions 전체 CI 성공이 확인되었다. 새 사용자 목표에서 다단계 분해를 실행하는 경로가 운영 코드에 연결되었지만 실제 사용자 모델 대화의 끝까지 성공한 건 추후 계측할 과제다.

#### 다음 Sub3 시작점
1. `42947169857`의 `sub3-real-model-attempt` 아티팩트에서 `candidateId`, `toolsUsed`, `acceptedCommit`, `improvedHoldouts`를 확인할 것. 0회라면 **다음 모델 도구 선택이 실제 편집까지 갔는가**를 원인으로 삼아 코드 수정할 것.
2. 준비된 코드는 13개 고정 A/B에서 0.75→1.00 개선됨; 이를 실모델 자율 채택이라고 잘못 발표하지 말 것.
3. 내부 실모델 채택이 실제로 나오면 **수정 바이너리 재빌드→같은 목표 다음 세대**와 다른 문제에서의 성능 향상까지 검증할 것.


## Sub3 후속 개선 — 실모델 실행 고리 연결 (2026-11-11, 4세대 연속)

### 역할·시작 기준과 이전 심판
- 역할 Sub3 / 수정 가능 브랜치 `dominion-sub3`; 시작 HEAD `4563fde7c1b85fb2ef99cfcbc5eea9e629639f06`.
- 이전 심판은 Sub3 **36/100, 3위** (Champion39, Sub2 37, Sub4 35, Sub1 34). 새로운 심판 점수는 스스로 확정하지 않는다.
- 21개 캠페인·41세대·기록상 채택 1회·최대 연속 1회는 과거 공유 기준. 이번 실모델 실패를 추가 채택으로 계산하지 않는다.

### 다른 브랜치에서 확인한 참고점
- Sub1 (조회한 HEAD `65deca43`, 기구현 `c8a5113`): 최신 실패 작업과 완료 시각을 다음 캠페인의 실행 가능한 교정 지시로 계승.
- Sub2 (조회한 HEAD `cc44bfc4`, 기구현 `7cee3e7`): 실제 정책 실험을 대조군과 묶고, 무생산 통제 반복 시 후보 탐색을 실행.
- Sub4 (조회한 HEAD `523852b7`, 기구현 `8bc72ac`): 검증된 소스 분야 절차를 다른 분야에서 실행하고 도구·사후 조건 피드백으로 결과 기록.
- Champion (조회한 HEAD `41186886`, 기구현 `75f02240`): 독립 동결 holdout 비교와 증명 없는 후보 승격 차단, 재빌드 계보 보존.
- 위 브랜치들은 **읽기/비교만 진행**, 수정하지 않았다.

### 관측으로 드러난 장애
- 실제 자유 모델 `openai-fast` 캠페인 `42947169857`: **3세대/75,971토큰, 실제 편집 0, 후보 0, 채택 0, 마지막 no_progress**. 검색1+읽기8로 끝남.
- 후속 캠페인 `42949697316`: **2세대/15,469토큰, 모델 호출3, 실제 읽기1, 편집0, 후보0, 채택0**. workflow 성공 ≠ RSI 성공.
- 원인 분석에서 **캠페인 워커가 과거 BASE_SHA(1d94e1da)로 재빌드되는 실행 계보 문제** 발견: 현재 하네스 변경은 CI에서 테스트되어도 실제 모델 워커에서는 적용되지 않았다. 그러므로 위 모델 결과의 변화는 새 런타임 성능 향상 증거가 아니다.

### 이번 세션에 실제 푸시한 변경
1. `01facc1` `src/engine/chat/runtime.rs` / `src/engine/jobs.rs`:
   - 명시적 소스 수정 목표의 확인된 원문/완료된 읽기 호출을 기반으로 **읽기→편집 실행 단계**로 전환.
   - 체크포인트 이후 다음 라운드에 검색 도구가 다시 열리지 않도록 매 라운드 적용.
   - 실제 코드 변경이 없는 조기 완료에는 1회 교정 실행 기회를 제공.
2. `0759930`: 1번 소스 읽고 종료하려는 모델의 **조기 종료**도 단발 편집 재시도로 연결. 실모델 스크립트에 sourceEditCalls, candidateCount, acceptedGenerations, independentlyVerifiedImprovement 추가.
3. `fd971a7` `.github/workflows/sub3-live-model-cycle.yml`:
   - 워커를 **현재 Sub3 코드**에서 빌드하기 위해 최신 HEAD로 분리 작업공간 생성.
   - 오직 `src/engine/decompose.rs`만 역사적 버그가 있는 `BASE_SHA` 버전으로 복원하고 **분리 Git 실험 커밋** 작성.
   - 실제 모델이 생성한 코드 변경과 재현용 버그 복원 코드를 구별.
4. `4205f37` → `f7797bf` `runtime.rs`:
   - 명시적 RSI 편집 단계에서 네이티브 도구 응답 대신 모델이 텍스트 `<tool_call>`을 반환할 경우 기존 알려진 도구 파서로 실행 가능한 호출 복원.
   - 일반 모델 대화/자율 연구 단계에는 이 복구 경로를 적용하지 않음. 단위 테스트 추가.

### 직접 확인한 실행 및 검증 범위
- `01facc1`: CI https://github.com/hushmind3/dominion-rust/actions/runs/42949449776 **성공** — Rust 16 benchmark + 623 unit + 3 extra + 99 e2e = **741/741**, 웹 타입 검사/빌드 성공.
- `0759930`: CI https://github.com/hushmind3/dominion-rust/actions/runs/42949697346 **성공** — 동일 Rust **741/741**, 웹 타입 검사/빌드 성공.
- `fd971a7`: CI https://github.com/hushmind3/dominion-rust/actions/runs/42950862155 **성공**. 새 실모델 캠페인 https://github.com/hushmind3/dominion-rust/actions/runs/42950861967 실행됨 — **이 문서 작성 당시 결과 미확인**, 성공으로 계산하지 말 것.
- `f7797bf`: 독립 CI https://github.com/hushmind3/dominion-rust/actions/runs/42951474873 실행 중. 실모델 캠페인 `42950861967`에는 이 후속 텍스트 파서 수정은 포함되지 않음.
- 실제 모델에 의한 소스 패치 생성→독립 paired holdout 개선→선택→재빌드→후손 적용 전체 완주는 **이 문서 작성 시 아직 입증되지 않음**.

### 다음 세션의 최우선 실행
1. **첫째:** `42950861967` 종료 후 아티팩트 `sub3-real-model-attempt`에서 `sourceEditCalls`, `candidateId`, `acceptedCommit`, `improvedHoldouts`와 `generatorCommit`을 확인. 부모-워커 계보가 실제 최신 Sub3 코드인지 GitHub Actions의 `SUB3_RUNTIME_HARNESS_COMMIT`/`SUB3_EXPERIMENT_SOURCE_COMMIT` 로그로 검증.
2. 여전히 편집 0이라면 실제 assistant 반환 내용·미완료/실패한 `execution_operation`을 수집해 **도구 선택 미발생과 도구 호출 실패**를 구분. 네이티브 텍스트 도구 복구 `f7797bf`을 포함한 동일 실모델 실험을 재실행하고 그 결과에 따라 실제 실행기를 수정.
3. 생성한 candidate가 있으면 이미 있는 frozen paired holdout 비교에서 성공/회귀/토큰을 확인하고, 승격 후 **후손 바이너리 재빌드와 이어지는 다음 개선**까지 실제 실행한다.
4. 이후 Sub1의 최신 실패 피드백 계승과 Sub4의 실행형 방법 전이를 비교 실험으로 연결한다. 새로운 검증 규칙보다 실제 패치 생성·완주와 성능 개선이 우선.


## 5세대 Sub3 — 실제 코드 편집 출력량 병목 개선 (2026-11-11)

### 최신 역할 / 배정 / 심판
- 담당 Sub3 / 수정 브랜치 `dominion-sub3`. 시작 HEAD `976900fe2`.
- 새로 전달된 독립 심판: **74/100, 2위** (구현 35/40, 완주 20/30, 자기개선 12/20, 일반화 7/10). Champion 78, Sub4 73, Sub1 70, Sub2 69. 이 점수는 아래 추가 코드에 대한 새 채점이 아니다.
- 공통 목표: 모델에 비종속적인 하네스가 실행·교정·기억 일반화·자기개선·후손 계승을 **실제 작업에서 완주**하는 것. 정적 검사나 모델 호출 자체는 완주가 아니다.

### 이번 작업의 실제 수정
1. **`8dd4e59999fe56b27356cde39b4df412e7084b34`**: `workspace_replace_range(path,startLine,endLine,newText,expectedSha256)`를 Rust 실행 도구로 구현. `workspace_read_range`의 원문 해시·줄 번호를 이용해 oldText 복제 없이 새 코드만 생성·대체, 최대 120줄, UTF-8 바이트 오프셋 계산, 기존 줄바꿈 유지, 해시 불일치 시 파일 보존. 기존 `workspace_replace` 유지.
   - `chat/runtime.rs` 편집 체크포인트·쓰기 인식, `chat/provider.rs` 실제 모델 네이티브 `tool_choice: required`, `api/rsi.rs`와 `rsi_evolve.rs` 연구→편집 지시, 실모델 캠페인 도구 집계 연결.
   - 실제 모델의 약 1,500 output tokens 제한에서 oldText와 newText를 중복 출력해야 하는 문제를 줄이는 **새 실행 수단**. 채택 성공이라고 해석하지 않는다.
2. **`43d0ef255e05b6f5759a501c33757e1919427fe3`**: 새 편집 도구의 실제 파일 변경 후 `postSha256`을 반환하고, `effect::seal`이 읽어 확인한 결과와 비교해 `verified` / `effect_unknown` 기록. 편집/읽기 도구의 동일 리소스 정체성 연결. 부모/후손 세대 도구 메뉴와 프롬프트에도 새 도구 등록.
3. **`2c06b00170e1628515b5543bd98eeeefaf15106a`**: 기존 `research_survives_restart_and_executes_hash_checked_reuse_in_the_next_session` 통합 검증 경로에서 실제 파일 수정 수단을 새 줄 단위 편집으로 변경. 1세대 연구 영구 저장 → 재시작 → 2세대 해시 검증 편집 → 개선 코드 계승과 갱신된 소스 재사용을 한 실행 경로로 확인하도록 수정. 이 변경의 CI 결과는 별도 확인해야 한다.

### 직접 실행해 확인한 결과 (코드 검증과 모델 성능을 구분)
- **`8dd4e59` CI https://github.com/hushmind3/dominion-rust/actions/runs/42956290382 성공**: 16 benchmark + 626 unit + 3 extra + 99 E2E = **744/744**, 실패 0. 반복 원문 줄에서 지정한 줄만 바꾸고 stale SHA를 거부하는 새 Rust 테스트 포함.
- **`43d0ef2` CI https://github.com/hushmind3/dominion-rust/actions/runs/42956709647 성공**: 16 benchmark + 627 unit + 3 extra + 99 E2E = **745/745**, 실패 0. 수정 해시 영수증을 별도로 재검증하는 테스트 포함.
- **`2c06b00` CI https://github.com/hushmind3/dominion-rust/actions/runs/42957662159**: 이 문서 작성 시 진행 중. 성공으로 선기록하지 않는다.
- 기존 실제 무료 모델 `42953014650`: 모델 6호출, 코드 탐색 1회+파일 읽기 1회, **편집 0, 후보 0, 채택 0**, 종료 `no_progress`.
- 새 `workspace_replace_range`를 포함한 실모델 run https://github.com/hushmind3/dominion-rust/actions/runs/42956290333 : 이 문서 작성 시 진행 중. 해당 run은 **첫 커밋 `8dd4e59` 기준**이므로 둘째 해시 영수증 통합/셋째 계승 테스트 코드는 포함하지 않는다. 종료 후 `sourceEditCalls`, 도구별 영수증, `candidateId`, `acceptedCommit`, `improvedHoldouts`를 확인해야 한다.
- 로컬에서 `cargo` 직접 실행 불가, Rust 실제 컴파일·테스트는 GitHub Actions 기록 기준.

### 핵심 미완성과 이어갈 실제 개발
1. 완료된 `42956290333` 영수증에서 **새 도구 호출을 실제 모델이 했는지**를 확인. 실패하면 처음 코드를 읽은 직후의 tool_choice, 공급자 응답 툴 형식, 줄 번호/해시 일치, 실제 실행_operation 실패 결과로 원인을 분리해 **엔진 코드** 수정.
2. 최초 유효한 도구 소스 패치를 얻으면 그 후보를 동일 원본과 독립 frozen baseline/holdout에서 실행. 채택은 품질·시간·회귀를 실제 계측한 경우만 인정.
3. 채택 바이너리의 후손 세대가 **그 능력으로 다음 패치와 추가 성능 향상**을 만드는지 2·3세대까지 돌리고 다른 문제/모델로 전이. 연구 등록만으로 완료 표시 금지.
4. Sub1의 최신 교정 재시도/지연 모델 구분, Sub2의 실제 로컬 모델 도구 호출과 Sub4의 전이 교정, Champion의 로컬 도구 왕복을 상호 비교해 필요한 부분을 자기 브랜치에서 결합. 이번 세션에서 다른 브랜치는 읽기만 함.

### 반복하지 말아야 할 실패
- GitHub Actions `success`를 실모델의 코드 채택 성공으로 혼동하지 말 것.
- 오래된 실험 워커의 `BASE_SHA` 바이너리를 실행해 최신 코드를 검증했다고 주장하지 말 것.
- `workspace_replace_range`의 부분 영수증만으로 인과적 품질 상승을 선언하지 말 것. 원본 SHA, 범위, 실제 readback, frozen quality 평가, 재빌드/계승은 서로 다른 단계다.


### 5세대 후속 핵심 결함 수정 — 자동 승인 누락
- `9519b46070d093a8694f2aebb247d23fdd8cbe16` `src/engine/rsi_evolve.rs::auto_write_allowed`: 신규 `workspace_replace_range`는 등록·provider 선택·직접 도구 실행까지 연결되었어도 **실제 RSI 쓰기 승인 자동 경로가 `workspace_write`/`workspace_replace` 두 가지만 허용**해 승인 대기/거부가 발생할 수 있었음. 현재 실행 중인 승인된 분리 작업공간, 살아 있는 캠페인·세대 작업, 안전한 소스 경로로 제한된 기존 자동 승인 조건에 새 도구를 추가. 같은 조건에서 새 도구 허용, 비허용 툴·경로·중단된 작업은 계속 차단하는 회귀 테스트 추가.
- `31565837ab2d5c0b5cf3b440ee9f70e4f048dec4` `scripts/sub3-live-model-campaign.mjs`: 모델 생성 코드가 사용한 편집 수단을 `rangeEditCalls`와 `legacyEditCalls`로 나누어 실제 영수증 집계. 이 변경으로 승인 누락을 수정한 최신 런타임의 실제 원격 모델 캠페인을 재시도: https://github.com/hushmind3/dominion-rust/actions/runs/42958230759 .
- 먼저 성공한 3번째 실행 CI: https://github.com/hushmind3/dominion-rust/actions/runs/42957662159 — **745/745 Rust 통과**, `research_survives_restart_and_executes_hash_checked_reuse_in_the_next_session` 실제 통과.
- 승인 누락 수정 CI https://github.com/hushmind3/dominion-rust/actions/runs/42958151269 와 최신 스크립트 연동 CI https://github.com/hushmind3/dominion-rust/actions/runs/42958230828 는 이 기록 시점 진행 중. 성공 전 통과/완주 주장 금지.
- 현재 브랜치는 Sub3 전용. 다른 4개 개발 브랜치나 `main`은 쓰지 않았다.
- **다음 세션 첫 실행**: `42958230759`의 실모델 세대별 `rangeEditCalls`·`legacyEditCalls`, `candidateId`, `acceptedCommit`, 독립 `improvedHoldouts`를 확인. 도구 호출 실패/승인 단계 실패/모델 선택 미발생을 구분하고 새로운 실행 코드로 수정. 결과가 없는데 채택·전이 개선을 추가했다고 기록하지 말 것.
