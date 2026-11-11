# DOMINION Sub2 — 2026-11-11 development handoff

## 현재 세대 / 역할 / 담당 브랜치 / 기준 커밋

- 4세대(2026-11-11) / Sub2 개발자 / `dominion-sub2`
- 시작: `7cee3e78184aef13c10b5120328a60f54920611e`
- 코드 수정 HEAD: `4ca98563342fb9eb274e59f23cf091b080610ac2`
- 다른 개발 브랜치 및 `main`·`dominion-champion`은 수정하지 않았음.

## 현재 목표와 핵심 미완성 능력

실패한 자율 검색 알고리즘 시도가 종료 상태에서 영구 반복/정체하지 않고,
실제 실행한 실패 이유에 따라 *다른 실행 가능 정책*을 만들어 다음 세대에서 사용하도록 한다.
후보 탐색 정책의 실제 이득은 동일 대상·동일 코드 계보의 active-policy 대조군과
held-out 평가를 통과한 경우에만 인정한다.

범용 하네스의 완전한 RSI 능력 입증(연속 개선 3세대, 메타 전이, 모델/작업 교차 도메인
성과)은 아직 미완성이다. 이를 구현 완료로 표시하지 않는다.

## 이번 세션의 실제 코드 변경

1. `491ddf2` — inconclusive/rejected/adopted trial을 pending 후보로 무한 재사용하지 않도록 분리,
   현재 캠페인에 실패한 trial이 있으면 서로 다른 후보의 실험을 최대 3개까지 재설계.
   실제 trial outcome이 저장된 **뒤** 재설계하므로 조기 종료 대신 다음 세대에 후보 전달.
2. `508d3ad` — 새 후보가 대기 중일 때 4번째 무성과 세대가 즉시 캠페인을 종료하지 않도록 수정.
   연속 6회 무성과 상한과 사용자의 시간/토큰/시도 예산은 유지.
3. `21f43fd` — targetExpression이 전혀 실행되지 못한 기록이 있으면,
   다음 실험은 domain-only 계산식을 달리 구성해 실제 정책 실행 가능성을 회복.
4. `eb080ab` — 다른 브랜치의 control-domain pin 개념을 연구한 후 Sub2에 적용.
   기존 대조군의 대상 도메인을 정책 id·revision에 결속하고,
   정확히 그 후보를 위한 active-policy 대조군만 trial 비교에 사용.
   불필요한 다른 정책의 무패치 실행을 대조군으로 취급하지 않음.
5. `203355a` — 검증 대기 중인 실행 가능한 정책은 연구-읽기 연장보다
   대조 실행/실험을 먼저 받아, 짧은 캠페인에서도 실제 시도를 진행.
6. `4ca9856` — GitHub CI (`42935277282`, `42935397804`)가 발견한 기존 테스트 2건의 오류 수정.
   후보별 대조군 결속을 요구하도록 설계가 강화되었으나 기존 테스트는 소유 후보 ID를
   명시하지 않아 잘못된 기대를 가지고 있었다. 2개 테스트의 대조군 fixture에
   후보 ID와 revision을 명시해 실제 엔진의 실행 조건과 동일하게 구성했다.

각 수정에 Rust 단위/회귀 테스트 추가. 파일:
`src/engine/rsi_evolve.rs`.

## 직접 확인한 실행 결과

- 개발환경 container에는 `cargo` / `rustc`가 없고 외부 git clone DNS가 차단됨.
- GitHub Actions CI `42934266272` (`491ddf2`), `42934569627` (`508d3ad`),
  `42934914237` (`21f43fd`): **success**. `21f43fd` 로그에서
  프론트 typecheck/build, bash syntax, Rust 총 744건(16+626+3+99) 통과 확인.
- 코드 `eb080ab`과 `203355a`의 첫 CI는 **기존 fixture 2개가 새 후보 ID 결속을 누락**해 실패했고,
  `4ca9856`에서 실제 실패 로그를 읽고 고쳤음.
  최신 코드의 최종 CI 결과는 반드시 후속 확인 필요.
- 후속 커밋들은 별도 GitHub CI 작업으로 실행됨. 반드시 최신 HEAD CI의
  최종 상태를 확인하고 실패 시 테스트 로그 기반으로 고칠 것.
- 저장된 실제 다세대 LLM 캠페인이나 실제 모델 A/B 이득은 이번 세션에서
  측정하지 못함. CI 통과를 '지능 성장 입증'으로 혼동하지 말 것.

## 다른 개발자/Champion에서 직접 검토한 장점

- Sub1 `780b456`: 연구→구현으로 이어지는 1회의 corrective generation.
- Sub3 `2d6e1b5`: 정책별 control domain pin 및 실패 데이터 기반 탐색.
- Sub4 `e1e47d5`: 실행 가능한 방법의 검증·전이와 indexed source 재계획.
- Champion `a2c49bc`: 보존된 채택 증거, frozen holdout 비교 및 실제 반영 통제.

Sub2는 후보별 control-domain pin을 채택했지만, 전 브랜치를 병합하거나
타 브랜치를 수정한 것은 아님.

## 남아 있는 중요한 미완료 작업

1. GitHub CI의 **최신 HEAD** 최종 결과 확인, 회귀가 발생했다면 즉시 수정.
2. 실제 모델과 동일 시작 상태·동일 모델·예산·held-out 사례에서
   active policy vs generated candidate policy를 실행하여
   실제 도달률/패치 채택률/토큰/시간을 비교할 것.
3. 시험이 `comparable_baseline_missing`으로 계속 끝난다면
   활성 전략의 **대조 실행 자체**가 유효한 패치/측정행을 생산하도록
   비교 입력을 재설계한다. 기존 대조군 실패를 성공으로 간주하지 말 것.
4. 실제 3연속 채택, 개선기 자체 개선, 새 도메인 전이를 확인하고
   `capabilityDemonstrated`을 참인 경우에만 보존할 것.
5. Sub4의 검증된 실행 방법 전이를 Sub2의 목표 재계획에 연결할지
   실측 성과를 기준으로 검토할 것.

