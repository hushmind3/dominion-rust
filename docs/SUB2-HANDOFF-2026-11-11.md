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
