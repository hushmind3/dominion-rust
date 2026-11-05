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