## 막힌 원인과 반복하지 말아야 할 실패

- `trial_inconclusive` 정책을 무한히 다시 선택하면
  새 전략을 실제 실행할 기회를 없앤다.
- 새 전략을 예약만 하고 no_progress로 종료하면 성과 0이다.
- 생성기의 진행 도중 후보별 소스 도메인을 새로 산정하면
  대조군과 실험군이 달라져 비교 불가 상태가 된다.
- 후보가 패치를 만들거나 토큰을 적게 썼다는 이유만으로
  인과적 알고리즘 향상을 선언하지 않는다.
- 테스트 수·정책 금지 규칙 자체를 정체성 구현의 성과로 대체하지 않는다.

## 직전 심판 피드백

- 이전 공개된 Sub2 판정(과거 `ecfbba4` 기준): **85/100, 3위**.
- 강점: 표현식 기반 코드 탐색, 세대 재계획, 경험 재사용 및 원자적 소유권.
- 남은 핵심: 인과적 A/B·held-out 실증, 실제 반복 성장.
- 이는 이번 HEAD에 대한 새 심판 채점이 아니므로 순위 갱신 주장 금지.

## 다음 세션이 바로 시작할 작업

`dominion-sub2` HEAD와 최신 CI 결과부터 확인한 뒤,
실제 한 모델에서 동일 문제·소스 스냅샷·예산을 고정해 대조군과 생성 정책을
각각 실행한다. 결과가 불일치하거나 실험을 시작하지 못하면,
원본 실행 로그의 실패 원인을 수정해 *둘 다 실제 도구 실행에 도달*시키는 것을
최우선으로 삼는다. 검증된 결과는 Champion 통합용 제안으로만 공유한다.

## 참고 위치

- `src/engine/rsi_evolve.rs`: 정책 생성·대조군·실험·재시도/캠페인 실행
- `src/engine/rsi.rs`: frozen held-out 평가 및 원본 측정행 기반 재귀 성능 증명
- `src/engine/rsi_worktree.rs`: 별도 Git worktree 평가 및 검증된 변경 반영
- `web/src/pages/SelfImprovement.tsx`: 사용자 실행 설정 및 실험 가시화
- `docs/rsi-completion.md`: 이전 라이브 캠페인 측정 기준선
- `DOMINION_COMMON_SOURCE.md`: 프로젝트 공통 정체성 문서(대화에서 제공됨)


## 2026-11-11 5세대 Sub2 — 완주 중심 심판 37점 피드백 반영

- 심판의 최신 동일 척도는 **핵심 구현 22/40, 완주 8/30, 자기개선 4/20, 일반화 3/10 = 37/100, 2위**.
- 현재까지 인용된 실측: 캠페인 21개, 시도 41세대, 채택 1회, 최대 연속 채택 1세대.
  검증된 성능 증가와 개선기 세대 계승은 아직 없다.
- 본 세션에서 새로 추가:
  1. `69571924`: `scripts/sub2-live-rsi.mjs` — 실제 DOMINION HTTP API와 로컬 Ollama 모델로 자기개선 캠페인을 시작해 소스 변경·독립 paired holdout 측정·후속 바이너리 계승을 추적하고 JSON 증거를 저장. 모델이 생성한 텍스트는 도구 실행으로 허위 계상하지 않음.
  2. `7788efa2`: `.github/workflows/sub2-live-rsi.yml` — 독립 러너에 실모델 `qwen3:4b-instruct` 및 Rust 엔진을 올려 평가. 개선 입증 실패 시 CI 실패와 증거 artifact 남김.
- 최초 실행: GitHub Actions `42944539721`, 소스 커밋 `7788efa2531bd0109d6b4285bb02eaf443dd4f5c`. 실행 중인 상태를 성공으로 보고하지 말 것.
- 결과 판정: `firstMeasuredImprovement`은 **실제 모델이 생성한 패치가 실측 eligible/held-out 개선/0회귀/paired receipt 조건을 만족할 때만 true**.
  `descendantInheritedImprovement`은 후속 세대가 채택 커밋에서 다른 바이너리 SHA로 실행했을 때만 true.
- 이 실험은 모델 1종의 실제 개선 도달 여부를 보는 첫 단계. 탐색 정책끼리 동등한 입력에서 비교하는 인과적 A/B 실증은 별도 미완료 작업.

### 다음 우선 작업

1. `42944539721`이 종료되면 artifact의 `live-rsi-receipt.json`과 엔진 로그를 읽고 가장 먼저 실제 결과를 해석한다.
2. 후보가 없다면 조사에 그치는 병목(도구 호출 실패/문맥/모델 출력/타깃 선택)을 수정한다.
3. 후보는 있었으나 동결 holdout 성능이 개선되지 않았다면 실제 평가의 점수·도메인과 수정 대상을 연결한다.
4. 첫 검증 개선이 나오면 같은 계보에서 **재빌드된 후속 개선기가 새 패치를 만들고 독립 평가에서 또 개선**하도록 계속한다.
5. 개선을 한 번 채택했다는 사건만으로 40점 이상 혹은 RSI 완주를 선언하지 않는다.


## 2026-11-11 Sub2 후속 — 실제 Ollama 실행 장애 3단계 수정과 다음 완주 목표

### 담당 및 비교 기준
- 현재 역할 **Sub2**, 담당 브랜치 **dominion-sub2**. 본 회차 시작 `873a91e34f`.
- 최신 공통 심판 기준: **37/100, 2위** (구현 22/40, 완주 8/30, 자기개선 4/20, 일반화 3/10). 점수 재채점은 심판의 몫이며 이번 구현만으로 점수를 올려 쓰지 않음.
- Champion의 실패 결과→실행 행동 지시, Sub1의 연구/실패 인계, Sub3의 실제 목표 경로의 코드 향상, Sub4의 실행 가능한 다른 도메인 절차 전이를 조사하여 개발 목표를 정함. 타 브랜치 직접 수정 없음.

