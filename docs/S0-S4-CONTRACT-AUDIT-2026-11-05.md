# S0–S4 구현 계약 대조표

작성일: 2026-11-05  
기준 커밋: `a779986` (`codex/native-memory-harness`)  
계약 문서:
- `docs/SELF-IMPROVEMENT-TASKS-2026-11-05.md` S0–S4
- `docs/SELF-IMPROVEMENT-DESIGN-2026-11-05.md` §2, §3, §7

판정 기준: `구현` / `부분` / `미구현`.  
`부분`은 요구사항의 핵심 경로가 동작하지만 명세의 필수 조건을 모두 충족하지 못한 경우다.  
테스트 수는 전체 스위트(158 unit + 22 integration)와 별도로, 해당 요구사항을 직접 검증하는 테스트만 적는다.

**중요:** 이전 보고에서 “S0/S1 완료”라고 표현한 것은 계약 기준으로 과장이었다. 아래 대조표가 정본이며, S0/S1은 아직 완료가 아니다.

---

## S0. 기준선과 실험 보호

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | HEAD·원격 branch·tracked diff·untracked procedure hash 저장, reset/stash 금지 | **구현** | `scripts/benchmark-baseline.sh`가 HEAD, remoteHead, trackedDiffSha256, statusSha256, untracked 파일별 sha256을 저장. reset/stash 코드 없음 | — |
| 2 | default 4317 제거, 명시 target·sentinel·서버 identity 확인, 실패 시 nonzero | **구현** | 스크립트가 `DOMINION_BASE`·`BENCH_ROOT`·sentinel 필수, `/api/benchmark/identity`로 benchmark flag·instanceId·dataRoot·workspace 확인 | — |
| 3 | repo/bench `rm -rf` 및 전역 autoApproveAll 제거, fixture 도구만 승인 | **구현** | `BENCH_DIR="$BENCH_ROOT/workspace/bench"`; `reset_bench`는 격리 workspace만 삭제. `autoApproveAll:false` + `autoApprove:[workspace_write,…]` | — |
| 4 | 운영 data/workspace 경로 거부, 포트 검사만으로 판정 금지 | **구현** | identity의 canonical dataRoot/workspace를 `BENCH_ROOT`와 비교. symlink workspace·저장소 루트 거부 | — |

**S0 테스트 충족도**

- 있음: 주소 미지정, 4317, sentinel 불일치, symlink workspace, 비벤치마크 identity 거부 (수동 실행 확인)
- 있음: `benchmark_identity_reports_the_isolated_roots` (E2E)
- 있음: `scripts/benchmark-baseline.sh`가 untracked 파일 hash까지 기록

**S0 결론:** 구현 완료. 4개 요구사항과 반례 테스트 충족.

---

## S1. 타입·revision·transaction 기반

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | TaskContract/StrategyRevision/Decision/Observation/EvidenceRef/Outcome 타입 + strict serde | **미구현** | 없음. `src/engine/adaptation/types.rs`, `episode.rs`, `verification.rs` 없음 | 타입·schemaVersion·unknown field 거부·길이 제한 전부 |
| 2 | 기존 records/messages 유지, additive schema·index·finalization/update marker | **부분** | `execution_finalization` record kind 추가. 기존 records/messages 유지 | SQL schema version, index, `adaptation_updates` marker 없음 |
| 3 | immutable revision canonical hash, terminal finalization transaction, idempotent observation API | **부분** | `execution_finalization` + `put_if_absent`, `appliedExecutionIds`, `recover_finalizations` | immutable revision/hash 없음. execution·marker·stats가 하나의 SQLite transaction이 아님. observation API 없음 |
| 4 | `Store::all()` 오류를 빈 데이터로 처리하지 않고 Result query 사용 | **미구현** | 새 학습 경로도 `Store::all()` 사용 | Result 기반 query·오류 전파 없음 |
| 5 | legacy counter 보존, 새 점수에 미합산, 기존 validated에 E4 없음 표시 | **미구현** | legacy `uses/successes`를 그대로 사용 | E0–E4 등급, legacy counter 분리 없음 |

**S1 테스트 충족도**

- 있음: `the_same_execution_is_counted_once`
- 없음: 같은 finalization marker 2회, 같은 ID 다른 payload, 두 worker 동시 종료, stats 반영 전 crash 복구, migration 2회, 깨진 JSON

**S1 결론:** 대부분 미구현. 원자적 종료도 transaction이 아니라 marker + 멱등 카운터 수준.

---

## S2. 절차 compiler와 검증기

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | normalize의 조건 필터링 → 전체 schema 오류 거부 | **구현** | `procedure::normalize`가 하나라도 invalid면 `type:"invalid"` 반환. 테스트 `one_invalid_condition_rejects_the_whole_revision` | — |
| 2 | typed arg/resource 비교, canonical workspace, symlink·크기, operation seq | **부분** | `argument_matches` 정확 경로 일치, `read_workspace_file` symlink/2MiB/canonical 검사. 테스트 `exact_paths_do_not_match_suffixes_or_commands`, `symlink_and_oversized_files_are_rejected` | 단조 `opSeq` 없음. `startedAt` 정렬 사용. typed resource adapter 없음 |
| 3 | DAG cycle·자기 의존·미정의 binding·미지원 도구·모순 조건 거부 | **미구현** | 없음 | DAG/binding/tool catalog/모순 검사 전부 |
| 4 | Before를 각 target event에서 검사, per_target/task lifetime·invalidation 정의 | **부분** | `missing_requirement`는 현재 호출마다 검사. `evaluate`는 첫 target/prerequisite 위치만 비교 | target 반복, invalidation, per_target/task lifetime 없음 |
| 5 | NotTriggered/Unknown/Violated/Followed 구분 | **부분** | `not_triggered`, `followed`, `violated`, `unverified`/`invalid` 구현. 테스트 `empty_execution_is_not_triggered_not_compliant` | `Unknown`(읽기 오류·권한 오류) 없음. 읽기 실패는 false로 처리 |
| 6 | goals 검사 구현을 공통 모듈로 이동, 기존 wrapper/successCriteria 호환 | **미구현** | `goals::check_acceptance`가 그대로 존재 | `verification.rs` 공통 모듈 이동 없음 |

