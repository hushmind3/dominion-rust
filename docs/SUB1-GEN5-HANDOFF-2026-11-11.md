# DOMINION Sub1 — 5세대 실행 인수인계 (2026-11-11)

## 역할·브랜치·기준
- 역할: Sub1 독립 개발자, 대상 브랜치 `dominion-sub1`. 다른 Sub/Champion, `main`은 수정하지 않음.
- 시작: `2384a507320d5551db11674b8cc06ebdd237aab5` (직전 CI 성공: https://github.com/hushmind3/dominion-rust/actions/runs/42946899378, Rust 731개).
- 기능 변경: `ea4899c` (후손 소스/연구 문맥 재계획), `5d84194` (E0502 소유권 오류 수정), `366fd91` (사용자 지정 소스 수정의 첫 세대 직접 실행).
- 실행 실험 추가: `9b231be` (`scripts/sub1-live-model-campaign.mjs`), `b896298` 및 `c6566e4` (`.github/workflows/sub1-live-model-cycle.yml`).
- 이 문서의 작성 시점에 최신 소스 기능 HEAD는 `c6566e416b7f7374c99424dd7386c88ef84fc9d1`.

## 이번 세션 실제 구현
1. `src/engine/rsi_evolve.rs`에서 부모와 재빌드된 후손의 목표/전략이 달라졌을 때 오직 mismatch로 종료하던 경로를 변경. 후손의 최신 도메인과 실제 인덱스를 사용해 소스 파일을 다시 고르고, 사용자 원본 요청·계승 경험·연구 근거를 보존해 프롬프트와 토큰 예산을 재구성한 다음 실행할 수 있도록 연결. 기존 계획/새 계획/파일 변경/문맥 구성 결과를 `rsiTargetReplan`에 기록.
2. 사용자가 구체적 소스 파일과 명확한 수정 목표를 전달한 첫 세대에서는 일반 조사 라운드를 우선하지 않고 `implement_research` 도구 실행을 요청. 이전 실패가 있으면 직접 실패에 따른 `change_approach`가 우선한다. 광범위 자율 탐색과 단순 조사 요청에는 적용하지 않음.
3. 재계획 시 연구 보존, 새 파일 포커스, 동일 계획의 중복 문맥 방지, 첫 세대 실행/후속 실패 교정에 대한 Rust 회귀 테스트 추가.
4. 다른 브랜치를 직접 열람. Champion의 후손 재계획, Sub2의 재시도 정책, Sub3의 무료 실모델 실험, Sub4의 확인된 방법 전이를 비교. Sub3의 실험 기반을 Sub1의 독립 실모델 측정 스크립트에 적용했음.

## 직접 확인한 실행과 범위
- 첫 `ea4899c` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42949439854 — 프론트 타입검사/빌드 성공, Rust `E0502` 1건으로 실패. 원인을 확인하고 `5d84194`로 수정.
- `5d84194` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42949616011 — 작성 시점 진행 중. 완료 결과를 직접 재조회할 것.
- `366fd91` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42950015168 — 작성 시점 진행 중.
- 최신 `c6566e4` CI: https://github.com/hushmind3/dominion-rust/actions/runs/42950166881 — 작성 시점 진행 중.
- 첫 실모델 실행: https://github.com/hushmind3/dominion-rust/actions/runs/42949753956 — 무료 `openai-fast` 실제 접속 프로브를 지나 빌드 단계에 진입함. 이는 편집/채택 성공을 의미하지 않음.
- 최신 실모델 실행: https://github.com/hushmind3/dominion-rust/actions/runs/42950166931 — `366fd91`의 첫 세대 직접 실행 정책을 포함한 소스로 시작. 반드시 최종 아티팩트에서 실제 도구 호출과 후속 평가를 확인할 것.
- 이번 세션 종료 전 실제 모델의 코드 편집, 독립 holdout 품질 향상 및 세대 계승이 확인되지 않았다면 성공으로 보고하지 말 것.
- 로컬 컨테이너에서는 GitHub DNS가 제한되고 Cargo/Rust 툴체인이 설치되지 않아 로컬 Rust 실행 대신 GitHub Actions를 사용.

## 직전 심판 평가
- 마지막 직접 읽은 2026-11-11 후속 기록: Sub1 34/100점, 5위 (다른 역할: Champion 39, Sub2 37, Sub3 36, Sub4 35). 현 코드 수정에 대한 새 독립 판정이 아님.
- 공통 원본 규칙은 40/30/20/10 항목별 부분 성과를 평가하며 근거 없는 점수 상한을 두지 않음. 90점 초과/Champion 초과는 역할 교체 조건임.

## 다음 최우선 순서
1. `c6566e4` CI와 최신 실모델 런 `42950166931`을 직접 확인하고 실패 원인을 수정. `model_reachable`이나 `workflow SUCCESS`만으로 RSI 완료라 부르지 말 것.
2. 실모델의 실제 `workspace_replace/write` 호출 → 파일 diff → `candidateId` → paired holdout의 품질·회귀·토큰 → accepted commit → 후속 재빌드와 사용까지 일관되게 추적. **첫 실모델 편집 0회라면 정확한 도구 선택/실패 지점부터 수정할 것.**
3. 동일 공급자·모델·시작 코드·예산·동결 holdout으로 시작점 `2384a`과 Sub1 최신판을 비교. 연구→편집 전환률, 첫 후보 도달 세대, 토큰/채택당 비용을 측정.
4. 모델이 생성한 실제 채택 패치를 다음 세대 바이너리로 재빌드하고 다른 문제까지 확장해 세대·작업 간 경험 일반화를 확인한다. 3세대 연속 채택·방법 자체의 개선은 아직 미완.
5. 실험에서 새 도메인 선택이 발생하면 `rsiTargetReplan`, `rsiFocusSelection`, `rsiParentContextFit`, `rsiWorkerContextFit`이 실제 새로운 소스 내용과 일치하는지 확인한다.

## 반복하면 안 되는 실패
- 읽기/검색/새 발견 수를 패치나 성능 개선으로 집계하지 말 것.
- 후손이 바꾼 목표 이름만 업데이트하고 이전 목표 파일의 발췌를 프롬프트에 남기지 말 것.
- 코드 컴파일 성공을 실모델 성능 향상의 증거로 대체하지 말 것.
- 다른 개발 브랜치의 상태를 임의로 덮어쓰지 말 것.

참조: `src/engine/rsi_evolve.rs`, `src/engine/code_index.rs`, `scripts/sub1-live-model-campaign.mjs`, `.github/workflows/sub1-live-model-cycle.yml`, `docs/SUB1-GEN4-JUDGE-FOLLOWUP-2026-11-11.md`, 프로젝트 공통 `DOMINION_COMMON_SOURCE.md`.

## 5세대 추가 라이브 결과 및 코드 보정
- 코드 `95c72f4efe8239015e189317823d3b70b5af90bf`: 명시적 수정 목표 탐지의 `goal.contains(*verb)` str-pattern 인수 명확화. 최신 실행 코드 기준.
- 최초 실모델 캠페인 (수정 전 `b896298`) GitHub Actions https://github.com/hushmind3/dominion-rust/actions/runs/42949753956 **workflow SUCCESS**, 실제 결과는 `status=completed, stopReason=no_progress`. 실모델 `openai-fast`, 1세대 `promptTokens=3095`, `completionTokens=1500`, `calls=1`, **toolsUsed=[], candidateId=null, accepted=false, newFindings=0**. 즉 모델 연결·캠페인 구동은 작동했지만 실행·개선은 실패. 이 실패 때문에 첫 세대에서 실행을 바로 요구하는 `366fd91`을 추가함.
- 첫세대 구현 정책의 실모델 실행: https://github.com/hushmind3/dominion-rust/actions/runs/42950166931 (source `c6566e4`)과 후속 https://github.com/hushmind3/dominion-rust/actions/runs/42950467997 (source `95c72f4`), 최종 outcome/실제 편집 도구 호출·패치·paired holdout 성공 여부를 확인하기 전엔 채택 성과로 간주하지 않음.
- 첫 후손 재계획 + 테스트 `5d84194`: https://github.com/hushmind3/dominion-rust/actions/runs/42949616011 **성공**. Rust **732/732 (16+614+3+99)** 통과, 웹 타입 검사·빌드 성공.
- 최신 전체 CI: https://github.com/hushmind3/dominion-rust/actions/runs/42950468115. 최종 status를 후속 확인.

## 실제 실행의 추가 결과와 다음 교정
- 첫 세대 직접 구현 정책이 들어간 `c6566e4`의 라이브 캠페인 https://github.com/hushmind3/dominion-rust/actions/runs/42950166931: **무료 실모델 실제 호출 5회**, prompt 16,586 tokens, completion 1,356 tokens, `code_search` **2회**, `workspace_read_range` **1회**, `newFindings=3`. 모델이 실제 수정 전에 HTTP 502 (`TimeoutError: The operation was aborted due to timeout`)로 중단됨. 결과 `needs_review`, 후보/채택/holdout 이득 0. Workflow SUCCESS 자체는 지능 개선 성공이 아니다.
- 위 실패를 반영한 `4f9982afa39d17f332ce302c2b8e710f0efb28ff`: `src/engine/jobs.rs`에서 사용자의 실제 편집 지시를 받은 RSI 세대가 **실제 차이 0·모델 오류 없음·작업공간 정합**으로 끝나면 동일 작업공간/세션에서 한 번 더 도구 기반 수정을 요청하도록 실행 루프를 확장. 모형 호출을 도구 실행으로 위조하지 않으며, 제공자 장애/소스 무결성 오류에 무한 재시도하지 않음. `noEditRetryAttempts` 측정 필드와 회귀 테스트 추가.
- `3fb9675484beaa3b482b8d542c53f2d62fe255bd` 실모델 워크플로에 `src/engine/jobs.rs` 감시 추가, `c1855831f6402c0687bb712af5272b10dc6c3b3c`은 프록시 타임아웃을 120→240초로 조정 (엔진 기본 non-room 제공자 제한 300초보다 짧게 유지). 무료 제공자 timeout에 의해 충분한 후속 모델 응답이 끊기는 현상 개선을 목표로 함.
- 후손 재계획 `5d84194` CI **732/732 통과**; 직접 수정 정책 `366fd91` CI https://github.com/hushmind3/dominion-rust/actions/runs/42950015168 **733/733 Rust 테스트 성공**, 웹 타입검사/빌드 성공.
- 후속 Rust CI: `4f9982a` https://github.com/hushmind3/dominion-rust/actions/runs/42951146157 ; `3fb9675` https://github.com/hushmind3/dominion-rust/actions/runs/42951164178 ; `c185583`은 그 이후. 최종 success/failure 재조회하고 실패면 코드를 수정할 것.
- 실모델 회귀: `95c72f` https://github.com/hushmind3/dominion-rust/actions/runs/42950467997 ; 무편집 복구 `3fb9675` https://github.com/hushmind3/dominion-rust/actions/runs/42951164189 ; 타임아웃 연장 `c185583` https://github.com/hushmind3/dominion-rust/actions/runs/42951533077 . 세 실행의 원본 `generationEvidence`에서 `toolsUsed`, `candidateId`, `acceptedCommit`, `evaluation.metrics`를 반드시 확인. 모델이 직접 패치하지 못했다면 누적 연구/호출 수를 개선으로 환산 금지.

## 확정된 5세대 최신 추가 실행 결과
- `95c72f4` 무료 실모델 캠페인 https://github.com/hushmind3/dominion-rust/actions/runs/42950467997 : `completed/no_progress`, **3세대** 시도. 1세대 1회 호출·도구 0; 2세대 6회 모델 호출·`code_search` 2회·`workspace_read_range` 3회·새 발견 5; 3세대 1회 호출·도구 0. **후보 0·소스 편집 0·채택 0·holdout 이득 0.** 모델이 실제 검색/읽기에 도달해도 코드 편집은 수행하지 않았다.
- `3fb9675` (한 번의 무편집 교정 코드 포함) 실모델 https://github.com/hushmind3/dominion-rust/actions/runs/42951164189 : `needs_review`, 1세대 3회 모델 호출·`workspace_read_range` 2회·새 발견 2, **제공자가 느린 모델을 건너뛰어 실행 종료**. 편집·후보·채택 0. 이 경우는 `modelError`가 존재해 무편집 교정의 **성공 사례가 아니며**, 모델 장애를 교정 성공으로 세지 않음.
- `3fb9675` 전체 CI https://github.com/hushmind3/dominion-rust/actions/runs/42951164178 : **734/734 Rust 테스트 통과(16+616+3+99), 실패 0**, 새로운 `patch_generation_retries_once_only_after_real_no_edit_outcome` 테스트 통과, 웹 타입검사·빌드 통과.
- 프록시 제한 240초 수정 `c185583` 실제 모델 실행 https://github.com/hushmind3/dominion-rust/actions/runs/42951533077 은 본 기록 시점 결과 미확인. 담당자는 이 실행의 최종 `candidateId`, `accepted`, `toolsUsed`, 도메인별 holdout을 최우선으로 확인.
- 공통 목표 관점: 런타임 실행 복구와 후손 재계획 코드는 확인되었지만 **실모델 독립 성능 향상 및 3세대 연속 개선은 미입증**. 다음에는 무료 제공자 병목을 분리한 동일 소스·예산·동결 holdout의 실제 모델 A/B를 해야 한다.


## 5세대 Sub1 후속 개발 — 실모델 실행 경로 결함 수정 (2026-11-11)

- **역할/브랜치:** Sub1 / `dominion-sub1`; 다른 Sub·Champion·`main` 변경 없음.
- **심판 최신 인계:** Sub1 `e59c643` 기준 **70/100, 4위**(구현 33, 완주 18, 자기개선 12, 일반화 7). 이 점수는 아래 새 커밋에 대한 독립 재채점이 아님.
- **이번 최신 코드:** `f5001dc` → `186f631` → `4c02281` → `66bd824` → `ed08cd7` → `37f5c50`. 각 커밋은 `dominion-sub1`에만 반영됨.

### 실제 구현 (설명이나 방어 규칙만 추가하지 않음)

1. `f5001dc`: `src/engine/jobs.rs`가 사용자 지정 소스 편집을 실제 행동 목표로 구분하고 `rsiExplicitFocus`·검증된 소스 발췌 여부를 세션에 전달. `src/engine/chat/runtime.rs`는 성공적 소스 읽기 뒤 코드 편집 단계로 전환하여 모델에게 `workspace_replace/write`를 선택하게 하고, 편집 대신 설명을 하면 한 번의 bounded continuation을 실행. `src/engine/chat/provider.rs`에서 그 단계의 native `tool_choice: required` 설정. 조사-only 일반 과제의 도구는 제한하지 않음.
2. 같은 커밋 `src/engine/rsi_evolve.rs`: 무료/로컬 제공자 지연과 정상 무편집 종료를 구분해 `provider_interrupted`로 기록. 실제 전송 중단은 모델이 편집을 거부한 것으로 학습하지 않고 이전 유효한 편집 지시를 유지. 예산/소스·세대 증거는 유지.
3. `186f631`: 첫 CI `42953097156`에서 발견된 기존 체크포인트 테스트의 누락 인자 2건 수정.
4. `4c02281`, `66bd824`: 실제 Qwen3 4B Ollama + DOMINION Rust의 별도 RSI 워크플로 `.github/workflows/sub1-local-ollama-rsi.yml` 및 실행 스크립트. 격리된 Git 소스 경로를 실제 개발 설정에 연결하고, 16K 컨텍스트와 모델/도구/실패 영수증을 남김.
5. **핵심 실제 병목 `ed08cd7`:** `src/engine/chat/provider.rs`가 60초 이상 걸린 모든 모델 응답에 `gateway::mark_slow`를 호출했고, 이 함수가 15분 동안 다음 생성 호출을 차단해 **소스 읽기 뒤 편집으로 넘어가지 못했음**. 방 임시 교체에 필요한 느린 모델 격리만 `roomId`가 있는 실제 회의 대화에 적용하고, 장시간 RSI/일반 작업의 다음 모델 호출은 유지하도록 변경.
6. **실측 HTTP 오류 수정 `37f5c50`:** 처음 만든 로컬 Ollama 워크플로에서 제공자 root `:11434`로 인해 `/chat/completions`이 HTTP 404를 반환. `src/engine/gateway.rs::resolved_base_url`에서 `kind=ollama`의 루트 경로를 일관되게 `/v1`으로 정규화하고 RSI/채팅 모두의 엔드포인트를 통일. Sub2의 동일 유형 수정과 테스트를 참고한 Sub1 고유 적용.

### 직접 확인된 실제 결과

- `186f631` CI `https://github.com/hushmind3/dominion-rust/actions/runs/42953416958`: **성공**, Rust **738/738** = benchmark 16 + unit 620 + extra 3 + E2E 99. 실패 0.
- `66bd824` CI `https://github.com/hushmind3/dominion-rust/actions/runs/42953724597`: **성공**.
- `f5001dc` 무료 모델 캠페인 `https://github.com/hushmind3/dominion-rust/actions/runs/42953097122`: `openai-fast` 2호출 / `workspace_read_range` 2회 / 실제 편집 0 / 후보 0 / 채택 0. `needs_review`, 느린 제공자 차단 메시지. `workflow success`는 RSI 개선 성공이 아님.
- `66bd824` Qwen3 4B 첫 로컬 캠페인 `https://github.com/hushmind3/dominion-rust/actions/runs/42953724579`: **모델 호출 0회**, 편집 0, `HTTP 404 Not Found`, `needs_review`. 이 장애 때문에 `37f5c50`의 /v1 URL 수정이 필수.
- 최신 코드 검증: `ed08cd7` CI `42954399657`, `37f5c50` CI `42954667598`는 이 인수인계 시점 결과 미확정.
- 최신 실제 Qwen3 캠페인: `37f5c50`의 `https://github.com/hushmind3/dominion-rust/actions/runs/42954667541` 진행 중. 결과가 나오기 전 편집/개선 성공을 주장 금지.

### 다음 세션이 즉시 수행할 행동

1. `37f5c50` GitHub CI 최종 빌드와 Rust 테스트 수를 확인하고 실패 시 **코드부터 수정**. 느린 모델 회의 격리와 장시간 RSI 실행을 동시에 회귀 테스트할 것.
2. `42954667541` 아티팩트의 `sub1-local-rsi.json`, `sub1-local-operations.json`, `sub1-engine.log`에서 네이티브 모델 호출 수, `rsi.action_checkpoint`의 `editOnly`, `workspace_replace` 시도/실패/성공, `candidateId`, `evaluation.metrics`와 채택 확인. **0회 편집이면 모델 도구 선택 단계의 실제 오류를 수정**; 독립 holdout 개선이라고 추정 금지.
3. 실제 수정에 도달하면 동결 holdout의 개선·회귀를 독립 비교하고, 후보 채택 뒤 새 바이너리에서 다음 세대가 같은 개선 기술을 활용하는지 2·3세대까지 실행. 다른 도메인 전이도 반드시 실제 도구/품질 영수증으로 확인.
4. 무료 제공자 지연으로 끝난 과거 `needs_review`를 정상 무편집 실패와 섞어 학습하지 않을 것. 모델이 편집할 수 있는 환경을 먼저 확보해 연구→실행→피드백의 루프를 완주할 것.


## 5세대 Sub1 — 실제 Qwen 쓰기 도구 최초 도달과 시간 예산 병목 수정 (2026-11-11)

### 현재 역할·담당 브랜치·기준
- 역할: Sub1 독립 개발자. `dominion-sub1`에서만 구현·실험. Champion/Sub2~4/`main` 수정 없음.
- 독립 심판 최신 공개 인계: `e59c643` 기준 70/100, 4위(구현 33, 완주 18, 자기개선 12, 일반화 7). 이 뒤 변경에 대한 신규 심판 점수는 미확정.
- 추가 구현: `4dd9fba` → `236d32b` → `3ed1c77`. 최신 확인할 실험 커밋은 `3ed1c77f9b56bbb9d91e8089df40b71374aae0dd`.

### 이번에 실제 완료한 코드 작업
1. `4dd9fba` `src/engine/chat/runtime.rs`: 명시된 소스 편집 과제에서 검증된 코드 발췌가 이미 모델 입력에 있으면 첫 호출의 도구를 `workspace_read_range`, `workspace_replace`, `workspace_write`에 집중. 실제 근거가 없는 연구/자율 탐색은 검색 경로 보존. 회귀 검증 추가.
2. `236d32b` `.github/workflows/sub1-local-ollama-rsi.yml`: 상기 런타임·작업 실행기·RSI 소스 변경 시 **실제 로컬 Qwen 캠페인**이 다시 실행되도록 트리거 연결. CI 성공과 실모델 RSI 성공은 별개로 기록.
3. `3ed1c77` `src/engine/chat/runtime.rs`: 일반 대화 10분 제한을 보존하면서 **실제 RSI 캠페인 내부의 세대 채팅 실행은 14분**까지 허용. 기존 부모 `rsi_evolve.rs` 워커 상한 15분보다 짧게 유지해 결과 수집 시간을 확보. 외부 캠페인/사용자 취소·한도는 계속 적용. 회귀 테스트 추가.
4. 같은 커밋 `scripts/sub1-local-ollama-rsi.mjs`: 실제 CPU Qwen 실험이 패치 생성→동결 A/B 평가까지 시간을 쓸 수 있도록 캠페인 1,200→2,100초, 독립 러너 관측 22→35분으로 확장.

### 직접 확인한 실제 모델 성공과 실패
- 수정 전 실제 로컬 Qwen3 4B/Ollama/Rust 캠페인: [Actions 42954667541](https://github.com/hushmind3/dominion-rust/actions/runs/42954667541), `37f5c50` 소스. 워크플로 자체 성공, **모델 호출 3회**.
- SQLite `execution_operation` 실측: `code_search` completed 1, **`workspace_write` completed 1**, `workspace_read_range` completed 1. `rsi.action_checkpoint`가 `editOnly=true`로 실행됨. 과거 0회 쓰기 도구 호출이라는 병목에서 실제 전진.
- **후보 0, verified 채택 0, `needs_review`**, 오류 `실행 시간 제한(10분)을 초과했습니다.`. `workspace_write` operation completed가 곧 변경 diff/유효 후보를 의미하지는 않음. 실제 파일의 diff와 write 도구 `result.written` 세부 확인은 남아 있음. 런타임이 종료되며 detached 후보 수집 단계에 도달하지 못한 점이 직접 수정 대상.
- `37f5c50` 전체 CI [42954667598](https://github.com/hushmind3/dominion-rust/actions/runs/42954667598) 성공. `654eba82` CI [42954985071](https://github.com/hushmind3/dominion-rust/actions/runs/42954985071) 성공.
- 최신 `3ed1c77` CI [42956630103](https://github.com/hushmind3/dominion-rust/actions/runs/42956630103) 및 로컬 모델 재실험 [42956630085](https://github.com/hushmind3/dominion-rust/actions/runs/42956630085)은 **실행 시작 확인, 결과 미확정**. 추가 `236d32b` 실험 [42956141806](https://github.com/hushmind3/dominion-rust/actions/runs/42956141806)도 별도 비교 대상.

### 다음 최우선 과제 / 아직 미완성
1. `42956630103` CI 실패 시 **실제 오류**를 즉시 수정; `42956630085`의 최종 SQLite 영수증, 모델 호출, 쓰기 도구 `result` 및 `candidateId` 확인. `workspace_write` 완료가 에러 응답인지 파일 변경인지 구분.
2. 코드 diff가 생성됐다면 고정 planning-quality holdout의 **baseline 0.75 → candidate 1.0** 등 독립 수치가 실제로 증가했는지 paired receipt·회귀·eligible·채택을 검증. 아직 개선 수치를 실측 성공으로 주장하지 않음.
3. 첫 유효 후보 채택→수정된 코드의 다음 실행기 재빌드→두 번째/세 번째 세대 계승과 다른 도메인 전이를 실모델로 증명.
4. 연구만 수행하거나 모델 응답이 중단된 사례를 지능 성장으로 집계하지 말 것. `workspace_write` 호출, 실제 파일 diff, 독립 검사 통과, 채택과 다음 세대 계승을 구분할 것.

참고: `src/engine/chat/runtime.rs`, `src/engine/chat/provider.rs`, `src/engine/jobs.rs`, `src/engine/rsi_evolve.rs`, `scripts/sub1-local-ollama-rsi.mjs`, `.github/workflows/sub1-local-ollama-rsi.yml`, `src/engine/rsi_oracle_fixture.rs`.