### 실제 수정 커밋
1. `633d529`: Ollama URL을 /v1/chat/completions에 일관되게 정규화, regression test.
2. `6f1394c`: gateway 변경이 실제 로컬 모델 캠페인을 자동 재실행하도록 workflow 경로 연결.
3. `1def363`: API 키 없는 Ollama 연결 테스트 및 native /api/tags 모델 조회, HTTP 모의서버 regression.
4. `6d0841f`: 세대의 직접 관측된 성공/실패/코드 편집 횟수를 다음 세대의 **서로 다른 실행 행동** (implement_research / repair_failed_case / revise_candidate / complete_candidate / change_approach / extend_verified_capability)으로 연결. 현재 생성기와 후손 worker가 동일한 저장 기록을 기준으로 지시를 재구성.
5. `ac73f66`: 조사만 수행→실제 편집 지시, 편집했으나 후보 없음→후보 완성, holdout 실패→다른 수정 및 반복 실패 시 접근 변경, 실제 편집 도구 영수증 검사 회귀 테스트.
6. `8ad1e11`: 실제 Qwen 모델의 Ollama /api/chat preflight가 native code_search tool call을 반환하면 DOMINION 제공자를 native tool mode로 등록. 기존 compat 강제의 텍스트 도구 안내 한계를 제거.
7. `57a262c`/`801f7d3`: 자기개선 worker(최대 900초) 안에서 CPU 로컬 모델이 실제 토큰을 반환하도록 RSI 한정 840초 요청/라운드 제한. 일반 채팅·그룹 제한 유지. 최초 회귀 테스트 import 컴파일 오류 E0432를 즉시 고침.
8. `b24d2c6`: 작은 Ollama 모델은 실제 색인 소스 발췌를 6000자로 제한하고 나머지는 workspace_read_range로 읽도록 연결. 과도하게 긴 발췌가 통째로 사라지는 문제도 수정, regression.
9. `f8c01a6`: CI Ollama 설치기가 먼저 띄운 **기본 4096토큰 systemd 서비스**를 종료하고 작업 러너의 OLLAMA_CONTEXT_LENGTH=16384를 가진 서버를 실행. 문맥 초과 400을 근본 교정.
10. `cc44bfc`: 실모델 사전실행에 num_ctx 16384 명시, /api/ps에서 실제 상주 컨텍스트 기록·최소 크기 검사. 세대별 도구 호출/실제 편집/프롬프트 길이/선택된 행동을 원본 영수증에 저장.