**S2 테스트 충족도**

- 있음: 파일명 부분일치, `echo filename`, 잘못된 condition 하나, symlink, 순서 역전, NotTriggered
- 없음: 이미 존재하는 결과, empty contains, binary 존재, permission error, target 반복, timestamp 동률

**S2 결론:** 1번만 완전 구현. compiler라기보다 런타임 조건 검사기 수준.

---

## S3. 일반 채팅 RunGuard 연결

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | owner 원문·sourceMessageId로 task 생성, context selection/revision snapshot 1회 고정 | **부분** | `strategyCandidates`와 `appliedImprovementIds`를 execution에 저장 | TaskEpisode/TaskContract 없음. context와 revision을 하나의 SelectionSnapshot으로 고정하지 않음 |
| 2 | 모델/하네스 공통 dispatch·approval·cancellation·operation 저장 | **부분** | 모델 도구 경로는 공통. approval/cancel/operation 저장 유지 | harness-assisted 경로 없음. 공통 dispatch 추출 없음 |
| 3 | 도구 전 guard, 도구 후 evidence, 최종 single finish | **부분** | pre-call `missing_requirement`, finalize `strategyCompliance` | `finish` 단일 경로가 아님. early return·MAX_ROUNDS·cancel 경로가 별도 |
| 4 | force_final·acceptance_passed·repair 소진·MAX_ROUNDS·tool disabled 모두 필수 절차 검사 | **부분** | 최종 답변에서 standing 위반 시 Err. `force_final`도 검사 | MAX_ROUNDS 초과·조기 Err·tool disabled 경로에서 최종 procedure 판정 없음 |
| 5 | repair 총 2회, 공통 BudgetLedger, 승인 거절·취소를 실패 학습에서 분리 | **부분** | `procedure_repairs < 2` | 공통 BudgetLedger 없음. 기존 8/40/600 예산과 별도. 취소/거절 학습 분리 미검증 |
| 6 | 결정적 단순 작업은 추가 planner 호출 없이 실행, 불확실할 때만 plan/review | **미구현** | `action_plan`이 action 요청에 대해 LLM 계획 호출 | 조건부 plan/review 정책 없음 |
| 7 | status와 별도 taskOutcome/compliance/exposure 저장, 성공 경험은 검증된 outcome만 | **부분** | `strategyCompliance`, `appliedImprovementIds` 저장 | taskOutcome/effect 없음. 성공 경험은 acceptance 통과 시 생성되어 새 기준과 불일치 |

**S3 테스트 충족도**

- 있음: mock E2E `procedure_violation_blocks_the_target_and_repairs_with_the_prerequisite`
- 없음: force_final 위반, 2회 repair 후 실패, 선택적 전략 실패+작업 성공, tools disabled, cancel 중 procedure, MAX_ROUNDS

**S3 결론:** 부분. 완료 우회를 일부 닫았지만 single finish/BudgetLedger/taskOutcome이 없다.

---

## S4. 약한 모델용 harness-assisted

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | ready DAG node 선택, verified binding, next intent | **미구현** | 없음 | 전부 |
| 2 | model_guided/harness_assisted revision별 선택, candidate 자동 실행 금지 | **미구현** | `authority` 필드만 있음 | assistance mode, 후보 차단 없음 |
| 3 | literal/owner binding/앞 node JSON pointer, 길이·type·resource 제약 | **미구현** | 없음 | 전부 |
| 4 | argv 도구는 existing terminal_exec, shell 문자열 조합 금지 | **미구현** | 없음 | 전부 |
| 5 | node 실행도 기존 승인·취소·budget·side effect uncertainty, source=procedure | **미구현** | 없음 | 전부 |

**S4 결론:** 착수 전. S3의 single finish/공통 dispatch가 먼저 필요하다.

---

## 요약

| 단계 | 구현 | 부분 | 미구현 |
|---|---:|---:|---:|
| S0 | 4 | 0 | 0 |
| S1 | 0 | 2 | 3 |
| S2 | 1 | 3 | 2 |
| S3 | 0 | 6 | 1 |
| S4 | 0 | 0 | 5 |

**현재 완료된 단계는 없다.**  
S0는 실험 보호의 기본만, S1은 멱등 종료의 일부만, S2는 조건 검증의 일부만, S3는 완료 우회의 일부만 닫았다.

## 다음 구현 순서 (계약 기준)

1. **S0 잔여**: benchmark identity endpoint + script handshake, symlink workspace 거부 테스트, untracked hash 명시.
2. **S1**: TaskContract/StrategyRevision/Observation/EvidenceRef 타입, strict serde, canonical revision hash, SQLite transaction finalization, Result query.
3. **S2**: DAG/binding/tool catalog/motivation checks, `Unknown`, per-target/invalidation, goals 공통 모듈 이동.
4. **S3**: single finish, 공통 BudgetLedger, taskOutcome/effect, 조건부 planner.
5. **S4**: harness-assisted는 S3 완료 후.
6. **S7**: 격리 runner와 실제 4B paired experiment.
7. **S10**: adaptive selector는 S7 결과 위에서.

각 단계는 위 체크리스트의 요구사항 ID를 커밋 메시지와 테스트 이름에 연결한다.
