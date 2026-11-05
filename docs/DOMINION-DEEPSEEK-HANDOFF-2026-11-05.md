# DOMINION — DeepSeek 전달용 전체 기술 설계서

작성일: 2026-11-05

이 파일 하나에 현재 구현 진단, 자기개선 알고리즘, Rust 구현 명세, 단계별 개발 지시, 검증·벤치마크, 미해결 위험을 모두 담았다. 구현 코드를 변경한 보고가 아니라 후속 구현자를 위한 설계서다.

기준: hushmind3/dominion-rust / codex/native-memory-harness / ac1d0a0 및 분석 시점의 미커밋 procedure 변경. 일반 채팅 실행이 중심이며 /goal은 같은 코어를 사용하는 선택 기능이다.

읽는 순서: 제1부 설계 → 제2부 S0~S12 개발 지시 → 제3부 검증. 문서에 들어 있는 테스트 명령·실험은 앞으로 실행할 지시이며 이번 작성 과정에서 실행한 결과가 아니다.


---

# 제1부

# DOMINION 범용 자기개선 설계 — DeepSeek 구현 기준

작성: 2026-11-05. **설계 산출물이며 구현 완료 보고가 아니다.**

## 0. 읽는 순서와 판정

1. 이 문서: 실제 구현 진단, 알고리즘, Rust·저장 명세, 위험.
2. [단계별 개발 지시서](/Users/gim-yeonsu/dominion-rust/docs/SELF-IMPROVEMENT-TASKS-2026-11-05.md): 의존 순서대로 별도 커밋할 작업.
3. [검증·벤치마크 명세](/Users/gim-yeonsu/dominion-rust/docs/SELF-IMPROVEMENT-BENCHMARK-2026-11-05.md): 모의 모델과 실제 모델, 인과 비교, 성장 판정.

선택한 구조는 **기존 실행 기록을 토대로 한 검증 가능한 절차 + 범위를 나눈 통계적 전략 선택 + 격리된 대조 실험**이다. 새 에이전트 런타임이나 별도 기억 DB를 만들지 않는다. 기존 Rust 실행 루프, SQLite, `learn`/`reflect` 작업을 확장한다. 일반 채팅이 첫 적용 대상이고 `/goal`은 같은 실행·검증 코어를 호출하는 선택 기능이다.

모델 가중치는 바꾸지 않는다. 개선 대상은 절차, 검색 결과, 복구 선택, 검증 프로필, 제한된 실행 정책이다. 코드 변경은 제안으로만 남긴다. **동일 모델의 실제 성공률 상승은 앞으로 실험해야 할 목표이며, 이 문서로 달성했다고 주장하지 않는다.**

## 1. 분석 기준과 현재 구현 진단

### 1.1 확인한 저장소 상태

- 저장소: `hushmind3/dominion-rust`, 브랜치 `codex/native-memory-harness`.
- `git fetch origin codex/native-memory-harness` 후 로컬과 원격 모두 `ac1d0a0f95510a38bfc334c661f338b36e46a5bb`.
- 최근 이력: `ac1d0a0` 난제 보고서 → `b6985ba` 성공 경험 공유 → `c30ad3f` 학습·채널 전파 → `46ed801` 기억·전략 채택 수정.
- 분석 당시 미커밋 변경: `chat/runtime.rs`, `improve.rs`, `learning.rs`, `reflect.rs`, `engine/mod.rs`, `tests/e2e.rs`; 미추적 `engine/procedure.rs`. 이 변경은 HEAD와 별개다. 삭제하거나 덮어쓰지 않는다.
- [기준 보고서](/Users/gim-yeonsu/dominion-rust/docs/HARD-PROBLEMS-2026-11-05.md)의 148 unit + 21 integration, 실제 4B baseline 0 / post 0/3 / cross-agent 0은 **기존 보고 값**이다. 이번 분석에서 테스트나 모델 실험을 재실행하지 않았다.
- 아래 줄 번호는 분석 당시 작업 트리 기준이다. DeepSeek는 작업 시작 시 HEAD·diff를 다시 기록하고 함수 이름으로 위치를 확인한다.

### 1.2 실제 호출 경로

```text
일반 채팅 / room turn
  run_turn_with_policy → 실행 record 생성 / 취소 세대 고정
  run_turn_body → 원문 저장·replyTo → prepare_owner_action
    → system_prompt → memory + skill + improvement + experience 주입
    → 선택적 action_plan → stream_completion
    → 도구 인자 파싱 → [WIP procedure 검사] → 승인 → tools/MCP
    → execution_operation + tool message 저장
    → acceptance / [WIP procedure repair] → 최종 메시지
  finalize_execution → [WIP compliance] → improve::record_execution
  autoLearning이면 learn / 교정 감지 시 reflect enqueue

learn → 페이지·checkpoint → LLM 추출 → save_grounded / 스킬 draft / propose
reflect → 교정·최근 실행 분석 → global experience / strategy / memory / skill draft
goal job → plan → run_quiet_outcome(일반 실행) → acceptance + reviewer → repair/replan
```

`execution`, `execution_operation`, `improvement`, `experience`, `memory`, `skill`, `job`은 대부분 SQLite `records(kind,id,data)`에 저장된다. `messages`는 별도 테이블이다. 이 저장 모델을 유지한다.

### 1.3 재사용·수정 지점

| 위치 | 확인한 구현 | 판단 / 수정 |
|---|---|---|
| [improve::propose](/Users/gim-yeonsu/dominion-rust/src/engine/improve.rs:10) | 근거 ID 목록·rationale 검사, 유사 문구 비교, 기본 `adopted`, harness만 `proposed` | 출처 ID의 의미·권한·내용을 재검증해야 한다. `candidate`부터 시작하고 내용 유사도만으로 기존 절차를 덮어쓰지 않는다. |
| [active_values](/Users/gim-yeonsu/dominion-rust/src/engine/improve.rs:288) | 범위에 맞는 adopted/validated 최대 6개. 작업 적합도·비용·성과 조건 없음 | 작업별 selector로 교체. 한 번 선택한 불변 snapshot을 주입과 실행 양쪽에 사용. |
| [record_execution](/Users/gim-yeonsu/dominion-rust/src/engine/improve.rs:348) | HEAD는 주입 ID + toolCalls + completed로 성공 집계. WIP는 준수 결과도 사용 | WIP는 진전이나 인과 추정이 아니다. 5회·기준 성공률−0.2 방식 폐기. observed와 controlled evidence 분리. |
| [attach_verification](/Users/gim-yeonsu/dominion-rust/src/engine/improve.rs:462) | 같은 record에 검증 정의 덮어쓰기, version 증가 | 실행 중 snapshot은 유지되지만 성과는 ID 단위로 섞인다. 내용 주소 기반 불변 revision과 revision별 통계 필요. |
| [experience::record_with_scope](/Users/gim-yeonsu/dominion-rust/src/engine/experience.rs:97) | signature·outcome·lesson 기반 병합, 근거 최대 12개 | 원시 episode와 재사용 교훈 분리. 근거 12개 잘림으로 원시 이력 유실 금지. |
| [experience::retrieve](/Users/gim-yeonsu/dominion-rust/src/engine/experience.rs:186) | 문자열 포함, 발생 횟수 boost; 검색 자체가 uses 증가 | exposure로 이름 변경. 의미 검색 추가하되 실제 준수나 효용으로 집계하지 않는다. |
| [consolidate](/Users/gim-yeonsu/dominion-rust/src/engine/experience.rs:252) | signature/outcome 중복 정리, 전체 300개 초과 archive | 상반된 교훈도 같은 signature면 묻힐 수 있다. 의미·조건·출처를 보존하고 범위별 working set 관리. |
| [learning::run](/Users/gim-yeonsu/dominion-rust/src/engine/learning.rs:192) | actor별 lock, immutable through ID, grounding·checkpoint | 재사용. remainder가 `throughMessageId=through`로 바뀌어 원래 snapshot 끝을 잃는 경로를 수정. 원본 through 고정, cursor만 이동. |
| [apply_learning](/Users/gim-yeonsu/dominion-rust/src/engine/learning.rs:370) | 기억은 엄격한 원문 검사, 전략은 batch ID 포함 여부 중심 | 전략에서도 메시지 역할·권한·실행 증거 검사. assistant ID를 사실 증거로 채택하지 않는다. |
| [reflect::run](/Users/gim-yeonsu/dominion-rust/src/engine/reflect.rs:107) | 최근 대화·실행을 분석하고 global 산출물 생성. memory는 직접 put | 지연 실행 시 다른 턴이 섞일 수 있다. episode/교정 watermark 고정. `save_grounded` 우회 금지. 원문 대신 승인된 추상 교훈만 범위 확대. |
| [memory::visible](/Users/gim-yeonsu/dominion-rust/src/engine/memory.rs:56) / [save_grounded](/Users/gim-yeonsu/dominion-rust/src/engine/memory.rs:321) | partition·agent·sourceAuthorKey·정확 인용 검사. global은 범위 통과 | 대부분 재사용. 명시적인 공유 허가와 원문/추상본 visibility를 공통화. 정확 인용은 출처 증명이지 추측성 요약의 진실 증명이 아니다. |
| [chat::context](/Users/gim-yeonsu/dominion-rust/src/engine/chat/context.rs:62) | 기억·스킬·전략·경험을 각각 조회 | 실행 시작 때 고정한 ContextSelection을 전달. 중복 절차와 토큰 폭증 방지. |
| [runtime](/Users/gim-yeonsu/dominion-rust/src/engine/chat/runtime.rs:194) | 실제 승인·도구·기록·취소·예산, ordinary acceptance 이미 존재 | 유지. 일반 실행을 완료 주장만 하는 루프로 되돌리지 않는다. 공통 RunGuard를 도구 경계와 완료 경계에 연결. |
| [runtime::action_plan](/Users/gim-yeonsu/dominion-rust/src/engine/chat/runtime.rs:984) | 직접 실행 요청은 별도 LLM 계획; 실패 시 reviewer 호출 | 단순 결정적 실행은 추가 호출 0으로 시작. 필요한 경우만 계획/리뷰 프로필 선택. 비용 계측에 보조 호출도 포함. |
| [goals::run](/Users/gim-yeonsu/dominion-rust/src/engine/goals.rs:9) | plan·실행·검토·repair·replan·중단 checkpoint | 보존. 합성 goal 프롬프트를 새 사용자 지시로 다시 추출하지 않도록 parent task 연결. 중복 평가·중복 보상 제거. |
| [goals::acceptance_gaps](/Users/gim-yeonsu/dominion-rust/src/engine/goals.rs:704) | 파일·실행 근거 검사, 원래 baseline | 공통 verification 모듈로 이동하고 호환 wrapper 유지. 절차 준수와 결과 검증을 통합 판정하되 각각 기록. |
| [jobs](/Users/gim-yeonsu/dominion-rust/src/engine/jobs.rs:125) | 재시작의 uncertain side effect 보류, CAS claim, learn 우선순위 낮음 | 유지. reflect/embed/consolidate도 background class. learn_running 관찰 후 claim은 여러 worker 간 동시 admission을 완전히 보장하지 않으므로 원자적 slot 필요. |
| [Store](/Users/gim-yeonsu/dominion-rust/src/db.rs:111) | put upsert, 단일 record CAS·increment | episode 종료·observation·정책 집계는 여러 record에 걸치므로 하나의 transaction 필요. 읽기 실패를 빈 목록으로 처리하는 기존 API를 새 평가 경로에서 쓰지 않는다. |
| [task-benchmark.sh](/Users/gim-yeonsu/dominion-rust/scripts/task-benchmark.sh:11) | 기본 4317, repo/bench 삭제, autoApproveAll, 파일 내용만 검사 | 현 상태로 실행 금지. 격리 sentinel·읽기 확인·순서 검증·원시 결과 저장이 있는 Rust runner로 대체. shell은 호출 wrapper만. |

### 1.4 미커밋 procedure의 구체적인 결함

기존 시도를 버리지 않고 다음 검증기를 엄격하게 만든다.