### 실제 실모델 결과와 수정 원인
- [42944539721](https://github.com/hushmind3/dominion-rust/actions/runs/42944539721) 최초: Qwen3:4b-instruct Ollama native preflight에서 **code_search tool call 1회 실제 생성**. DOMINION 자체 호출은 잘못된 경로의 HTTP 404, 후보 0/편집 0.
- [42949302635](https://github.com/hushmind3/dominion-rust/actions/runs/42949302635): /v1 경로 수정으로 실제 스트리밍 HTTP 진입. **300초 후 stream decoding error**; 세대 1회, 토큰 0, 도구 호출 0, 채택 0. 이때는 Qwen의 native 지원에도 제공자를 compat으로 강제해 도구 스키마 안내 비용이 큼.
- [42950399582](https://github.com/hushmind3/dominion-rust/actions/runs/42950399582): native mode 연결 후 **HTTP 400: 요청 4288토큰 / 서버 실제 한도 4096토큰**. Ollama 시스템 서비스가 workflow의 OLLAMA_CONTEXT_LENGTH=16384를 적용받지 않는 직접 원인. 후보·채택 0, 실제 향상 증거 없음.
- 최신 context 서비스 수정/실모델 증거: [42951534602](https://github.com/hushmind3/dominion-rust/actions/runs/42951534602)와 [42951576225](https://github.com/hushmind3/dominion-rust/actions/runs/42951576225). **종료 후 아티팩트 `sub2-native-live-rsi-<run_id>`를 열어 실제 model context, toolsUsed, actionObservation, candidateId, acceptedCommit, paired holdout gain을 먼저 확인. 현재 결과를 미리 성공으로 기록하지 말 것.**
- CI [42950399563](https://github.com/hushmind3/dominion-rust/actions/runs/42950399563): `8ad1e11` Rust/웹 전체 성공. 후속 `57a262c`/`b24d2c6` 중간 CI에서 새 테스트의 모듈 import 오타 E0432 발생해 `801f7d3`에서 수정. 최신 결과는 [42951576269](https://github.com/hushmind3/dominion-rust/actions/runs/42951576269)를 확인할 것.

### 다음 세션이 할 가장 중요한 일
1. 최신 CI와 두 실모델 워크플로의 **완료 아티팩트를 확인**하고 실패 지점을 수치로 분리. 불명확한 실패를 성공으로 포장하지 말 것.
2. 실제 Ollama context가 16384 이상인데 후보가 안 나오면 **Qwen의 native structured tool dispatch → code_search → workspace_read_range → workspace_replace → 실험 후보 생성**을 먼저 실제 실행하게 고친다. 도구 호출의 존재와 파일 변경 성공은 별개.
3. 진짜 수정 후보가 나오면 **분리된 동결 paired holdout의 개선/퇴보/실행비용**으로 평가. 미달 원인에 따라 다음 세대에 정확한 수정 행동을 넘기고, 재빌드된 후손이 이전 능력을 계승하여 새 문제를 해결하는지 실측.
4. 성과가 나오면 Champion이 통합할 수 있도록 **실제 커밋·워크플로·영수증·성능 수치**를 전달. 실패했으면 동일한 실행 환경에서 다른 원인 수정 후 재실행할 것.

실행 근거 경로: `scripts/sub2-live-rsi.mjs`, `.github/workflows/sub2-live-rsi.yml`, `src/engine/gateway.rs`, `src/api/providers.rs`, `src/engine/chat/runtime.rs`, `src/engine/rsi_evolve.rs`; GitHub Actions 해당 run의 업로드 아티팩트.


## 2026-11-11 5세대 Sub2 — 심판 69점 피드백 이후 실제 개선과 인수인계

### 역할 / 담당 / 최신 코드
- 역할: **Sub2**, 담당 브랜치 **`dominion-sub2`**. 심판의 확인 기준 `00d6e66`에서 이어서 개발.
- 심판 직전 점수: **69/100, 5위** (핵심 구현 34/40, 완주 17/30, 자기개선 12/20, 일반화 6/10). 이번 변경의 신점수는 심판 재검사 전이므로 추정하지 않음.
- 코드 변경 HEAD: **`641cbdf19041674d56f4c551f92c0fcb76b6e2ab`**. 타 브랜치, main은 수정하지 않음.

### 발견한 실제 실패
- 실모델 Qwen3 4B [run 42951576225](https://github.com/hushmind3/dominion-rust/actions/runs/42951576225) 최종 **failure**, 실제 영수증 status `needs_review`, accepted 0, source edit 0. Ollama 상주 컨텍스트 **16384**, native `code_search` 도구 생성 **1회**. 실패 이유는 `최근 응답이 너무 느렸던 모델이라 잠시 건너뜁니다.`.
- 원인: `chat/provider.rs`가 정상 완료 여부나 RSI 특성과 무관하게 60초 이상 진행된 모델 호출을 15분간 slow quarantine에 등록, 같은 도구 작업의 후속 요청을 스스로 차단.
- 기존 Champion [run 42951558544](https://github.com/hushmind3/dominion-rust/actions/runs/42951558544)에서는 실제 로컬 `workspace_list` 왕복+저장을 이미 입증했음. 이것을 RSI 소스 수정 성공으로 오인하지 않음.
- Sub3의 지속되는 편집 전용 도구 단계와 Sub4의 연구용 RSI 강제 쓰기 회귀 및 교정 기록을 비교해 적용 범위를 좁힘.

### 실제로 Sub2에 반영한 코드
1. **`10cf76c`**: `provider.rs` slow quarantine을 대화형 회의실 응답 지연에만 적용해 정상 로컬 장시간 RSI 다음 라운드 차단을 해제. 실제 Rust 채팅 RSI는 60초 초과해도 후속 모델 호출을 이어갈 수 있는 경로를 제공. `provider.rs`에서 native 편집 전용 도구 메뉴에 `tool_choice=required` 적용. `runtime.rs`에서 소스 읽기 관측 후 다음 라운드에도 edit-only 메뉴를 유지.
2. **`f79b5a7`**: 기존 연구용 RSI E2E 1건 회귀를 실제 CI 실패로 발견한 뒤, edit-only 진입을 사용자가 지정한 소스의 **실제 세대 작업**으로 제한. 일반 연구 RSI가 방법 제안 및 변경 없는 종료를 계속 수행할 수 있게 복구.
3. **`6b57928`**: 집중 소스 개선 세대가 소스를 읽고도 설명만 남기면 같은 턴에서 최대 1회 추가 실행 기회와 edit-only 메뉴를 제공. 실제 패치 생성/성공을 조작하지 않음.
4. **`dcd87e9`**: 모델에 이미 전달된 인덱스 기반 소스 발췌(`rsiFocusContextChars>0`)를 세대 세션에 기록해, 중복 검색 없이 이 근거에서 바로 1회 수정 시도로 전환 가능. 일반 연구와 무근거 편집은 제외.
5. **`641cbdf`**: 실제 모의 HTTP 모델 요청에서 집중된 RSI 소스 편집 메뉴에 `tool_choice: required`가 전송되고, 일반 채팅에는 전송되지 않는지 검증하는 통합 경로 테스트.

### 실행·검증 현황(작성 시점)
- 초기 `10cf76c` CI [42954705217](https://github.com/hushmind3/dominion-rust/actions/runs/42954705217): **실패 1건**, `rsi_context_is_not_reinterpreted_as_tasks_or_forced_into_a_write`. 이유는 연구형 RSI에도 edit-only 적용된 범위 오류.
- 수정 `f79b5a7` CI [42955076641](https://github.com/hushmind3/dominion-rust/actions/runs/42955076641): **success**.
- 수정 `6b57928` CI [42955262103](https://github.com/hushmind3/dominion-rust/actions/runs/42955262103): **success**.
- `dcd87e9` CI [42955861963](https://github.com/hushmind3/dominion-rust/actions/runs/42955861963), 최신 `641cbdf` CI [42956089564](https://github.com/hushmind3/dominion-rust/actions/runs/42956089564): **작성 시점 진행 중. 최종 결과 재확인 필수**.
- 실모델 전 과정: `10cf76c` [42954705461](https://github.com/hushmind3/dominion-rust/actions/runs/42954705461), `6b57928` [42955262116](https://github.com/hushmind3/dominion-rust/actions/runs/42955262116), 최신 `641cbdf` [42956089722](https://github.com/hushmind3/dominion-rust/actions/runs/42956089722) 등 **작성 시점 진행 중**. 실제 편집/후보 채택은 성공으로 주장하지 않음.
- 실행 환경에는 cargo/rustc가 없고 Rust 실험은 GitHub Actions가 맡음. 모델 호출 성공, 도구 요청/실제 실행, 목표 완주, 독립 held-out 개선을 각각 다른 상태로 기록.

### 다음 세션 최우선 — 실제 완주를 향한 정확한 진행 순서
1. **최신 `641cbdf` CI 로그와 실모델 아티팩트**를 확인. CI 실패는 해당 원인의 Rust 코드를 고치고 동일 브랜치에 재실행. 실모델의 `history[].toolsUsed`, `actionObservation`, `candidateId`, `acceptedCommit`, `evaluation.metrics`를 직접 확인.
2. 모델이 실제 편집 도구를 호출하지 않았다면 `rsiFocusContextReady`, `rsiExplicitFocus`, `rsiGenerationJobId`, `rsi.action_checkpoint`, 모델에 전송된 native `tool_choice`와 후속 `workspace_replace` 이벤트를 **같은 세대의 실제 증거**로 대조하고 해당 단계 수정.
3. 모델이 수정 요청을 했는데 적용이 실패했다면 `workspace_replace`의 정확한 `oldText`와 거절 결과를 다음 시도에 전달하는 도구 실패 교정부터 해결.
4. 실제 소스 패치가 생성되면 동결된 baseline/candidate paired holdout에서 작업 품질·속도·회귀·토큰을 재측정. 독립 성능 개선을 채택한 뒤 다음 세대의 **새 바이너리**에 인계, 다시 다른 개선을 만드는지 증명.
5. 문서·테스트만 늘리지 말고 실제 모델 `편집 0회` 병목을 없애는 것을 최우선으로 유지. 대량 재시작·무편집 루프·성공 조작 금지.

참고: `src/engine/chat/provider.rs`, `src/engine/chat/runtime.rs`, `src/engine/jobs.rs`, `src/engine/rsi_evolve.rs`, `scripts/sub2-live-rsi.mjs`, `.github/workflows/sub2-live-rsi.yml`.


## 2026-11-11 Sub2 — 심판 39점(5위) 이후 코드 중심 개선

### 판정과 실측 병목
- 최신 심판: **39/100, 5위** (구현 21/30, 실제 자기개선 3/40, 계승·일반화 9/20, 서비스 통합 6/10). 자리 교환 없음.
- 실제 [42956089722](https://github.com/hushmind3/dominion-rust/actions/runs/42956089722) 아티팩트 원본: 1세대 Qwen3 4B 모델 호출 5회, 27,835토큰, code_search 1회, workspace_read_range 2회, workspace_write 완료 호출 1회, 유효 후보 0. 2세대 11,731토큰, workspace_read_range 1회, 편집 0회, timeout. 합계 39,566토큰, 채택 0, 후손 개선 0.
- 이전 영수증의 toolsUsed는 성공 상태의 도구 횟수만 집계. 실제 Git diff와 상태별 도구 영수증이 없어서 쓰기 시도가 실패/무변경/컴파일 실패 중 무엇인지 분리하지 못했다.

### 담당 브랜치에 반영한 코드
- `87f2cf4`: `jobs.rs`에서 사용자 지정 focusFiles가 있는 연속 RSI도 편집 체크포인트를 활성화. 자동 탐색/연구 작업은 별개.
- `1802fcf`: `tools.rs`에서 blocked write를 성공한 replace로 표기하지 않도록 교정.
- `9d32711`: `rsi_worktree.rs` 후보 증거에 실제 편집 도구 상태·경로·written·오류·staleVersion·preflightBlocked 기록.
- `15db7e4`, `61d99a4`: `rsi_evolve.rs`에서 편집 시도·실제 성공·Git diff·후보 채택 조건 분리. 실패한 쓰기는 새 `repair_failed_edit` 행동 및 원본 오류가 들어간 다음 세대 지시로 전달. 0바이트 diff가 확인되면 실패한 편집의 도구 오류로 다음 패치를 교정.
- `aaabac2`: 기존 Rust 내장 `workspace_replace`에 `startLine/endLine/expectedSha256/newText` 모드 추가. `workspace_read_range`의 `sourceSha256`에 결속, 최대 120줄, 기존 UTF-8 원문/CRLF 유지, CAS 불일치 시 수정 거부. 원문 전체 재생성이 불필요한 부분 편집.
- `58bf994`, `91d0174`, `476910b`: 사용자 지정 집중 실행의 프롬프트와 모델 자체 도구 메뉴에 줄 범위 수정 안내 연결.
- `b9db27e`, `1e48512`: 작업 세션에 집중 파일을 보존해 기존 소스 개선 단계에서는 새 파일 생성용 `workspace_write` 대신 `workspace_replace` 선택하도록 축소.
- `2a8a31d`: 격리 RSI에서 기존 파일을 `workspace_write`로 통째로 덮으려는 시도는 명시적으로 거절하고 범위 수정을 제시. 정상 채팅/신규 파일 생성에는 영향 없음.
- `7b74b67`: 실모델 receipt에 editActions, sourceDiffBytes, captureError, 미검증 후보, 정적 검사 결과를 구분하여 포함.
- `20a9784`: SHA 일치 부분 수정/지정 범위 변경/오래된 해시 거절 회귀 테스트 추가.
- `bc51046`: CPU 기반 실모델 라이브 워크플로는 최신 커밋만 실행하도록 concurrency 추가.

### 검증과 남은 핵심
- 코드 수정 중 초기 CI `87f2cf4`, `1802fcf`, `9d32711`은 success 관측. 후속 Rust/실모델 Actions는 이번 인수인계 작성 시점 일부 queued/in_progress. 최신 HEAD 통과·실모델 개선 결과를 미리 주장하지 않는다.
- 최우선: 최신 실제 receipt에서 `editActions[].written`, `sourceDiffBytes`, `unverifiedCandidateId`, `staticChecksPassed`, `candidateId`, `acceptedCommit`을 직접 확인. 줄 범위 편집이 실제 Git 패치/후보/동결 paired holdout으로 연결되었는지 검증한다.
- 후보가 형성되었으나 빌드 실패 시 컴파일러 진단을 다음 세대에 전달해 1개 패치로 복구. 후보가 채택되면 반드시 새 바이너리 계보에서 **추가로 다른 개선을 만들어** 2세대 이상 실측. 아직 RSI 완주/품질 향상/39점 상향을 주장하지 않음.
- 다른 브랜치 및 main은 변경하지 않음.

## 2026-11-11 — 심판 44점 이후: DOMINION 실행 전략의 자기 진화

심판의 새 핵심 기준은 모델 코드 작성 능력이 아닌 DOMINION이 경험으로 자신의 실행 알고리즘을 바꾸는 능력이다.

- 239cd6b1: rsi_evolve.rs에 selfimprove.action 학습기 구현. 이전 세대의 실패 맥락·선택 행동·후속 실제 실행 결과를 연결해 행동별 가치와 탐색 보너스를 재계산. 단순 정해진 실패 유형 분기 대신, 반복 실패에 따른 새로운 복구 전략 탐색으로 이어진다.
- 34d78a03: coordinator와 실제 재빌드된 후손 worker 모두 새 전략 함수를 호출. 같은 세대 결과와 동일한 입력으로 정책을 재생성한다.
- 35226a61: 반복 편집 실패의 정책 전환, 쓰기만으로 채택으로 오인하지 않기, 서로 다른 캠페인 간 전이를 결합하지 않는 회귀 테스트 추가.
- c6e1c379: 이전 CI에서 반복 실패한 해시 복구 테스트의 비색인 루트 파일을 실제 src 경로로 고쳐 테스트가 실제 색인 수정을 재현하도록 함.
- 3c5a7d1e: 전략의 선택 결과에 따라 탐색 계수도 재조정하는 메타 적응 구현. 실측 후속 결과가 없는 경우 기존 탐색 계수를 사용.
- d81f3778: generation.actionDirective.policy의 revision, 정책 지표, 후속 행동 실측 outcome을 영구 rsi_generation/nextDecision에 계승.
- 8cccd79b: 메타 적응의 계수 변화 테스트.

학습은 동일 repository/owner partition과 목표 ID를 가진 최대 80개 세대 내에서 진행하며, 같은 캠페인의 연속 전이에서만 행동 결과를 귀속한다. 채택/유효 후보/실제 Git diff/새 발견을 차별화하고, 반복 실패와 토큰 비용을 차감한다. 정책 점수는 독립 품질 증거가 아니며 실제로 채택되지 않은 경우 measuredGain 및 qualityGain은 null로 유지한다.

미완: 실모델에서 연속 채택 후 새 바이너리가 다시 개선을 만드는 2세대 이상의 재귀 성능 상승은 아직 입증되지 않았다. 최신 CI 및 실제 모델 증거가 나오면 결과를 직접 확인해야 한다. Champion, 다른 Sub, main은 수정하지 않았다.

### 2026-11-11 추가 — 실행형 전략 계산식 합성
- 7998eb77 / b3ea84c7: 관측된 실패율·실제 채택 경험에 따라 회복 전략의 점수 계산식 구조를 생성한다. 실험마다 새 표현식(scoreExpression)과 코드 revision을 계산하고 기존 Rust 수식 평가기(eval_expression)에서 실제로 실행한다. 반복 실패가 있을 때 failureRate 벌점을 추가하고, 채택 경험이 있는 경우 verifiedAdoption 보상 항을 추가한다. 탐색 계수는 이전 정책 실험 결과로 계속 조정한다.
- 정책의 변경 여부, 계산식, 점수, 사용한 경험 ID, 실제 다음 행동 및 후속 결과를 세대 기록에 남긴다. 미검증 전략의 향상은 품질 상승으로 계산하지 않는다.
- CI 42971418567 (c6e1c379)은 정상 통과 관측. 이후 계산식 합성과 새 메타 테스트 커밋은 CI 대기/실행 중이며 실모델 연속 개선 실적은 아직 없다.


## 2026-11-11 Sub2 — 심판 59점 이후 구조적 자기수정

- 담당 브랜치: dominion-sub2. 시작 e605c7834, 코드 HEAD 580777f09. 다른 개발 브랜치·main은 변경하지 않음.
- 심판이 공개한 이전 점수: 59/100, 4위. 이 회차의 점수는 새 심판 판정 전까지 갱신하지 않음.

### 구현한 변화

1. 6bfb5d3, f911360: 도구의 실제 실패 영수증에서 원문 기반 stale SHA 복구와 기존 소스 쓰기 실패의 버전 기반 대체 실행 DAG를 합성. 저장된 후보를 후속 세대 도구 실행에 연결하는 테스트 추가.
2. 9014f9e, d6411e0: stale line-edit 실패에서 서로 다른 알고리즘인 읽기-재바인딩과 세대별 소스 인덱스 재바인딩을 합성. 같은 파일의 경쟁 후보는 세대별 1개만 배정하며 이전 실제 채택 및 적용 이력에 따라 다음 실행 선택을 바꿈.
3. 4eeddd1, d123951: 모의 모델 제공자와 DOMINION의 실제 native tool runtime을 통과하는 read→replace 통합 테스트를 작성. 도구 적용이 됐더라도 독립 채택 전에는 qualityGain을 올리지 않음.
4. 80459cf, a4fc128: 소스 파일이 옮겨져 읽기에 실패하면 현재 세대의 동일 저장소 인덱스에서 파일명이 유일하게 일치하는 새 경로를 찾아 DAG 노드·후속 SHA 바인딩·검증조건을 다시 구성. 후보가 여럿이면 추측하지 않음.
5. 11ab567: 기존 파일의 전체 교체를 자동 합성하는 경우 원래 절차에 file_equals의 정확한 목표 전체 내용이 있었을 때만 허용하여, 부분 내용 포함 조건으로 나머지 파일을 지우지 않도록 함.
6. 580777f: 다음 세대 결정에 복구 연산자와 부모 revision, 실제 적용 횟수, 검증 채택 횟수, 미측정 인과 상태를 기록.

### 현재 증거와 남은 완주

- GitHub에 실제 Rust 코드 및 회귀·통합 테스트 소스가 반영된 것까지 확인.
- 현재 런타임에는 cargo/rustc가 없고 GitHub Actions의 CI/실모델 캠페인이 queued 상태여서 새 커밋의 컴파일·테스트 통과·실모델 개선은 아직 확인 전. 이전 라이브 캠페인은 새 커밋 갱신으로 cancelled.
- 새 절차의 생성과 실제 효과는 구분한다. 누적 자기개선 연속 채택 3세대 및 메타 일반화 성능 증명은 미완성이고 capabilityDemonstrated를 참이라고 보고하지 않음.

다음 작업: HEAD CI 에러 교정 → 최신 라이브 영수증 도구 실행 확인 → 소스 diff 및 독립 paired held-out 채택 → 후손 재빌드와 2·3세대 연속 새로운 개선 채택 → 복구 연산자 자체의 동조건 비교 및 승격·폐기.


## 2026-11-11 Sub2 — 심판 65점 이후 성공 기반 자기진화·전이·정확한 성과 귀속

- 담당: dominion-sub2. 작업 시작 HEAD 87765bb5a, 직전 구현 HEAD ac21af43e.
- 직전 심판 점수: 65/100, 4위. 이 점수는 최신 구현 재채점이 아님.
- 다른 개발 브랜치, Champion, main 변경 없음.

### 신규 엔진 능력

1. abb0c6fa: 실제로 채택된 개선 방법이 수행한 read → versioned workspace_replace DAG에서 읽기 노드가 SHA 바인딩용으로만 사용됐고, 기존 세대의 인덱스 CAS로 대체 가능한 경우 읽기 노드를 없애는 새로운 실행 프로그램을 합성한다. 원래 프로그램과 성과는 보존하고 새 프로그램을 candidate로 남긴다.
2. 91d88311: 후손 후보를 다음 세대에서 단 1회 우선 시험하고, 성과 없이 실행만 했을 때 부모 방법으로 복귀하도록 선택기 자체를 변경한다.
3. c04e670c, 2953bd56: memory와 tools에서 같은 구조적 변환을 시험하는 단위 테스트, 성공 부모/후손 같은 파일 동시 편집 차단, 재시험 후 선택 변경 테스트.
4. 40916bb9, b0c72d8c: 실제 실행 영수증 → 프로그램 합성 → 영구 기억 → 다음 세대 선택 경로, 인덱스 SHA만으로 읽기 없이 실제 workspace_replace를 수행하는 통합 테스트 코드 추가.
5. 6a0d0f11, 65259ea6: 동일한 저장소 partition의 다른 도메인에서 성공적으로 채택한 read-ablation 연산자를 새 도메인의 관측된 실제 도구 실행에 적용하여 새로운 후보 DAG 합성. 출발 도메인의 채택 증거가 없으면 전이하지 않는다.
6. d91d3dbe, 9f22412b: 채택된 전이 후보를 다시 다른 도메인에 전파할 수 있게 depth 3으로 제한된 메타 연산자 계보를 추가한다. 각 도메인에서의 후속 성과는 개별적으로 입증되어야 한다.
7. 7edd047c, 3bb20124: 후손 방법이 실제로 채택되면 오래된 부모의 누적 사용 횟수가 많다는 이유로 승격이 막히지 않게 한다. 부모가 변경되는 동시에 실행되지 않도록 별도 후보를 유지한다.
8. d0c6d944, 6c076a9a, 159a2666, 54096f28, ac21af43: 채택된 소스 개선의 정확한 candidate.paths, verified/eligible 평가, 성공한 method 작업의 written 영수증, 실제 postSha256 및 최종 acceptedSourceHashes 일치가 모두 있을 때만 successfulApplications를 올린다. 같은 세대의 타 도구·다른 후속 수정이 채택됐더라도 무관한 방법에는 성공 보상을 주지 않는다.

### 확인 범위 / 다음 완주 단계

- GitHub Rust 코드와 단위·통합 테스트 소스 커밋 확인. 이 환경에 cargo/rustc가 없고 GitHub Actions 최신 CI/실모델 작업이 queued이므로 신규 코드의 빌드 통과 및 모델 기반 성능 향상은 아직 입증 전.
- 메타 방법 합성 자체에는 qualityGain을 설정하지 않으며, 채택된 소스 효과와 방법 자체의 인과적 성능 우월성도 별도로 구분한다.
- 다음에는 CI 오류 발생 시 즉시 수정, 실제 모델의 후보 패치 생성·독립 frozen paired holdout·채택, 새로운 바이너리 후손에서 다시 방법 진화 및 독립 평가를 3세대 이상 완주해야 한다.


## 2026-11-11 Sub2 — 심판 62점 이후 실모델 무수정 정체를 해결하기 위한 자기개선 실행 계층

출발 HEAD e94af6460, 담당 dominion-sub2. 심판 현재 점수 62/100·2위는 새 독립 채점 전까지 그대로 유지한다.

### 적용한 실제 엔진 변경

1. src/engine/rsi_worktree.rs: zero-diff 후 원본 소스가 깨끗하고, 세대가 단일 파일의 실제 소스 인덱스나 성공한 workspace_read_range 증거를 보유할 때만 같은 격리 작업 공간에서 구체적인 코드 편집을 1회 다시 시도하는 no_edit_feedback 추가.
2. src/engine/jobs.rs: 기존 컴파일 오류에 대한 재작업 경로를 실제 소스 수정 0건 재시도 및 도구 오류 복구와 결합. 한 작업에서 총 2번을 넘지 않고, source_edit_bootstrap / observed_tool_repair / verification_repair 종류별로 실행 영수증을 저장.
3. src/engine/rsi_worktree.rs: 모델이 workspace_replace staleVersion / preflightBlocked 또는 기존 파일에 workspace_write를 사용해 실패한 원인을 읽고, 기존 내용 재관측 후 실제 수정을 유도하는 failed_edit_feedback 추가. 패치 자동 합성이나 성공 선언이 아니라 후속 모델 도구 실행을 통한 수정 시도.
4. src/engine/rsi_evolve.rs: 기존 procedure가 전혀 없는 첫 세대에서도 원시 모델 workspace_replace의 staleVersion 또는 preflightBlocked 도구 기록을 기반으로 read_range → 동적 SHA 바인딩 → workspace_replace의 새로운 실행 DAG를 생성. candidate로 저장하고 remember_research를 통해 다음 세대에 계승. 0건 수정 결과를 허위 성공으로 계산하지 않음.
5. src/engine/rsi_evolve.rs: 각 세대 selfRepair 항목에 실제 재작업 횟수·사용한 방식·소스 diff 생성·후보 등록 여부를 저장. 재시도 후에도 수정 0건이면 다음 세대 행동 선택기의 컨텍스트를 exhausted_recovery로 바꾸고 탐색만 반복하지 않도록 change_approach를 우선 실행.
6. src/engine/jobs.rs: 사용자 명시 파일과 세대별 소스 색인 hash를 한 번 더 연결하여, 직접 파일 읽기 도구를 호출하지 않고도 이미 인덱스 근거를 가진 첫 세대의 실패를 복구할 수 있게 함.
7. .github/workflows/sub2-live-rsi.yml: jobs.rs, rsi_worktree.rs 변경도 로컬 Qwen 실모델 워크플로가 감시하도록 수정. scripts/sub2-live-rsi.mjs에 selfRepair·실제 methodExecutions 결과를 추가하여 보고서가 개선 도구의 효과와 단순 CI 통과를 구별하게 함.
8. 관련 신규 회귀 테스트: 원문 근거가 없는 무편집 상태, 다른 세션의 도구 관측, 모델 API 오류, 실패한 편집과 이미 적용된 수정의 구별, raw model 실패→실제 후손 DAG 도구 의도 생성→경험 저장→다음 세대 선택, 실패한 재시도 후 정책 전환.

### 실행 증거와 완주 미완

- 모든 코드는 GitHub dominion-sub2에 실제 커밋했으며 다른 개발 브랜치는 수정하지 않음.
- 이 실행 환경에 cargo/rustc가 없으므로 이번 변경의 컴파일·Rust 테스트 완료를 주장하지 않음. 최신 CI/실모델 워크플로는 GitHub Actions에 제출된 상태이고 완료 여부를 별도로 확인해야 한다.
- 재시도와 복구 DAG를 새로 구현한 것은 실모델 개선 성공 증거가 아님. 실제 소스 diff > 0, 후보 등록, frozen paired holdout 품질 개선, 새 후손 바이너리 재빌드 후 다시 개선 채택까지 확인되어야 완주한다.

### 다음 최우선

HEAD CI/실모델 로그의 첫 컴파일·실행 결함 즉시 수정 → Qwen 4B 실모델의 workspace_replace 실제 수행 확인 → 후보 frozen paired 검증 및 채택 → 재빌드된 후손의 새로운 알고리즘 변형·채택 연쇄 3세대 달성.


## 2026-11-11 · 심판 65점 이후: 관측 DAG 구조 연산자의 귀납·전이 및 결과 적응 선택

시작: ecf74f6bee0b. Sub2만 변경. 해당 평가 시점의 점수 65점·2위는 독립 재채점 전까지 변경하지 않는다.

### 신규 자기개선 알고리즘

1. **successful_observation_pruning**: 동일 세대의 실제 채택된 파일 변경을 직접 수행한 procedure가 있어야만 이전 DAG의 실행된 code_search/web_search/http_fetch 노드를 분석한다. 해당 노드의 결과가 후속 인자, before/after, 다른 바인딩에 소비되지 않고 독립 검증을 요구하지 않는 경우, 노드를 삭제하고 선행·후속 간선들을 합성해 새로운 DAG 프로그램을 만든다. 소스 읽기는 상태형 실행 전제일 수 있으므로 일반 가지치기 대상에서 제외한다. 새 방법은 candidate로 저장되며 성공을 승계하지 않는다.
2. **transferred_observation_pruning**: 동일 저장소 파티션에 속하고 다른 능력 영역에서 독립적으로 채택된 성공적인 가지치기 후손이 존재할 때만 그 변형 원리를 새 영역에 적용한다. 도착 영역도 실제 도구 실행 영수증을 가져야 하고, 새로운 후보의 qualityGain은 null이다. 채택 후 추가 전이는 3단계 깊이 제한.
3. **observed_method_outcomes_posterior_v1**: 방법의 단순 누적 성공 수를 기준으로 선택하지 않는다. 실제 응용 기록의 채택된 효과(파일 결과 해시·채택된 패치), 실행 성공과 실패, 미시험 불확실성을 바탕으로 선택 점수를 계산한다. 전체 성공률에서 탐색 계수를 갱신하여 효율적인 후손 후보를 시도하고, 후손 채택 후에는 기존의 우선순위·동일 파일 동시 편집 방지 규칙을 유지한다.
4. **학습과 성과를 구분하는 영수증**: 방법 후보의 원본 method ID, revision, 수정 전·후 노드 의존 관계, 실제 실행 ID, 전이 도메인 및 깊이를 기록하고 미실험 후보에는 성능 개선 보상을 부여하지 않는다. 방법 선택 계수는 관측 기반 스케줄링이지 인과 A/B 품질 개선의 입증이 아니다.

### 추가한 회귀 테스트

- memory의 code_search, tools의 http_fetch를 성공 실행 DAG에서 제거하되, 선행/후속 의존 관계를 올바르게 재연결.
- 조회 결과가 바인딩으로 소비됐거나 postcondition에서 도구 호출을 요구하거나 실행 증거가 없으면 제거 거부.
- 부모 DAG 채택 없이 변형된 후보를 성공으로 인정하지 않음.
- 다른 도메인의 채택된 변형 연산자는 새 후보로 전이하되 새 도메인에서 아직 성공하지 않았다고 명시.
- 성공 6회/20번 시행 방법보다 성공 2회/2번 시행 방법을 우선하는 선택기, 실패가 많아지면 탐색 계수가 증가하는 메타 적응.
- 한 세대에서 같은 파일을 2개의 후보가 동시에 수정하지 않도록 선택기에서 중복 대상 차단.

### 완료 조건과 이번 검증 구분

Rust 소스·테스트 코드 GitHub 커밋은 확인했다. 당시 CI 및 새 실모델 캠페인은 대기 상태였으며, 이 작업 세션에서는 cargo/rustc로 빌드·테스트를 직접 실행하지 못했다. 앞선 실모델 캠페인의 소스 빌드 단계 성공은 확인했으나 최신 합성 연산자와 전체 3세대 재귀적 향상 실증은 아직이다. 연속된 실제 소스 diff → 독립 frozen holdout 승격 → 재빌드된 후손의 새로운 메타 연산자 생성 → 후속 승격을 확인해야 완주다.
