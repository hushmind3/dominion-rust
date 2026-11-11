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