- [normalize](/Users/gim-yeonsu/dominion-rust/src/engine/procedure.rs:60)는 잘못된 조건을 필터링해서 나머지만 채택한다. 필수 조건이 조용히 사라지므로 **전체 revision을 Invalid로 거부**해야 한다.
- [operation_matches](/Users/gim-yeonsu/dominion-rust/src/engine/procedure.rs:101)는 `path/file/command/cwd`의 부분 문자열을 검색한다. `report.txt.bak`, `echo report.txt`도 혼동 가능. typed tool adapter가 정규화한 resource ID와 인자 값을 비교한다.
- [evaluate](/Users/gim-yeonsu/dominion-rust/src/engine/procedure.rs:265)는 before의 양쪽 작업이 없으면 true. 이는 조건부 제약의 공허한 충족일 수 있지만 ‘전략 실행’ 증거가 아니다. `NotTriggered`를 구분한다.
- before는 첫 위치만 비교한다. 대상 호출마다 prerequisite 범위·무효화 여부를 검사해야 한다. 정렬은 timestamp/문자열 ID 대신 엔진 단조 sequence를 사용한다.
- [read_workspace_file](/Users/gim-yeonsu/dominion-rust/src/engine/procedure.rs:224)는 lexical 상대경로 검사만 하고 read_to_string한다. symlink 탈출·큰 파일·binary 존재 검사·읽기 오류와 부재를 구분할 필요가 있다.
- [runtime 절차 repair](/Users/gim-yeonsu/dominion-rust/src/engine/chat/runtime.rs:411)는 `!force_final && repairs < 2`에서만 실행된다. 소진 후에는 위반을 terminal failure로 고정하지 않는다. [acceptance_passed](/Users/gim-yeonsu/dominion-rust/src/engine/chat/runtime.rs:861)는 일반 조건만 통과해 force_final을 만들 수 있다. **최종 공통 판정을 우회하는 완료 경로가 있어서는 안 된다.**
- 최종 적용성 계산은 tool=None이라 tools-only 적용 조건을 놓칠 수 있다. 요청 시점의 eligibility와 실행 중 trigger를 별도 기록한다.
- [finalize_execution](/Users/gim-yeonsu/dominion-rust/src/engine/chat/runtime.rs:1136)은 실행 저장 전에 카운터 갱신을 호출한다. 반복 finalization의 중복 집계·중간 crash·동시 덮어쓰기 방지 필요.

## 2. 데이터와 권한의 기본 계약

### 2.1 혼동하면 안 되는 여섯 가지

| 개념 | 정의 | 성공 증거 여부 |
|---|---|---|
| Exposure | 선택된 내용을 실제 provider 요청에 포함 | 아니오 |
| Applicability | 실행 전에 해당 작업·환경에 적용 가능 | 아니오 |
| Trigger | 해당 절차의 조건이 실제 실행에서 발생 | 아니오 |
| Compliance | 요구된 실행·순서·조건을 증거로 충족 | 절차 준수만 증명 |
| TaskOutcome | 사용자 작업 조건을 충족 | 해당 작업 결과 증명 |
| Effect | 같은 조건의 control보다 성과가 개선 | 통제 실험의 대상 분포 내 개선 근거 |

`appliedImprovementIds`는 호환 필드로 남기되 의미를 exposure로 명시한다. 새 집계는 이 값만 보고 갱신하지 않는다. completed는 전송/실행 lifecycle일 수 있으므로 `taskOutcome`을 별도로 제공한다.

### 2.2 작업·실행·증거 단위

- `TaskEpisode`: 실제 권한 있는 원문 요청 한 개 또는 명시적인 계속 작업의 root. `taskId`, `sourceMessageId`, `instructionRevision`, `owner/actor`, scope, 작업 의도·성공 조건·budget을 갖는다.
- `ExecutionAttempt`: 기존 `execution`. 한 episode에 여러 turn, agent, repair 시도가 연결된다. `/goal`의 합성 프롬프트는 `parentTaskId` 아래 attempt일 뿐 새 owner instruction이 아니다.
- `Operation`: 기존 `execution_operation`. proposal/blocked/approved/running/completed/failed/interrupted를 구분하고 `opSeq`, `attemptId`, `taskId`, `origin=model|procedure`, `contractRevision`, `strategyRevision`을 추가.
- `EvidenceRef`: 종류가 명확한 message/operation/artifact/verification/owner_correction 참조. ID만이 아니라 source scope, revision, observedAt, digest, 관찰자를 포함.
- `Experience`: 위 episode들의 추상 교훈·실패 분류·적용 조건·근거 링크. 원본 사실을 대체하지 않는다.

동일 원문에 9명이 대답해도 성공 표본 9개를 만들지 않는다. 작업 단위 결과는 한 번, 정책 결정별 관측은 별도로 한 번. 다른 에이전트의 성공을 읽은 행위와 직접 실행한 행위도 구분한다.

### 2.3 작업 계약과 학습 전략을 분리

`TaskContract`에는 사용자가 요구한 결과, 필요한 권한, 명시한 필수 절차만 들어간다. 전략 후보는 이 계약의 정답·성공 조건·승인 정책을 바꿀 수 없다.

전략은 두 종류다.

1. **StandingConstraint**: 소유자의 유효한 교정에서 확정한 필수 작업 규칙. 실제 사용자 의도·적용 범위를 확인해 버전 고정. 실서비스에서 무작위로 꺼서 실험하지 않는다.
2. **AdaptiveStrategy**: 성공 가능성을 높인다는 가설. 대체 가능한 구현·복구 방법이다. 미준수면 전략 성공으로 집계하지 않지만, 선택적 전략 실패만으로 사용자 작업 결과를 거짓 실패로 바꾸지 않는다.

필수 절차 위반 + 파일 결과만 올바름 = 작업 계약 실패. 선택적 전략 미준수 + 결과 올바름 = 작업 성공 / 전략 준수 실패. 두 사실을 동시에 보존한다.

사용자 지시, 인용문, peer 발화, 도구 결과는 타입으로 구분한다. 기존 문자열 휴리스틱은 초기 intent 후보만 만든다. 구조화된 외부 채널 origin·owner identity·replyTo가 authority의 근거다. 모호한 고위험 새 행동을 반성 모델의 추측으로 승인하지 않는다.

## 3. 알고리즘 전체 흐름

```mermaid
flowchart TD
  A[권한 있는 요청 / TaskContract] --> B[범위 필터 + 경험 검색]
  B --> C[적합한 revision·정책 선택 및 고정]
  C --> D[기존 Rust 실행 루프 + RunGuard]
  D --> E[도구 증거 / 준수 / 결과 별도 판정]
  E --> F[원자적 episode 종료 + observation]
  F --> G[정책 통계 갱신]
  F --> H[실패·교정·새 경험의 background 분석]
  H --> I[개선 가설 / 새 불변 revision]
  I --> J[구조 검증 + 격리 실험]
  J --> K[canary / supported / inactive 결정]
  K --> C
  G --> C
```

### 3.1 작업 시작

1. owner 원문·수정 이력·범위를 고정한다. 외부 요청이 현재 owner의 권한인지 이미 존재하는 identity/policy 경로로 확인한다.
2. 결정적으로 추출 가능한 조건은 Rust가 계약으로 만든다. 불명확한 작업은 목표 설명과 unknown 검증을 유지한다. 정답을 임의로 보충하지 않는다.
3. 도구 capability·workspace resource baseline·모델 fingerprint·tool schema hash·기억 snapshot을 고정한다.
4. permission/scope/applicability로 후보를 먼저 줄이고 lexical+semantic retrieval을 수행한다.
5. 필수 제약 + 선택적 전략 1개(초기 기본값) + 필요하면 비충돌 보조 스킬을 선택한다. 실험에서는 후보 하나만 바꾸고 나머지를 고정한다.
6. SelectionSnapshot과 실제 주입된 revision ID를 기록한다. context 재생성 때 DB에서 새 후보를 고르지 않는다.

### 3.2 실행 전·중·후 검사

| 시점 | Rust가 하는 일 | LLM 필요 |
|---|---|---|
| 전 | schema, 권한, 모델/도구 호환, 변수 binding, DAG cycle, resource baseline | 모호한 binding/계획만 선택적 |
| 도중 | 매 proposed call의 args/resource/순서/precondition, 취소, 예산, 승인 | 위반 시 작은 구조화 수정 요청 |
| 후 | 결과 oracle + 필수 절차 + evidence readback | 의미 평가가 필요한 항목만 reviewer |
| 실패 | 실패 코드·남은 budget에 맞춘 repair policy 선택 | 필요한 복구만 |

결정적 실패를 reviewer가 뒤집지 못한다. reviewer가 불가능하거나 JSON이 틀리면 semantic verdict는 Unknown이다. ‘도구 성공’만으로 작업 결과가 맞았다고 보지 않는다.

### 3.3 작은 모델을 위한 절차 실행

자연어를 반복 주입하는 것만으로 해결하지 않는다. 전략 revision은 선택적으로 **작고 제한된 partial-order procedure**를 갖는다. 전체 작업을 고정 workflow로 강제하지 않는다.

- 각 node는 typed tool intent 또는 deterministic assertion. dependency는 DAG이며 최대 16 node, 32 edge, 깊이 8.
- `model_guided`: 모델이 제안한 도구를 검증하고 누락 조건을 알려준다. 초안·새 전략의 기본 모드.
- `harness_assisted`: 구조와 근거가 검증된 revision의 다음 준비된 node를 Rust가 실제 tool intent로 제안한다. 사용자 허용 범위와 기존 approval을 그대로 거친다.
- `enforcement`는 필수 제약에 대한 `block`; 선택적 전략은 `observe` 또는 부작용 없이 가능한 제한된 `repair`. 임의 학습 후보가 사용자 명령을 가로막는 전역 규칙이 되지 않는다.
- 모든 값은 literal / owner-grounded task binding / 성공한 앞 node의 검증된 JSON pointer 결과 중 하나. 단순한 텍스트 치환이나 shell 문자열 보간 금지.
- 파일과 실행 명령의 인자를 타입 검사한다. 기존 `terminal_exec(command,args)`를 재사용한다. `shell_exec` 자체는 유지하되 learned macro가 도구 결과의 문자열을 쉘 명령으로 합성하지 못한다.
- 변수 값이 없거나 검증 불가능하면 node는 AwaitBinding. 모델에 정확히 그 값만 요청하거나 model_guided로 내려가며 값·권한을 만들어내지 않는다.
- 하네스가 실행해 성공한 경우 `origin=procedure`, `assistanceMode=harness_assisted`를 남긴다. 모델이 자발적으로 준수한 것으로 보고하지 않는다.

예: ‘작업 전 유효한 입력 형식을 확인하고, 변환 후 출력 스키마 검사’라는 구조는 파일 변환·API 응답 처리·데이터 분석에 공통 적용 가능하다. 구체적 경로·스키마는 이번 TaskContract와 입력 artifact에서 binding한다. 벤치마크 정답은 엔진에 넣지 않는다.

### 3.4 완료와 복구

단 하나의 `finish_attempt` 경로에서 최종 판정한다. early acceptance, force_final, 최대 반복 도달, 도구 없음, 스트림 종료, 재시작 모두 이 경로의 판정을 우회하지 않는다.

```text
cancelled                  → Cancelled
부작용 결과 불명확         → NeedsReview(Unknown)
필수 deterministic 실패    → Failed / budget 내 Repair
필수 semantic 불명확       → NeedsReview(Unknown)
필수 조건 전부 Pass        → Succeeded
작업 결과 검증 미정의      → AnsweredUnverified (성공률에 성공으로 넣지 않음)
```

복구 정책 초기 후보는 `fix_arguments`, `satisfy_prerequisite`, `readback_then_reconcile`, `alternate_tool`, `replan`, `stop`이다. 실패 코드와 도구 capability로 불가능한 후보를 제거한다. 재실행이 위험한 Unknown side effect는 readback/reconcile만 가능하며 retry가 기본값이 아니다.

현행 8 rounds / 40 tool calls / 600초는 상한을 유지한다. 하나의 BudgetLedger가 planner·실행·review·repair·하네스 node를 함께 센다. blocked proposal도 proposal budget을 소비하지만 실제 executed tool 수와 별도 기록한다. 기본 repair는 총 2회, 같은 violation digest는 최대 1회 재시도; 새 증거가 없으면 다른 방법 또는 종료. goal은 parent budget 아래 child limit을 부여하며 매 attempt마다 무제한 리셋하지 않는다.

### 3.5 완료 후 학습

모든 attempt는 성공/실패/취소/unknown을 기록한다. LLM 반성은 모든 턴에 돌리지 않는다. 사용자 교정, 새 failure signature, 동일 실패 3회, 새로운 성공 절차가 있을 때만 background hypothesis 생성. 반복 failure group당 활성 후보 최대 3개, 하루 호출·토큰 예산을 설정하고 초과 시 defer한다.

가설은 `observedFailure`, `suspectedCause`, `proposedChange`, `predictedObservableEffect`, `falsification`, `applicability`, `evidenceRefs`를 갖는다. 관찰과 추측을 구분하며 ‘실패 후 이 방법이 좋을 것 같다’는 유효성 증명이 아니다. 추측은 candidate, 명시적 owner 교정은 권한이 확인된 제약 후보가 된다.

## 4. 전략 선택·갱신 알고리즘

### 4.1 선택한 방법과 대안

초기 구현은 **작업 context bucket별 Beta 성공 추정 + 비용·위험 제약 + 확률이 명시된 ε-greedy 탐색**이다. 작업 특성에 따라 다른 정책 통계를 사용하는 contextual bandit의 단순한 구현이다. 정책 행동 선택이라는 원리는 [Li 등의 contextual bandit 연구](https://arxiv.org/abs/1003.0146)를 참고하되 구현과 효용 정의는 DOMINION에 맞춰 작성한다.

| 후보 | 이번 선택 |
|---|---|
| 고정 규칙 / 성공 횟수 | 권한·검증 invariant에는 사용. 학습 선택기로는 부족. |
| 전역 Beta/Thompson sampling | 작업 난이도가 섞이고 정확한 selection propensity 계산이 번거롭다. 기본 선택기로 채택하지 않음. |
| LinUCB·신경 contextual bandit | 희소 데이터에서 feature·행렬·비정상성 검증 비용이 큼. 후속 비교 후보. |
| bucket별 Beta + ε-greedy | SQLite로 충분, 선택 확률 정확, 실패 유형별 adaptation 설명 가능. 채택. |
| 전체 RL·소스 자동수정 | 현재 요구와 맞지 않음. 미구현. |

### 4.2 Context와 업데이트

`ContextKey = (taskFamily, failureClass_or_none, modelFingerprint, toolCapabilityVersion, riskClass, verifierProfile)`.

- taskFamily는 file_transform/code_change/web_query/research/analysis/multi_agent/other 같은 확장 가능한 label. label 자체는 성공 조건이 아니다.
- raw filename/user ID를 성공 예측 feature로 쓰지 않는다. 공유 권한으로 통계 집계를 먼저 나눈다.
- 처음에는 family/model/risk 기준 coarse bucket. 30개 미만이면 같은 안전 범위 parent 통계를 prior로 소량 차용한다. 다른 모델의 결과는 causal 근거로 이전하지 않는다.

선택용 통계만 시간 감쇠한다. 원시 observation과 실험 결과는 불변이다.

```text
ρ = exp(-ln(2) × elapsed_days / H), H 기본 30일
S ← ρS + y ; F ← ρF + (1-y)     # 최종 결과를 확인한 관측만
α0 = 1 + κ × μ_parent
β0 = 1 + κ × (1-μ_parent)         # parent 없으면 α0=β0=1
κ = min(4, parent_effective_n)    # 다른 bucket prior가 실측을 압도하지 않게 제한
α = α0 + S ; β = β0 + F
μ = α/(α+β)
σ² = αβ / ((α+β)²(α+β+1))
```

`y=1`은 필수 조건이 모두 검증된 성공, `y=0`은 확인된 실패. 취소·승인 거절·외부 서비스 불명확은 task learner의 S/F에 넣지 않고 별도 availability/unknown 통계에 넣는다. 실제 벤치마크의 strict 성공률은 unknown도 분모에 포함한다. 이렇게 해야 unavailable 실행을 빼서 실제 성능을 부풀리지 않는다.

준수 확률도 별도 Beta(C_followed,C_violated)를 둔다. NotApplicable/NotTriggered/Unknown은 준수 성공도 실패도 아니다. 그러나 실험에 배정되고 전략을 무시한 실행은 ITT 결과에서 제거하지 않는다.

비용은 `c = min(1, tokens/tokenCap + activeLatency/latencyCap + toolCost/costCap)`의 사전 고정 가중 합을 사용하고 각 원시 값도 저장한다. 권한 승인 대기 시간은 사용자 체감 지연과 active latency에 따로 표시한다. 위험 사건은 관측 위험률과 고정 capability 위험 상한을 함께 적용한다.

```text
freshness = exp(-ln(2) × days_since_compatible_validation / 90)
Q(a,x) = μ - 1.0σ - 0.10 E[c] - 0.50 U(risk)
         + 0.03 relevance + 0.02 freshness
```

위 가중치는 시작 설정이며 성능 보장이 아니다. relevance·freshness는 [0,1], unknown risk의 U는 1, 시험하지 않은 revision은 production exploit 후보에서 제외. 보상 가중치를 online 모델이 바꾸지 못한다. 위험·권한 invariant는 점수보다 앞선 hard filter다.

### 4.3 탐색 확률과 빈 데이터

- 후보 집합 A에는 항상 `no_optional_strategy` 또는 현재 안정 정책을 포함한다. standing constraint는 어느 arm에서도 유지한다.
- low-risk canary와 격리 실험만 새 revision 탐색 가능. production 기본 ε=0; 소유자가 설정한 저위험 탐색에서 ε=0.05. 테스트 설정에는 ε=0.10 허용.
- `a* = argmax Q`, tie는 revision hash 정렬. 확률 `p(a|x)=(1-ε)·1[a=a*]+ε/|A|`.
- RNG seed, 후보 집합, 모든 Q, p, 정책 version, 실제 선택을 **실행 전에 저장**한다. 저장 실패면 실험 실행을 시작하지 않는다.
- 한 context에 데이터가 없으면 안정 baseline. 불확실성만으로 실서비스에 미검증 변경을 밀어 넣지 않는다.
- tool_policy의 효용은 ‘그 도구의 성공 횟수’가 아니라 해당 정책을 선택한 작업의 결과로 측정한다. 도구를 피하는 정책은 그 도구가 호출되지 않아도 평가 대상이다.

### 4.4 시스템 자체의 개선

같은 selector 인터페이스를 세 decision slot에 적용한다.

1. `strategy`: 이번 작업에서 어느 검증된 방법을 쓸지.
2. `recovery`: 실패 상태에서 어느 복구 방법을 쓸지. 상태·이미 실행한 복구·남은 예산을 고정하고, 주 지표는 terminal outcome, 보조 지표는 다음 checkpoint gap 감소.
3. `verification_profile`: 필수 oracle는 그대로 두고 additional readback / semantic review / stronger semantic review 중 어느 추가 검증을 할지.

검증 profile의 성공 보상은 자기 판정이 아니라 독립적인 benchmark oracle/owner adjudication과의 일치다. 독립 label이 없는 작업은 검증기 정확도 업데이트를 하지 않는다. 저비용 profile도 고정 비율(초기 10%)의 무작위 audited sample을 유지해 쉬운 표본만 평가되는 것을 막는다. 감사 budget이 없으면 profile을 낮추는 학습은 멈춘다.

초기에는 episode별 세 slot 중 **하나만 탐색**한다. 나머지는 고정해야 효과의 원인을 비교할 수 있다. 여러 전략이 동시에 바뀐 경우 bundle revision의 효과로만 보고하고 개별 기여를 만들어내지 않는다.

정책 version은 정해진 관측 batch(초기 20개 확정 관측)마다 재계산해 CAS로 공개한다. in-flight 실행은 이전 snapshot을 끝까지 사용한다. 최근 window에서 성과 하락·환경 fingerprint 변화가 생기면 통계를 삭제하지 않고 compatibility를 stale로 바꿔 canary로 재검증한다. 심각한 권한/데이터 손상 1건은 즉시 quarantine한다.

## 5. 인과 귀속과 승격

### 5.1 두 종류의 통계를 섞지 않는다

관찰 데이터는 ‘이 context에서 이 선택 뒤에 어떤 결과가 났나’를 알려준다. 선택된 방법이 어려운 작업에서만 쓰였다면 낮은 성공률만으로 나쁜 방법이라고 단정할 수 없다. Beta score는 선택 휴리스틱이며 인과 증거가 아니다.

인과 효과의 1차 근거는 **동일한 초기 환경의 무작위 paired control/treatment 실험**이다. 모델, quantization, prompt 구성, 정보, 도구·권한, verifier, token/time 한도, 기억 snapshot을 고정하고 전략 revision/실행 모드 하나만 변경한다. 순서는 AB/BA를 무작위화한다.

`ITT = mean(Y_treatment - Y_control)`를 사용한다. 전략 미준수·실패 실행을 제외하지 않는다. `P(success | followed)`는 진단 지표이며 causal effect로 표시하지 않는다. ‘준수했을 때만 집계’하면 성공하기 쉬운 실행만 남는 사후 선택 편향이 생긴다.

### 5.2 승격 상태와 근거 등급을 분리

상태: `candidate → eligible → sandbox_passed → canary → active`; `inactive`, `quarantined`, `superseded`, `rejected`로 이동 가능. 새 내용은 새 revision이다.

근거 등급:

- E0 `hypothesis`: 출처 있는 가설.
- E1 `structural`: schema·mock·권한·복구 경로 통과. 실전 효과 증명 아님.
- E2 `observed`: 실제 결과 관측. 대조군 없는 상관.
- E3 `controlled_preliminary`: 통제 비교를 했지만 효과 또는 표본의 불확실성이 큼.
- E4 `controlled_supported`: 사전 등록된 holdout 비교 기준을 충족. **그 모델·작업 분포·환경 범위에서만** 효과 지원.

owner가 명시한 필수 규칙은 E4가 아니어도 유효한 constraint다. 규칙의 정당성과 성공률 개선 주장은 다른 문제다. 기존 `validated`를 E4로 자동 변환하지 않는다.

### 5.3 성과 계산과 불확실성

독립 평가 단위는 task instance이며 같은 instance의 seed 반복은 묶는다. `d_i`는 instance 내 paired 평균 차이로 [-1,1]이다. 같은 template가 사실상 같은 과제면 template 단위 cluster로 올리고 N도 그 수로 센다.

```text
Δhat = (1/N) Σ d_i
보수적 halfwidth h = sqrt(2 × ln(2/α*) / N)
CI = [max(-1, Δhat-h), min(1, Δhat+h)]
α* = 0.05 / M    # 사전 등록한 동시 비교 개수 M
```

이는 독립 bounded 단위에 적용하는 보수적 Hoeffding bound다. 고정된 사례 목록에 대한 결과를 새로운 모든 작업에 일반화하지 않는다. 별도로 cluster bootstrap 10,000회 percentile interval을 제공하되 작은 N·biased task sampling에서 보장되는 구간이라고 부르지 않는다.

E4 기본 판정은 conservative lower bound > 사전 설정 δ(초기 0.05), 새로운 심각한 위험 사건 없음, 사전 비용 한도 내, held-out family에서도 regression 기준 충족이다. 이 bound는 비싸다. `N > 2 ln(2/α*)/(Δhat-δ)^2` 정도가 필요하므로 3회·10회로 효과 입증했다고 표시하지 않는다. 예산이 부족하면 E3 canary로 남길 수 있고, owner가 제한된 활성화를 선택해도 근거 등급은 바뀌지 않는다. 정확한 미해결 상태가 거짓 승격보다 낫다.

실험 중 유리한 시점에 멈춰 통과시키지 않는다. 종료 sample 수·비교·δ를 사전 고정한다. 연속 관찰은 위해 중단만 허용하며, 추가 표본으로 새 판단을 하려면 별도 experiment ID와 α 배분을 등록한다. 비용·준수·과제군 지표는 secondary로 표시한다.

### 5.4 기록된 선택을 이용한 정책 평가

propensity가 있는 데이터에만 off-policy 보조 평가를 허용한다. [무작위 로그 기반 평가 연구](https://arxiv.org/abs/1003.5956)가 강조하는 지원 범위와 선택 편향 문제를 적용한다.

```text
w_i = π_new(a_i|x_i) / p_logged(a_i|x_i)
V_SNIPS = Σ w_i y_i / Σ w_i
ESS = (Σ w_i)^2 / Σ w_i^2
```

logged p=0인 새 행동은 평가 불가능. raw weights와 cap=20을 적용한 민감도 결과를 둘 다 기록한다. clipped 추정은 편향이 생기므로 승격 단독 근거로 쓰지 않는다. ESS<30, missing outcome>10%, policy/context 변경이 큰 경우 inconclusive. 선택된 행동 로그를 재생한다고 실행하지 않은 도구 결과를 알 수 있는 것은 아니다. 최종 확인은 새 통제 실행이다.

## 6. 장기 기억·경험 검색과 통합

### 6.1 검색

권한 필터를 **검색 전과 결과 주입 전** 두 번 적용한다. memory/experience/improvement의 각자 다른 global bypass를 공통 KnowledgeVisibility로 통합한다.

1. lexical: 기존 tokens와 한국어 character bigram을 함께 사용, 기존 문구 검색 fallback 유지.
2. semantic: 선택한 로컬 embedding provider로 비동기 문서 인덱싱. 벡터는 `(modelDigest, dimensions, normalizationVersion, contentHash)`별 분리.
3. query embedding은 cache 우선. 첫 호출의 hard deadline 초기 150ms; 초과하면 lexical만 사용하고 background에서 cache 채움. 실제 목표 수치는 성능 실험으로 조정.
4. 각 top-30 결과를 `RRF(d)=Σ 1/(60+rank_j(d))`로 합치고 타입·적용 조건·stale·모순을 재정렬한다. [RRF 원 논문](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf)의 순위 결합만 참고한다.
5. 상위 4 experience, 최대 3 선택 절차, 원문 source link와 제한된 요약만 context에 넣는다. 필수 제약은 잘라내지 말고 공간 부족을 명시한다.

초기에는 별도 벡터 서버 없이 SQLite BLOB + Rust cosine scan을 사용한다. 범위별 최대 10,000 hot vector(메모리·지연 측정 후 조정); archival 검색은 명시적으로 늘릴 수 있다. 모든 vector를 영구 삭제해서 상한을 맞추지 않는다. FTS5 가용성을 확인할 경우 lexical index에 사용할 수 있으나 별도 engine 의존은 만들지 않는다.

로컬 adapter 첫 구현은 [Ollama `/api/embed`](https://docs.ollama.com/api/embed)의 `model`, `input`, `embeddings` 계약을 사용할 수 있다. `truncate=false`, NaN/0norm/dimension 불일치 거부. 로컬 서비스가 없으면 degraded lexical mode로 명시한다. private raw text를 외부 embedding API에 자동 전송하지 않는다.

### 6.2 중복·충돌·일반화

- **중복**: 동일한 authority/scope/applicability/contract hash/내용이면 같은 revision. evidence refs를 set union하고 원본 episode는 유지.
- **충돌**: 동일 fact key·author·scope에서 상반된 값이면 source와 유효기간 비교. owner 교정은 해당 사용자 선호를 우선 정정하지만 이미 관찰된 도구 결과를 과거부터 변경하지 않는다.
- **전략 충돌**: 순서 cycle, 같은 자원에 상반된 pre/postcondition이면 conflict set. 유사 문장이라고 merge하지 않는다. specificity·authority로 명확히 해소되지 않으면 optional 후보 제외 또는 unknown.
- **일반화**: 서로 다른 task instance 3개 이상의 유사 failure/성공에서 binding 변수 후보를 만들 수 있다. 수치는 발동 조건일 뿐 검증 기준 아님. 기존 성공 예시와 counterexample fixture 양쪽에서 재검증해 새 revision으로 낸다.
- **스킬화**: active procedure revision을 `skill.procedureRevisionId`로 참조하는 manifest 생성. 별도 자연어 복사본을 실행 권위로 만들지 않는다. 기존 draft UI/API와 호환한다.
- **비활성화**: 위험, compatibility 불일치, 새 controlled regression, 반복 미준수면 revision inactive/quarantined. 선택 통계 감쇠만으로 과거 evidence를 삭제하지 않는다.
- **working set**: scope별 최근 유효성과 활용 기준으로 hot/cold 분리. 치명적 실패·owner correction·active revision의 근거는 pin. 낮은 사용률만으로 삭제 금지.

### 6.3 공유와 신뢰

KnowledgeSource와 ShareProjection을 분리한다. raw transcript/경로/계정 ID는 원래 범위에 남는다. projection에는 추상 lesson, typed applicability, 민감값 없는 변수 schema, 근거 digest, 원본 범위 opaque ref만 넣는다. 원문 권한이 없는 agent가 opaque ref로 원문을 조회할 수 없어야 한다.

기본 공유는 같은 partition. partition 간 공유는 소유자가 설정한 source→destination 정책 안에서만 수행한다. `global` 문자열 하나로 모든 원문을 공개하지 않는다. 기존 global record는 원본 유지 후 `legacy_unreviewed`로 분류하고 cross-partition 주입을 잠근다. 승인된 추상본만 새 projection으로 공개한다.

다른 에이전트의 말은 가설 출처가 될 수 있지만 성공 evidence는 아니다. 실행 ID·artifact digest·oracle 결과가 있어야 재사용 성과를 기록한다. 공유 지식을 가져온 agent는 자기 모델/환경에서 cold prior부터 시작하고 cross-agent benchmark를 거친다.

## 7. Rust 구현 명세

### 7.1 모듈 책임

| 모듈 | 변경 책임 |
|---|---|
| `engine/improve.rs` | 기존 facade 유지. revision 제안·lifecycle·승격 API. counter 기반 승격 제거. |
| `engine/procedure.rs` | WIP를 typed compiler + 적용/순서 검사로 확장. 장대해지면 `procedure/{types,compile,guard}.rs`로 이동. |
| 신규 `engine/verification.rs` | goals acceptance의 실제 검사 재사용·통합. TaskContract·Outcome·Evidence. |
| 신규 `engine/adaptation/{mod,types,selector,stats}.rs` | context별 선택·확률·통계·policy version. 외부 agent framework 아님. |
| 신규 `engine/episode.rs` | 기존 execution들을 root task에 연결, 종료 transaction 요청. |
| 신규 `engine/retrieval.rs` | 기존 memory/experience를 조회하는 hybrid index adapter. 저장 원본 재사용. |
| 기존 `experience.rs`, `memory.rs` | grounding·visibility·projection·consolidation. |
| 기존 `chat/runtime.rs`, `context.rs` | RunGuard hook, snapshot 전달, 공통 completion, 보조 호출 계측. |
| 기존 `goals.rs`, `goal_control.rs` | parent task 전달, 동일 guard 호출, 기존 cancellation/repair 유지. |
| 기존 `learning.rs`, `reflect.rs`, `jobs.rs` | immutable snapshot·후보 생성·낮은 우선순위 background, idempotent job. |
| 기존 `db.rs`, `migrations.rs` | schema migration·transaction·indexed query·read errors. |
| 신규 `src/bin/dominion-benchmark.rs` | 독립 실험 orchestrator. main binary 자동 선택 변화 주의: Cargo default-run 유지. |

### 7.2 핵심 타입 계약

아래는 새 API의 목표 명세다. 기존 코드에 이미 존재한다고 오해하지 않는다. `Id`는 UUID/String wrapper, `Revision`은 canonical JSON SHA-256, `EngineResult<T>`는 명시적인 error enum을 가진 Result다. serde는 camelCase, enum은 tagged `type`, unknown field 거부, 배열·문자열 길이 상한을 적용한다.

```rust
pub enum CheckVerdict { Pass, Fail, Unknown, NotApplicable }
pub enum Compliance { Followed, Violated, NotTriggered, Unknown, NotApplicable }
pub enum TaskOutcome { Succeeded, Failed, Unknown, Cancelled, AnsweredUnverified }
pub enum FailureClass {
    InvalidArguments, MissingPrerequisite, ToolFailure, WrongArtifact,
    MissingEvidence, ModelNoncompliance, SemanticUncertain,
    PermissionDenied, EnvironmentUnavailable, OutcomeUnknown, BudgetExhausted,
}
pub struct EvidenceRef {
    pub kind: EvidenceKind, pub id: Id, pub revision: Option<Revision>,
    pub scope: ScopeRef, pub observed_at: Timestamp, pub digest: Option<String>,
}
pub struct TaskContract {
    pub task_id: Id, pub revision: Revision, pub source: InstructionSource,
    pub scope: ScopeRef, pub intent: TaskIntent, pub family: String,
    pub mandatory: Vec<CheckSpec>, pub bindings: BindingSet,
    pub permissions: PermissionSnapshot, pub budget: BudgetLimits,
}
pub struct StrategyRevision {
    pub strategy_id: Id, pub revision: Revision, pub parent: Option<Revision>,
    pub applicability: Predicate, pub authority: StrategyAuthority,
    pub procedure: Option<ProcedureSpec>, pub advice: Option<String>,
    pub checks: Vec<CheckSpec>, pub provenance: Vec<EvidenceRef>,
    pub compatibility: Compatibility, pub scope: ScopeRef,
}
pub struct Decision {
    pub id: Id, pub task_id: Id, pub slot: DecisionSlot, pub context: ContextKey,
    pub candidates: Vec<CandidateScore>, pub selected: ArmId,
    pub probability: f64, pub policy_revision: Revision, pub rng_seed: u64,
}
pub struct Observation {
    pub decision_id: Id, pub attempt_id: Id, pub outcome: TaskOutcome,
    pub compliance: Compliance, pub checks: Vec<CheckResult>,
    pub evidence: Vec<EvidenceRef>, pub costs: CostVector,
    pub regime: ObservationRegime, pub labels: LabelProvenance,
}
pub struct SelectionSnapshot {
    pub contract: TaskContract, pub strategies: Vec<StrategyRevision>,
    pub decisions: Vec<Decision>, pub context_items: Vec<ContextItem>,
    pub memory_watermark: String, pub snapshot_hash: Revision,
}
```

`ObservationRegime`은 production_observed / sandbox_control / sandbox_treatment / canary다. 각 필드의 missing/default는 migration에서만 처리하고 새 실행에서는 필수다.

`CheckSpec` 지원 타입:

| 타입 | 필드 / 판정 |
|---|---|
| ResourceExists/Absent/Changed | ResourceRef, baseline digest, observation freshness |
| FileEquals/Contains | ResourceRef, 명시적 expected binding, byte/text mode, maxBytes |
| JsonSchema/JsonPredicate | artifact JSON pointer, 제한된 schema·비교 연산 |
| ToolSucceeded | tool capability, typed arg predicates, operation evidence |
| Before/After | node 또는 operation selector, per_target/task 범위, invalidation rule |
| SemanticRubric | frozen rubric, 필요한 evidence, judge profile, calibration version |

FileEquals는 임의 trim하지 않는다. newline normalization이 필요한 계약은 명시한다. FileExists는 읽을 수 있는 UTF-8 여부와 다르다. Unknown(접근 오류)과 Absent(ENOENT)를 구분한다. check는 read-only; 변경이 필요한 검증 준비도 정상 tool operation으로 승인·기록한다.

### 7.3 함수 시그니처

```rust
// 기존 facade를 통해 점진 교체
pub fn propose_candidate(state: &AppState, input: CandidateInput)
    -> EngineResult<StrategyRevision>;
pub fn compile(spec: &StrategyRevision, catalog: &ToolCatalog,
    contract: &TaskContract) -> Result<CompiledProcedure, Vec<CompileError>>;
pub fn select(state: &AppState, task: &TaskContract, candidates: &[StrategyRevision],
    policy: &PolicySnapshot, seed: u64) -> EngineResult<SelectionSnapshot>;

pub enum GuardDecision {
    Allow, Block(Violation), NeedBinding(BindingRequest), Stop(StopReason),
}
impl RunGuard {
    pub fn new(snapshot: SelectionSnapshot, ledger: BudgetLedger) -> Self;
    pub fn before_call(&mut self, call: &TypedToolIntent,
        trace: &VerifiedTrace) -> GuardDecision;
    pub fn next_ready_intent(&self, trace: &VerifiedTrace)
        -> Option<TypedToolIntent>;
    pub fn after_call(&mut self, operation: &DurableOperation) -> EngineResult<()>;
    pub async fn finish(&mut self, ctx: &VerificationContext<'_>)
        -> EngineResult<AttemptEvaluation>;
}
pub async fn dispatch_intent(ctx: &ExecutionContext<'_>, intent: TypedToolIntent,
    guard: &mut RunGuard) -> EngineResult<DurableOperation>;
pub fn finalize_once(store: &Store, finalization: &Finalization)
    -> EngineResult<FinalizeResult>; // AlreadyApplied or Applied
pub async fn retrieve(state: &AppState, query: RetrievalQuery,
    deadline: std::time::Instant) -> EngineResult<RetrievedContext>;
pub fn update_policy(store: &Store, batch: &[Observation], expected: Revision)
    -> EngineResult<PolicySnapshot>;
pub async fn run_experiment(ctx: &JobContext, manifest: ExperimentManifest)
    -> EngineResult<ExperimentResult>;
```

`VerificationContext`는 cancel handle, deadline, bounded read adapter, immutable contract, evidence reader를 포함한다. disk/CPU 작업은 bounded blocking executor에서 수행하며 DB Mutex를 잡은 상태로 await하지 않는다. `dispatch_intent`는 모델과 procedure 양쪽에서 공유하는 현재 승인·도구실행 경로다. 두 번째 tools executor를 만들지 않는다.

### 7.4 저장 스키마와 트랜잭션

원본 `records`에 추가할 kind:

| kind | key | 주요 내용 |
|---|---|---|
| task_episode | task UUID | contract, source, attempts, root status, selection snapshot refs |
| improvement_revision | SHA-256 | 불변 StrategyRevision canonical JSON |
| strategy_lifecycle | strategy UUID | active revision, state, evidenceGrade, revision counter |
| strategy_decision | decision UUID | 후보·score·확률·snapshot |
| strategy_observation | hash(decision,attempt,labelRevision) | 준수·결과·근거·비용·regime |
| policy_snapshot | revision hash | slots·bucket 설정·승격 기준·생성 cutoff |
| policy_stats | hash(scope,context,arm,regime) | S/F·준수·비용·unknown, lastUpdate, version |
| experiment | experiment UUID | immutable manifest, 상태, 근거 등급 |
| experiment_trial | hash(experiment,pair,arm,seed) | 환경 hash·assignment·원시 result refs |
| knowledge_projection | projection hash | scope 허가·추상본·provenance opaque ref |
| failure_hypothesis | hypothesis UUID | 관찰·가설·반증 조건·후속 revision |

기존 `improvement`는 UI·API용 facade/projection을 유지한다. 기존 experience는 episodeRefs, applicability, sourceGrade, conflictSet, projectionId를 추가한다. canonical JSON hash는 object key 정렬·명시적 schemaVersion·UTF-8 직렬화 규칙을 고정한다.

추가 SQL 테이블은 학습 원본을 복제하는 DB가 아니라 기존 record의 원자성·인덱스를 위한 보조 구조다.

```sql
CREATE TABLE adaptation_schema(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
CREATE TABLE adaptation_finalizations(
  attempt_id TEXT NOT NULL, evaluation_revision TEXT NOT NULL,
  payload_hash TEXT NOT NULL, applied_at TEXT NOT NULL,
  PRIMARY KEY(attempt_id, evaluation_revision)
);
CREATE TABLE adaptation_updates(
  observation_id TEXT NOT NULL, projection_version TEXT NOT NULL,
  PRIMARY KEY(observation_id, projection_version)
);
CREATE TABLE knowledge_vectors(
  record_kind TEXT NOT NULL, record_id TEXT NOT NULL, content_hash TEXT NOT NULL,
  model_digest TEXT NOT NULL, dimensions INTEGER NOT NULL,
  vector BLOB NOT NULL, created_at TEXT NOT NULL,
  PRIMARY KEY(record_kind, record_id, content_hash, model_digest)
);
CREATE INDEX idx_operation_run ON records(json_extract(data,'$.runId'))
  WHERE kind='execution_operation';
CREATE INDEX idx_execution_task ON records(json_extract(data,'$.taskId'))
  WHERE kind='execution';
```

`finalize_once`는 하나의 SQLite transaction에서 terminal execution/episode·observation·finalization marker를 기록한다. 동일 key+동일 payload면 재호출 성공, 다른 payload면 conflict error. terminal record를 성공으로 저장하기 전에 모든 mandatory evaluation이 존재해야 한다.

통계는 해당 observation을 별도 transaction에서 읽고 `adaptation_updates` INSERT와 stats 갱신을 함께 commit한다. 중간 crash 후 미반영 observation을 다시 처리한다. 재판정은 새 labelRevision으로 저장하고 옛 결과의 가산을 반복하지 않는다. 영향받은 bucket을 immutable observations에서 재구축하는 projection version을 발행한다.

단조 operation sequence는 attempt record의 nextSeq 증가와 operation intent 저장을 같은 transaction에서 수행한다. `running` 부작용 이후 crash는 Unknown. 성공으로 추정하거나 자동 재실행하지 않는다. timestamp는 표시용이고 순서 증명의 기준이 아니다.

### 7.5 마이그레이션

1. SQLite backup API 또는 서버가 정지된 일관된 복제본 사용. live WAL DB 파일만 복사하지 않는다.
2. schema version 생성, 필요한 열은 JSON additive. 기존 execution/message/operation 내용 보존.
3. legacy adopted/validated는 `legacy_observed` revision과 E0/E2 구분으로 이전. 과거 uses/successes는 `legacyCounters`로만 보관; 새 causal score의 초기 성공으로 사용하지 않음.
4. WIP verification은 strict compiler 통과 시 E1, 실패 시 원본을 보관하고 invalid reason 기록. 필수 조건 일부를 삭제해 통과시키지 않음.
5. provenance 없는 legacy global은 local/source-scope 가시성만 유지하고 cross-scope는 검토 대상으로 전환.
6. migration은 재실행해 같은 hash/record가 생기는 idempotent transaction. readback 실패 시 version을 올리지 않음.
7. feature flags: `adaptation.observe`, `adaptation.enforce`, `adaptation.select`, `adaptation.explore`. 기존 필수 acceptance·권한·취소는 어느 flag로도 끌 수 없음.
8. downgrade는 새 record를 삭제하지 않고 selection/enforcement 확장만 끄며 기존 안정 guard 사용. 사용자 데이터·미커밋 소스 자동 reset 금지.

### 7.6 runtime 의사코드

```text
source = resolve_authorized_instruction(request, optional_parent_task)
task = load_or_create_episode(source)               # 합성 peer/goal 문자열은 source 아님
snapshot = freeze_context_and_select(task)
attempt = persist_attempt(task, snapshot)
guard = RunGuard(snapshot, child_budget(parent_budget))
while budget_remaining:
    observe_cancel_epoch()
    intent = guard.next_ready_intent(trace) OR model.next_intent(context)
    if intent is final_answer:
        evaluation = guard.finish(current_evidence)
        if evaluation.requires_repair AND repair_allowed:
            select_recovery_with_logged_propensity(); continue
        break
    decision = guard.before_call(intent, trace)
    persist_proposal_and_decision(decision)
    if blocked: budget.charge_proposal(); bounded_repair_or_stop(); continue
    operation = dispatch_through_existing_approval_and_tools(intent)
    persist_operation_result_and_artifact_refs(operation)
    guard.after_call(operation)
evaluation = guard.finish_even_on_early_exit_or_budget_exhaustion()
finalize_once(attempt, evaluation, observations)
enqueue_projection_updates_and_triggered_learning_once()
```

## 8. 반응 속도·재시작·운영 제약

- foreground의 기본 경로는 추가 LLM 호출 0. 단순 검증은 Rust와 bounded readback, semantic 필요 시만 reviewer 호출. 상시 planner와 상시 reflect는 제거 대상이지만 기존 수행·검증 기능은 제거하지 않는다.
- 기본 retrieval+guard CPU p95 목표 50ms 이하(10k hot item 기준). query embedding deadline 별도. 실제 4B inference 지연과 분리해 계측한다.
- background model 작업은 provider별 제한된 slot을 공유하고 foreground 대기 시 새 작업 시작을 미룬다. 이미 시작한 GPU 호출은 즉시 선점 가능하다고 가정하지 않는다. timeout을 제한하고 queue wait를 기록한다.
- `learn`, `reflect`, `embed`, `consolidate`, `policy_update`는 durable job로 처리. 주기적 full scan 대신 변경 record cursor와 적절한 index를 사용.
- 취소·승인 거절은 strategy가 우회하지 못한다. 기존 kill/process-group·epoch·pending approval 복구 테스트를 보존한다.
- `4317` 서비스, 실제 DB, 토큰, Discord/Telegram 발송, 브라우저·DMG·UI는 이번 범위 밖. 실험은 별도 data/workspace/profile이며 실서비스 자격증명을 복사하지 않는다.

## 9. 미해결 위험과 실험이 필요한 판단

| 위험 | 설계로 막는 부분 | 실제로 확인할 부분 |
|---|---|---|
| 4B가 절차를 무시 | typed guard, bounded repair, harness-assisted mode | 새 작업에서 성공률이 얼마나 증가하는가 |
| 잘못된 성공 조건 | owner contract와 strategy 조건 분리, provenance | 자연어 의도를 얼마나 정확히 계약으로 만드는가 |
| 잘못된 학습 가설 | candidate 격리, counterexamples, 통제 비교 | 반성 모델의 후보 생성 수율·비용 |
| semantic judge 편향 | blinded rubric, unknown, 독립 labels | 해당 분야에서의 false accept/reject율 |
| 희소 데이터·표본 비용 | prior 제한, 근거 등급, 보수적 CI | 실용 예산으로 유의미한 차이를 발견할 수 있는가 |
| 자원·도구 변화 | fingerprint·stale·재검증 | 실제 drift 감지 민감도·false rollback |
| 잘못된 일반화 | parameter binding·negative examples·새 revision | 한국어 paraphrase와 unseen family 전이 |
| 프롬프트 과부하 | 선택 snapshot·토큰 상한·중복 제거 | 4k context 모델에서 최적의 정보량 |
| 원문 유출·오염 | projection ACL·출처 검사·global migration | 실제 기존 데이터에 대한 migration 검토 |
| 비멱등 도구 중단 | Unknown·readback·reconcile | 각 외부 tool의 조회·멱등성 지원 |

이 설계는 범용 강화를 위한 측정·제어 구조를 제공한다. 범용 AGI 성능이나 모든 모델에서의 지속적인 단조 성장을 보장하지 않는다. 다음 구현의 첫 성공은 **일반 채팅에서 준수·실제 결과·불확실성을 정확히 기록하고, 동일 모델의 대조 실험에서 개선 여부를 재현 가능하게 측정하는 것**이다. 그 위에서 정책의 선택이 실측 성과에 따라 달라져야 한다.

---

# 제2부

# DeepSeek 개발 지시서 — DOMINION 자기개선

설계 기준: [SELF-IMPROVEMENT-DESIGN](/Users/gim-yeonsu/dominion-rust/docs/SELF-IMPROVEMENT-DESIGN-2026-11-05.md).
검증 기준: [SELF-IMPROVEMENT-BENCHMARK](/Users/gim-yeonsu/dominion-rust/docs/SELF-IMPROVEMENT-BENCHMARK-2026-11-05.md).

## 그대로 전달할 첫 지시

> `codex/native-memory-harness`에서 아래 순서대로 구현하라. 현재 기준 HEAD는 `ac1d0a0`이나 작업 트리에 procedure 관련 미커밋 변경이 있다. 반드시 현재 diff를 보존하고 통합하라. 새 런타임을 만들지 말고 기존 Rust 일반 채팅 실행·SQLite 기록·learn/reflect 파이프라인을 확장하라. `/goal`은 같은 코어의 추가 호출자다. 주입·준수·작업 성공·인과 효과를 별도 데이터로 다루고, 완료 우회 경로를 먼저 닫아라. 운영 서버 `4317`이나 실제 DB·승인 설정을 실험에 사용하지 마라. Node는 프론트 빌드 도구로만 유지한다. 브라우저 자동화·FreeLLMAPI 가입·DMG·UI 작업으로 범위를 넓히지 마라. 아래 단계마다 관련 검증과 정확한 변경 설명을 남기고 별도 커밋하라. 실제 모델 성공률 개선은 실험 결과가 있을 때만 보고하라.

## 공통 규칙

- 설계 문서의 필드·수식·상태 이름을 단일 계약으로 사용한다. 명세 변경이 필요하면 코드와 문서를 같은 단계에서 수정하고 이유를 적는다.
- 각 단계의 테스트는 단순 함수 복사 테스트가 아니라 완료 조건을 깨뜨리는 반례를 포함한다.
- 새 revision은 불변이다. 사용자의 원문·기존 경험·미커밋 변경·실패 trial을 삭제하지 않는다.
- 기존 승인·취소·프로세스 종료·재시작 Unknown 처리·evidence gate를 보존한다.
- 새 테스트는 임시 data/workspace를 사용한다. 기존 테스트를 전체 실행하기 전에도 실서비스 의존이 없는지 확인한다.
- 새 기능을 구현할 때 이미 작성된 WIP `procedure.rs`를 그대로 ‘검증 완료’로 승격하지 않는다. strict compiler와 반례로 확인한다.
- 커밋 설명에는 구현, 검사한 범위, 실제 모델 실행 여부, 미완료 항목을 구분한다. 테스트 수를 추측하지 않는다.

## 의존 순서

```text
S0 → S1 → S2 → S3 → S4 → S5 → S6 → S7
                 └──────────────→ S8 → S9
S3 + S6 + S7 + S9 → S10 → S11 → S12
```

S7까지는 측정·준수 기반을 닫는 첫 묶음이다. S8/S9는 검색·장기 성장, S10/S11은 선택 시스템 자체의 적응이다. S12 실험이 끝나기 전 ‘성능 개선 입증’이라고 쓰지 않는다.

## S0. 기준선과 실험 보호

**목적:** 기존 수정 보존과 운영 환경 오염 방지.

**파일:** `scripts/task-benchmark.sh`, 신규 benchmark 실행 설정·fixture 디렉터리, 작업 시작 기록.

**입력:** 현재 HEAD, diff, 설계 문서, 기준 난제 보고서.

**요구사항:**

1. HEAD·원격 branch·tracked diff·untracked procedure hash를 저장한다. 사용자 변경을 reset하거나 자동 stash하지 않는다.
2. 기존 benchmark의 default 4317 사용을 제거한다. 명시적인 실험 target, disposable root sentinel, 서버 identity 확인 없으면 exit nonzero.
3. repo/bench를 `rm -rf`하는 동작과 전역 autoApproveAll 변경을 제거한다. 허용된 fixture 도구만 실험 환경에서 승인한다.
4. benchmark 설정은 운영 data/workspace 경로를 거부한다. URL 포트 검사만으로 격리를 판정하지 않는다.

**연결:** 이후 Rust runner가 완성되기 전까지 기존 shell 진입점이 위험한 실행을 하지 않게 한다.

**테스트:** 주소 미지정, 4317, symlink로 운영 workspace 참조, sentinel 불일치가 네트워크 mutation 전에 거부되는지 확인.

**완료:** benchmark 기본 실행이 운영 서버에 쓰지 않는다. 현재 코드 상태가 재현 가능한 시작 manifest로 남는다.

## S1. 타입·revision·transaction 기반

**목적:** 실행 결과와 학습 통계를 재시작·동시성에도 정확히 한 번 연결.

**파일:** `src/db.rs`, `src/migrations.rs`, 신규 `engine/episode.rs`, `engine/adaptation/types.rs`, `engine/verification.rs` 타입 부분.

**입력:** S0 baseline, 설계 §2·§7의 데이터 계약.

**요구사항:**

1. TaskContract/StrategyRevision/Decision/Observation/EvidenceRef/Outcome 타입, strict serde validation 구현.
2. existing records와 messages 유지. additive schema·index·finalization/update marker 추가.
3. immutable revision canonical hash, terminal finalization transaction, idempotent observation 처리 API 구현.
4. `Store::all()` 오류가 빈 데이터처럼 보이는 경로 대신 새 학습 경로에서 Result 기반 query 사용.
5. legacy counter는 보존하되 새 점수에 합산하지 않는다. 기존 validated에는 E4 근거가 없다고 표시.

**연결:** 기존 API용 improvement record는 facade로 유지. 새 테이블은 동일 SQLite 연결 사용.

**테스트:** 같은 finalization 2회, 같은 ID 다른 payload, 두 worker 동시 종료, stats 반영 전 crash, migration 2회, 깨진 JSON.

**완료:** 하나의 attempt가 한 번만 관측·집계되고, crash 후 누락된 projection을 복구할 수 있다. 기존 record는 유실되지 않는다.

## S2. 절차 compiler와 검증기

**목적:** 자연어 전략을 검증 가능한 제한된 계약으로 표현.

**파일:** `engine/procedure.rs`, `engine/verification.rs`, `engine/tools.rs` capability adapter, `engine/goals.rs` 검사 함수.

**입력:** S1 타입·revision·EvidenceRef.

**요구사항:**

1. WIP normalize의 조건 필터링을 전체 schema 오류 거부로 변경한다.
2. typed arg/resource 비교, canonical workspace 경계, symlink·크기 제한, operation seq 검증.
3. DAG cycle·자기 의존·미정의 binding·지원 안 되는 도구·모순 필수 조건 거부.
4. Before는 각 target event에서 검사하고 per_target/task lifetime·invalidation을 정의한다.
5. NotTriggered/Unknown/Violated/Followed를 구분한다. 아무것도 하지 않은 실행은 Followed가 아니다.
6. goals의 검사 구현을 공통 모듈로 이동하고 기존 wrapper와 successCriteria 호환 입력 유지.

**연결:** existing execution_operation과 resource baseline을 입력으로 사용. 새 도구 실행기를 만들지 않는다.

**테스트:** 파일 이름 부분일치, `echo filename`, 이미 존재하는 결과, empty contains, 잘못된 condition 하나, symlink 탈출, binary 존재, permission error, target 반복, timestamp 동률, 순서 역전.

**완료:** 결정적 pass에는 해당 조건을 지지하는 evidence refs가 있고, 검사할 수 없는 조건은 Unknown이다.

## S3. 일반 채팅 RunGuard 연결

**목적:** 기존 메인 실행에서 완료 우회·기록 불일치를 제거.

**파일:** `chat/runtime.rs`, `chat/context.rs`, `chat/evidence.rs`, `chat/mod.rs`, `episode.rs`.

**입력:** S1 transaction, S2 검증기.

**요구사항:**

1. owner 원문과 actual sourceMessageId로 task 생성. context selection과 revision snapshot 한 번만 고정.
2. 모델 호출/하네스 호출이 공통 dispatch·approval·cancellation·operation 저장을 사용하도록 추출.
3. 도구 전 guard, 도구 후 evidence, 최종 single finish 경로 연결.
4. force_final·acceptance_passed·repair 소진·MAX_ROUNDS·tool disabled 모두 필수 절차를 검사.
5. 총 repair 2회와 공통 BudgetLedger. 승인 거절·취소를 실패 학습으로 오염시키지 않음.
6. deterministic 단순 작업은 기본 추가 planner 호출 없이 실행. 불확실할 때만 plan/review 사용.
7. `status` 호환과 별도로 taskOutcome/compliance/exposure 전송·저장. 성공 경험은 검증된 outcome에서만 생성.

**연결:** 기존 tool parsing/native/compat, SSE, attachment, session lock, epoch를 보존한다.

**테스트:** 말로만 완료, 파일 결과만 맞고 필수 순서 위반, 2회 repair 후 실패, force_final 경로, 선택적 전략 실패+작업 성공, tools disabled, cancel 중 model/tool/approval.

**완료:** 일반 채팅에서 구조적 조건 위반이 Succeeded로 남지 않는다. 단순 대화는 도구 강제 없이 계속 가능하다.

## S4. 약한 모델용 harness-assisted 절차

**목적:** 모델이 매번 절차를 기억해야만 실행되는 한계를 줄임.

**파일:** `procedure.rs`, 공통 tool dispatch, `runtime.rs`.

**입력:** S3 공통 승인·도구 경로, 검증된 ProcedureSpec.

**요구사항:**

1. ready DAG node 선택, verified binding, next intent 생성 구현.
2. model_guided와 harness_assisted를 revision별 선택. candidate는 자동 실행 모드로 들어가지 못함.
3. literal/owner binding/앞 node JSON pointer만 허용. 길이·type·resource 제약 적용.
4. argv 도구는 existing terminal_exec를 사용. 쉘 문자열 조합으로 대체하지 않음.
5. node 실행도 기존 승인·취소·budget·side effect uncertainty 대상. source=procedure를 기록.

**연결:** 사용자에게 새로운 권한을 요구하는 별도 승인 체계를 만들지 않는다. 기존 허용 범위 안에서 같은 policy를 적용한다.

**테스트:** 절차를 계속 무시하는 mock model에서 준비된 node가 실제 실행됨; binding 부재·악성 tool output·승인 거절·취소·재시작에서는 실행 우회 없음.

**완료:** Rust가 실제로 수행한 절차와 모델 자발적 준수를 분리해 측정할 수 있다.

## S5. goal·room·복구 연결

**목적:** 모든 채널이 같은 검증·보상 단위를 사용.

**파일:** `goals.rs`, `goal_control.rs`, `rooms.rs`, `jobs.rs`, `chat/evidence.rs`.

**입력:** S3/S4 run guard와 task linking.

**요구사항:**

1. goal 합성 prompt에 parentTaskId/contract revision 전달. owner instruction으로 재해석하지 않음.
2. room의 원문당 root episode 하나, agent별 attempt와 관찰자 분리.
3. peer가 수행한 동일 작업은 근거로 참조 가능하지만 새 수행·독립 성공 표본으로 복제하지 않음.
4. goal 기존 reviewer/replan 유지. child guard와 parent evaluator가 같은 model review를 중복 호출하지 않게 ownership 명시.
5. Unknown 부작용의 자동 replay 금지. persisted operation readback과 별도 reconcile 상태.

**테스트:** 원문 하나에 9개 reply, 인용·peer 명령, goal child retry, 재시작 inFlight, cancelled epoch, old evidence의 새 요청 재사용 거부.

**완료:** 일반 채팅과 goal의 동일 fixture에 같은 oracle 판정. root 성공 집계 1회.

## S6. learn·reflect를 근거 있는 후보 생성기로 정리

**목적:** 자기평가를 곧바로 효과가 입증된 전략으로 만드는 경로 제거.

**파일:** `learning.rs`, `reflect.rs`, `improve.rs`, `experience.rs`, `memory.rs`, `jobs.rs`.

**입력:** S1 provenance/revision, S3 episode 결과.

**요구사항:**

1. learn의 immutable snapshot end 유지, cursor 별도 이동, 모든 page 처리 후 checkpoint.
2. reflect 입력에 correctionId·source episode·throughMessageId·operation IDs 고정. 최근 세션 전체 재조회로 다른 작업 혼입 금지.
3. 기억은 공통 grounding으로, 추측은 hypothesis로. assistant 주장만 근거인 strategy는 거부.
4. propose 기본 candidate. verified procedure 추가도 새 revision 생성. 자동 5회 validated 로직 제거.
5. 성공/실패 경험과 가설을 분리하고, failure taxonomy·예상 효과·반증 조건 저장.
6. 반복 가설 dedupe, source+revision 기반 enqueue_once, 학습/반성 admission 원자화·낮은 우선순위·budget 적용.

**테스트:** 30개 이상 학습 페이지, 지연된 reflect 중 새 메시지, evidence 위조, 봇 교정, revision 변경 시 옛 통계 분리, 중복 worker, failed learning 기록.

**완료:** ‘저장됨’과 ‘효과 검증됨’이 API와 DB에서도 분리되고 원시 source를 추적할 수 있다.

## S7. 실험 runner와 불변 결과 저장

**목적:** 성공률 개선을 평가할 도구 완성.

**파일:** 신규 `src/bin/dominion-benchmark.rs`, benchmark manifests/fixtures, `scripts/task-benchmark.sh`, 필요하면 `Cargo.toml` default-run 설정.

**입력:** S0 격리 기준, S1/S3 기록, S6 후보 snapshot. 상세 명세는 benchmark 문서.

**요구사항:**

1. 임시 인스턴스 생성·identity handshake·prepared DB/workspace 복제·hash 확인.
2. paired AB/BA assignment, 단일 revision 차이, eval 중 learning/reflect/selection 갱신 동결.
3. actual artifacts·operation order·terminal outcome oracle, SSE/HTTP 실패 분류.
4. trial 결과 JSONL·raw artifacts·manifest·diff fingerprint 보존. result-only 재집계 지원.
5. smoke와 실제 모델 비교를 별도 명령/결과로 구분. 실패·timeout·0/0을 성공으로 만들지 않음.

**테스트:** fake model과 clock으로 AB/BA, reset 실패, 학습 job 실패, 부분 종료 resume, 같은 trial 중복 저장, production 경로 거부.

**완료:** 실제 모델을 쓰지 않아도 독립된 두 arm이 같은 초기 상태에서 실행되고 결과 계산을 재현할 수 있다.

## S8. 권한 있는 의미 검색

**목적:** 한국어 paraphrase·다른 자원에서 유사한 교훈 검색.

**파일:** 신규 `retrieval.rs`, `memory.rs`, `experience.rs`, `context.rs`, `jobs.rs`, `db.rs` vector adapter.

**입력:** S1 schema, S6 source 분류.

**요구사항:**

1. lexical+character bigram fallback, local embedding adapter, model/content hash별 index.
2. cosine top-k와 RRF, eligibility·scope 전후 검사, snapshot token budget.
3. embedding 비동기 backfill, query cache/deadline, NaN/shape mismatch 거부.
4. retrieve는 exposure 기록만. 실제 성공/준수 카운터 증가 금지.

**테스트:** 한국어 paraphrase 평가셋, 같은 단어지만 다른 작업 negative, provider offline, dimension change, secret/scope leak, cold cache latency.

**완료:** lexical-only와 hybrid 검색을 비교할 수 있으며, embedding 장애가 foreground 실행을 멈추지 않는다.

## S9. 통합·공유·스킬 revision

**목적:** 누적 데이터가 오염·모순·무제한 prompt가 되지 않도록 관리.

**파일:** `experience.rs`, `memory.rs`, `improve.rs`, skill record adapter, `migrations.rs`.

**입력:** S6 provenance, S8 공통 visibility.

**요구사항:**

1. duplicate와 contradictory를 별도 판단. 내용·조건·source를 무시한 signature merge 제거.
2. private source와 abstract share projection 분리. source/destination grant·readback ACL 적용.
3. legacy global 검토 상태, 치명적 실패/owner correction pin, scope별 hot/cold 관리.
4. 여러 task에서의 일반화는 새 revision. counterexample 실험 전 기존 active 대체 금지.
5. skill은 검증된 procedure revision 참조. 따로 저장된 문구로 실행 권한을 확장하지 않음.

**테스트:** 상반된 교훈, owner 정정·도구 사실 구분, secret 포함한 lesson, 허용/금지 cross-agent, 원문 ref를 통한 접근, archived critical failure 검색.

**완료:** 공유는 재사용 가능한 추상본을 전달하고 원문 범위를 넓히지 않는다. 반례를 잃지 않는다.

## S10. 전략 선택과 원자적 성과 갱신

**목적:** 누적 실제 성과에 따라 다음 선택이 바뀌도록 구현.

**파일:** `adaptation/selector.rs`, `stats.rs`, `improve.rs`, `context.rs`, policy API의 필요한 read-only 출력.

**입력:** S3 observations, S7 실험, S9 revision/visibility.

**요구사항:**

1. 설계 §4의 context key·Beta shrinkage·감쇠·비용/위험 점수·ε-greedy 구현.
2. no_optional_strategy arm, 정확한 propensity, 후보·seed·policy revision 저장.
3. observational/controlled 통계를 분리하고 idempotent update projection 사용.
4. 수정 revision·모델·tool schema 변경 후 통계 격리. 위험 사건 즉시 quarantine.
5. lifecycle과 evidence grade 분리. 통제 효과 CI가 기준에 못 미치면 E3로 보존.

**테스트:** 고정 seed 확률 재현, p 합계=1, eligibility 제외, unknown/cancel, decay, sparse parent prior, duplicate observation, p=0 off-policy 거부.

**완료:** 서로 다른 실패 유형에 주어진 동일 결과 로그를 반영하면 적합한 arm의 선택 확률이 증가한다. 이것은 알고리즘 동작 검증이며 실제 모델 성장 주장과 구분한다.

## S11. 복구·검증 방식의 적응

**목적:** 전략 목록뿐 아니라 선택 시스템 자체가 성과를 사용.

**파일:** `adaptation`, `verification.rs`, `runtime.rs`, `jobs.rs` audit sampling.

**입력:** S10 selector·logged decisions, S7 독립 oracle.

**요구사항:**

1. recovery와 verification_profile slot 추가. episode당 한 slot만 실험.
2. recovery는 해당 실패 시점 state/budget을 저장하고 terminal outcome을 연결.
3. verifier reward는 독립 label과 일치 여부에서만 산출. 자기 Pass를 학습 정답으로 사용 금지.
4. randomized audit sample·coverage·miss rate·review cost 저장. mandatory checker 비활성화 금지.
5. fixed policy와 adaptive policy를 같은 동결 경험 snapshot으로 비교.

**테스트:** 복구 선택 변경, outcome unknown에서 retry 선택 불가, semantic false accept 감사, label 없는 검증기는 점수 갱신 안 함, 탐색 slot 충돌.

**완료:** ‘무엇을 기억했는가’뿐 아니라 ‘왜 이번에 이 복구/검증 방법을 골랐는가’를 decision record에서 설명할 수 있다.

## S12. 실제 동일 모델의 성장 실험·전달

**목적:** 구현 정확성과 실제 실행 능력 개선을 분리해 판정.

**파일:** experiment manifests, 결과 artifact 디렉터리, 결과 보고서. 엔진 수정은 발견한 결함에 한정.

**입력:** S0~S11 완료 코드, 고정 실제 4B 모델 fingerprint, benchmark 문서의 사전 등록 manifest.

**요구사항:**

1. mock suite 후 실제 모델 smoke. 학습 완료 barrier 확인 후 paired evaluation.
2. 동일 정보의 prompt-only/guard/assisted 비교와 학습 경험 효과 비교를 분리.
3. holdout task·cross-agent·한국어 paraphrase·negative applicability 포함.
4. N, effect, CI, strict success, compliance, unknown, latency, token·tool 비용을 원시 trial과 함께 공개.
5. 0 개선이면 그대로 보고. 성공할 때까지 benchmark를 바꾸고 옛 실패를 지우지 않음.
6. 자동 소스 수정·배포는 수행하지 않음. 코드 변경 제안은 별도 검증과 사용자 승인 경로.

**완료:** 성능 상승이면 근거 범위를 정확히 명시; 불확실/악화면 해당 revision 상태와 다음 가설을 기록. 실제 성능 판정이 없어도 구현·측정 상태를 정직하게 넘길 수 있다.

## 각 단계 종료 보고 형식

```text
단계 / commit / baseline HEAD 및 dirty diff hash
바뀐 실행 경로:
보존한 기존 기능:
검증 명령과 실제 결과:
실제 모델 실행 여부 / model fingerprint:
관측된 성공·실패·불확실 결과:
미완료 항목과 다음 단계 입력:
```

최종 결과는 ‘기억이 늘었다’, ‘전략을 주입했다’, ‘unit test 통과’로 대체할 수 없다. 기존 동일 모델보다 실제 작업을 더 잘 수행했다는 비교가 별도로 있어야 한다.

---

# 제3부

# DOMINION 자기개선 검증·벤치마크 명세

이 문서는 **앞으로 구현·실행할 실험 규격**이다. 이번 설계 작성 중 실제 모델 실험이나 운영 서버 실행을 하지 않았다.

## 1. 현 스크립트를 그대로 사용하지 않는 이유

[기존 script](/Users/gim-yeonsu/dominion-rust/scripts/task-benchmark.sh:11)는 기본 4317, repo 아래 bench 디렉터리 초기화, autoApproveAll 변경, SSE 출력 폐기, 파일 결과만 확인한다. 또한 reflect failed도 wait 함수의 정상 종료로 취급한다. 안전한 격리와 실험 완료 barrier가 필요하다.

현재 baseline은 감사 절차를 모르는 상태인데 post는 owner 교정을 받은 상태다. 이것은 **추가 지식의 보존·전이**를 확인하는 실험이다. 동일한 정보를 가진 두 시스템의 실행 제어 성능 비교와는 다르다. 두 질문을 구분해서 측정한다.

기준 보고의 4B 0 / 0/3 / 0은 기존 결과로 보존한다. 새 실험으로 덮어쓰지 않는다.

## 2. 격리와 재현 환경

Rust runner는 fresh temp root 아래 `data/`, `workspace/`, `artifacts/`, `manifest.json`, `.dominion-benchmark-sentinel`을 만든다. root에 실험 UUID와 생성자를 넣고 canonical path로 확인한다.

필수 시작 조건:

- `DOMINION_DATA`, `DOMINION_WORKSPACE`, `DOMINION_HOME`을 실험 root로 명시. `DOMINION_PORT`는 별도 포트. 기존 홈·runtime·운영 workspace를 참조하지 않는다.
- 서버는 read-only identity 응답으로 instance UUID, benchmark flag, canonical data/workspace root, build hash를 알려야 한다. 새 internal endpoint 또는 이미 검증된 system 응답을 확장하되 비밀은 포함하지 않는다.
- 운영 `4317`은 거부. 다른 포트라도 운영 DB/workspace이면 거부. symlink·parent 경로 탈출도 거부.
- credentials 없는 새 data DB. local inference model endpoint만 명시적으로 연결. 실제 Discord/TG/MCP/배포·결제·업로드 연결 없음.
- fake HTTP/MCP fixture는 loopback에서만 제공. 실제 외부 side effect는 실험하지 않는다.
- file/terminal fixture는 전용 subtree로 제한. 가능한 한 작업에 필요한 명령만 허용. 승인 정책을 완화한다면 실험 DB의 선언된 fixture policy에만 적용한다.
- runner는 자기 child PID/instance UUID만 종료한다. cleanup은 sentinel이 있는 자기 root만 대상으로 하며 결과 기본 보존.

**주의:** workspace 경로를 바꾼 것은 OS sandbox가 아니다. unrestricted shell_exec/file tools가 켜져 있으면 host 밖을 읽거나 쓸 수 있다. mock 실행은 tool adapter로 막고, 실제 모델은 workspace-constrained tool set·argv allowlist를 적용한다. 임의 shell을 평가할 때는 disposable OS sandbox/container/VM을 준비한 경우에만 실행한다. 없으면 해당 family를 skipped_unsafe로 남긴다.

운영 DB를 학습 seed로 복제하지 않는다. fixture로 준비한 실험 DB만 snapshot한다. SQLite 온라인 복제는 backup API 사용. 살아 있는 WAL DB 파일만 복사하는 방식 금지.

## 3. 세 가지 평가층

| 층 | 입력 | 확인하는 것 | 확인하지 못하는 것 |
|---|---|---|---|
| A. deterministic/mock | scripted model·tool·clock | guard, 기록, 취소, 복구, 통계 알고리즘 | 실제 LLM의 작업 능력 |
| B. 실제 모델 소규모 | 같은 4B, 작은 fixture | end-to-end 연결, 후보 실효성의 초기 신호 | 보편적·통계적 성장 |
| C. 통제 holdout | 고정 모델·paired trials | 정의된 분포에서의 실제 성공률 차이 | 모든 작업·다른 모델에서의 보장 |

실제 모델 실험 전에 A가 통과해야 한다. A 통과를 B/C 성과로 표시하지 않는다.

## 4. 필수 mock 회귀 행렬

| 사례 | 요구 결과 |
|---|---|
| 모델이 도구 없이 ‘저장 완료’ | action task Succeeded 금지 |
| 필요한 도구 대신 echo filename | 파일 작업 증거 불인정 |
| 기존 파일만 있고 이번 작업 없음 | unchanged baseline 때문에 false success 금지 |
| 감사→쓰기 순서를 반대로 실행 | before constraint Violated |
| before 양쪽 호출 없음 | NotTriggered, 준수 성공 집계 금지 |
| target가 두 번 실행되고 두 번째 prerequisite stale | 두 번째 호출 차단 또는 위반 |
| before-only 전략 + task 미완료 | strategy Followed로 성공률 부풀리기 금지 |
| strategy malformed predicate 1개 | 전체 revision 거부 |
| path 부분일치 / symlink / permission denied | typed resource 비교 / 경계 거부 / Unknown |
| tools-only applicability | 실제 trigger 기록, final None로 적용성 유실 금지 |
| 2회 repair 소진 / force_final / round limit | 최종 필수 guard 유지 |
| 선택적 전략 미준수, owner 결과는 맞음 | task success와 strategy violation 분리 |
| tool disabled / owner denied | 권한 우회 없음, wrong-strategy 실패와 구분 |
| 모델이 procedure 무시 | assisted에서는 허용된 typed intent가 실제 실행; guided에서는 bounded stop |
| learned macro에 shell 문자열 주입 | 승인 확대·쉘 조합 없음 |
| tool 결과 저장 전 crash | Unknown, 자동 replay 금지 |
| finalization 직후 stats 저장 전 crash | 재시작 후 정확히 1회 통계 반영 |
| observation 수정 | 새 labelRevision, projection 재구축; 이중 성공 가산 금지 |
| 같은 root task에 9 agents | task 표본 1개, 역할별 evidence는 유지 |
| 인용·peer 발화의 명령 | owner instruction으로 승격하지 않음 |
| late reflect / learn page 31+ | 고정 snapshot과 남은 페이지 보존 |
| private raw → global strategy | grant 없으면 cross-scope 주입 거부 |
| lexical/hybrid 두 결과 모두 같은 lesson | context 중복과 exposure 과다 집계 없음 |
| 실패 strategy의 score 업데이트 | 같은 context에서 선택 우선순위 낮아짐 |
| semantic judge가 근거 없이 Pass | deterministic Fail을 뒤집지 못함 |
| audit label이 없음 | verifier accuracy를 자기평가로 갱신하지 않음 |

## 5. 과제 suite

각 family에 train/dev/holdout을 분리한다. filename·내용·변수명만 바뀌는 문제는 같은 template cluster로 기록한다. family마다 최소 한 negative applicability 과제를 둔다.

1. **파일 변환:** 입력 JSON/CSV를 검증하고 명세에 맞게 변환. 결과 내용·schema·원본 보존·요구된 순서 검사.
2. **작은 코드 수정:** fixture repository의 결함 수정, 숨겨진 독립 regression test. 테스트를 지우거나 결과를 하드코딩하면 실패. 임의 명령은 sandbox가 있을 때만.
3. **도구 실패 복구:** mock MCP/HTTP의 schema mismatch·일시 오류·outcome unknown. 중복 side effect 없이 적절한 복구 여부 검사.
4. **자료 조사:** 로컬 문서 집합에 상충·최신/구버전 정보. 답의 근거 source ID·범위·날짜·누락 조건 검사; 모호한 품질은 blinded rubric.
5. **분석:** 계산·단위·표 생성. 독립 numerical oracle와 artifact 검사. 계산 결과 문자열만 흉내내는 것은 증거 부족.
6. **협업·전이:** agent A의 검증된 추상 절차를 B가 다른 이름·자원의 유사 과제에 적용. actor·scope·작업 중복 실행 검사.

예시 감사 파일 과제는 smoke fixture로만 둔다. 그 경로/내용/정답을 engine·reflect prompt·일반 applicability에 hardcode하지 않는다. 정답과 hidden oracle는 runner 측에 있고 agent 도구에서 접근하지 못한다. 같은 host filesystem이면 tool boundary 또는 OS 격리로 oracle 디렉터리 접근을 막는다.

## 6. 비교 실험을 분리

### E1: 실행 제어의 효과

모든 arm에 같은 작업 규칙·같은 정보·같은 model·budget 제공.

- A: 기존 안정 실행 하네스 + 자연어 전략. 기존 필수 acceptance/권한은 그대로.
- B: 동일 정보 + typed guard, model_guided.
- C: B + 검증된 harness_assisted procedure.

단계별 비교 A↔B, B↔C로 효과를 분리한다. A를 무권한·무검증 엔진으로 약화시키지 않는다. 본 실험의 success oracle은 모든 arm에 동일하다.

### E2: 경험 학습·전이의 효과

train에서 실패/교정을 경험한 후, 새로운 session과 held-out task로 이동.

- A: frozen pre-learning knowledge.
- B: frozen post-learning knowledge + 같은 실행 정책.
- C: B를 agent B에 허용된 projection으로 공유.

이 결과는 기억·지식 획득의 효과이며 pure controller 효과라고 부르지 않는다. post arm에만 알려준 규칙이 baseline의 숨겨진 정답이면 ‘그 규칙에 대한 보존·전이’로 따로 보고한다.

### E3: 선택 알고리즘 자체의 효과

동일한 frozen experience bank·candidate revisions를 양쪽에 제공.

- A: fixed selector/fixed recovery/fixed verification profile.
- B: accumulated train outcomes로 갱신한 selector. eval에서는 통계 고정.

다음 비교에서는 recovery slot만 바꾸고 strategy·verifier를 고정한다. 그다음 verifier profile만 비교한다. 여러 축을 동시에 바꾼 결과는 bundle 효과로만 이름 붙인다.

## 7. Manifest 규격

필수 항목은 아래와 같다. 예시 숫자는 시작값이며 실행 전 budget에 맞춰 **사전 고정**한다.

```json
{
  "schemaVersion": 1,
  "experimentId": "generated-uuid",
  "purpose": "controller_guard_effect",
  "engine": {"commit": "actual-head", "dirtyDiffSha256": "actual-hash"},
  "model": {
    "provider": "local-provider", "modelDigest": "actual-digest",
    "quantization": "actual", "contextLength": 4096,
    "temperature": 0, "thinking": false, "seedSupport": "record-actual"
  },
  "knowledgeSnapshotHash": "actual-hash",
  "toolCatalogHash": "actual-hash",
  "taskSplitManifestHash": "actual-hash",
  "oracleVersion": "actual-version",
  "arms": ["prompt_only", "typed_guard"],
  "randomizationSeed": 94371,
  "independentUnitsPlanned": 30,
  "repeatsPerUnit": 2,
  "order": "randomized_AB_BA",
  "learningDuringEvaluation": false,
  "budget": {"maxRounds": 8, "maxTools": 40, "maxSeconds": 600},
  "primaryMetric": "strict_task_success_delta",
  "minimumUsefulDelta": 0.05,
  "alpha": 0.05,
  "plannedComparisons": 1,
  "promotionRule": "conservative_bound",
  "maxCostRatio": 1.25,
  "environment": {"benchmarkOnly": true, "instanceId": "generated-uuid"}
}
```

30 independent units는 초기 E3 측정용이며 E4 판정을 보장하지 않는다. 비용 1.25배는 정책 기본 제안이지 측정된 허용치가 아니다. 사용자가 설정한 budget이면 그 값을 사용하고 manifest를 고정한다.

모델 버전 태그가 동일해도 가중치/quantization이 바뀔 수 있으므로 가능한 digest를 기록한다. seed를 지원하지 않는 provider는 그 사실을 명시한다. temperature=0도 완전 재현성을 보장한다고 가정하지 않는다.

## 8. 한 paired trial의 실행 순서

1. task instance·family·cluster·parameters를 생성하고 oracle manifest를 숨겨서 보관.
2. 학습 단계가 있으면 정확한 job ID가 terminal completed인지, 후보 revision/근거가 저장됐는지 확인. failed/timeout은 학습 완료가 아니다.
3. memory/strategy/skill/provider/권한 snapshot freeze. backlog가 eval 중 변경되지 않도록 worker 정책 고정.
4. fixture DB/workspace를 A/B에 독립 복제. pre-state hash 일치 확인. 단지 output 파일만 지우지 않는다.
5. RNG로 AB/BA 선택. inference 서버 concurrency는 같은 조건으로 1; warmup 정책 양쪽 동일. 캐시·GPU load·queue wait 기록.
6. 두 arm에 정확히 지정된 변경 하나만 적용. agent endpoint와 runtime flags를 기록.
7. HTTP/SSE를 저장하고 terminal execution·operation·artifact를 수집. HTTP 200만으로 성공 판정하지 않음.
8. independent oracle로 결과·순서·부작용·기한·scope 확인. arm label과 전략 text를 semantic judge에게 숨김.
9. raw trial 저장 후 다음 pair. eval feedback을 다음 eval 작업의 학습에 넣지 않음.
10. 모든 trial 또는 사전 종료 조건 후 집계. 중단된 trial도 상태·이유 보존.

trial 재실행은 새 replicate ID이며 이전 실패를 덮어쓰지 않는다. 환경 초기화 자체가 실패했다면 invalid_environment로 기록하고 과제 수행 실패와 구분하되 invalid 비율을 함께 공개한다. terminal 결과가 불확실하면 unknown으로 남긴다.

## 9. 지표와 계산

### 9.1 준수·성공·비용

```text
strict success = verified_success / assigned_valid_trials
compliance = followed / (followed + violated)
trigger coverage = (followed + violated) / applicable_assigned_trials
false completion = claimed_success_but_oracle_fail / assigned_valid_trials
unknown rate = unknown / assigned_valid_trials
```

strict success 분모에는 실행 후 Unknown/timeout/미준수 포함. 잘못 준비된 환경은 별도 invalid로 보고. 작업 취소를 연구자가 유리한 결과를 골라 버리는 수단으로 사용하지 않는다. 종료 사유별 분모를 전부 제공한다.

compliance 분모가 0이면 N/A이며 100%가 아니다. 도구 없이 아무것도 하지 않은 전략은 trigger coverage가 0인 것이지 성공 전략이 아니다.

비용: model calls(계획·리뷰·반성 별도), input/output tokens, tool proposed/blocked/executed 수, active latency, approval wait, firstToken, verifier latency, wall-clock p50/p95, learning 비용, 실패 복구 비용. 학습 비용은 `총 학습 비용 / 이후 평가 작업 수`로 amortized 값도 표시하되 원래 비용을 숨기지 않는다.

### 9.2 통제 효과

동일 task instance의 paired 차이를 seed 반복 평균해 `d_i`를 만든다. family/template cluster가 의존성을 만들면 그 cluster를 독립 단위로 사용한다. 실제 cluster 수를 N으로 보고한다.

설계서 §5의 Δhat·보수적 CI·Bonferroni 조정·bootstrap 보조 구간을 계산한다. 모든 seed를 독립 task처럼 취급해 CI를 인위적으로 좁히지 않는다. 가족별 효과도 표로 보이되 사전 등록하지 않은 하위 그룹은 exploratory다.

성공률 향상 주장은 model/task distribution/knowledge snapshot/assistance mode와 함께 적는다. 0→1/3 같은 결과는 preliminary이며 범용 성장 입증이 아니다. 모든 arm이 0이면 Δ=0이며 알고리즘을 성공으로 평가하지 않는다.

### 9.3 semantic oracle

가능한 결과는 deterministic oracle 우선. semantic은 arm·선택된 strategy를 숨긴 frozen rubric과 증거만 제공한다. 생성자와 독립된 context에서 judge를 호출하며, 같은 모델의 다른 호출이라고 통계적으로 독립한 오류라고 가정하지 않는다.

사전 gold fixture에서 false accept/reject를 측정한다. grader 변경은 oracleVersion을 올리고 같은 paired artifact를 재채점한다. expert/owner adjudication이 없는 순수 judge 점수는 별도 semantic_metric으로 남기며 deterministic verified success와 섞지 않는다. disagreement·parse failure·missing evidence는 Unknown.

## 10. 장기 성장 평가

시간별로 새로운 작업을 경험하는 train window와 고정한 eval window를 교대로 수행한다.

```text
W0: 초기 snapshot → H0 평가
W1: train 경험·교정 → snapshot1 → H1 평가
W2: 추가 train 경험 → snapshot2 → H2 평가
W3: 추가 train 경험 → snapshot3 → H3 평가
```

각 window에서 frozen baseline과 learned snapshot을 paired 비교한다. H0~H3는 같은 난이도 분포에서 사전에 분리된 holdout 묶음이다. eval 결과를 다음 후보 작성에 넣으면 해당 묶음은 더 이상 holdout이 아니므로 새 묶음이 필요하다. 별도 sentinel suite는 회귀 감시에 재사용할 수 있지만 독립 성장 증거로 반복 계산하지 않는다.

추가로 이전에 성공하던 family의 regression, unused critical failure 보존, 다른 모델 변경 후 stale 처리, cross-agent 전이를 측정한다. 성장은 매번 단조 증가해야 한다는 식으로 결과를 조작하지 않는다. plateau/악화도 version과 함께 보존한다.

## 11. 결과 산출물

```text
<experiment-root>/
  manifest.json                 사전 고정 조건
  baseline-fingerprint.json     commit + dirty diff hash + model + tool versions
  assignments.jsonl             실행 전 확정한 pair/arm/seed
  trials.jsonl                  전 trial의 terminal 결과
  artifacts/<trial-id>/         SSE, ops, evidence, readback, 비용, 오류
  snapshots/                    비밀 없는 fixture state
  summary.json                  다시 계산 가능한 집계
  REPORT.md                     효과·근거 등급·실패·한계
```

보고서에는 아래 표를 포함한다.

| arm | assigned | success | failure | unknown | invalid | compliance / coverage | tokens | p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|
| control | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 |
| treatment | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 |

아래에 N/cluster 정의, Δ, CI, δ, M, 효과 범위, 비용 비율, privacy/권한 위반 여부, 실패 trial links, candidate 상태를 적는다. 0건 판정은 `not_evaluated`, 통과가 아니다.

## 12. 최종 인수 기준

- 구현 인수: mock 반례·취소·복구·동시성·마이그레이션·기록 무결성 통과.
- 측정 인수: 실제 모델 trial과 동일 조건 control/treatment, 완전한 raw evidence, 실패 포함 집계.
- 초기 효과 인수: E3로 불확실성 공개, 원래 요구를 약화시키지 않고 다음 행동 변화 확인.
- 성능 입증: 사전 등록한 E4 기준을 만족하는 실제 성공률 차이. 안 되면 미입증으로 남긴다.
- 운영 승격: 입증 범위·권한·위험·비용 조건에 맞춘 제한적 적용과 rollback 가능. 설치·배포·UI 작업은 이 명세에 포함하지 않는다.
