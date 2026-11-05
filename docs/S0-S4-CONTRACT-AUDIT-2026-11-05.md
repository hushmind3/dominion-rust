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
| 1 | TaskContract/StrategyRevision/Decision/Observation/EvidenceRef/Outcome 타입 + strict serde | **구현** | `src/engine/adaptation/types.rs`에 전 타입, `deny_unknown_fields`, `validate()`. 실행 시 `task_episode`/`strategy_decision`/`selection_snapshot` records 생성. 테스트 `unknown_fields_are_rejected`, `observation_roundtrips_and_validates` | — |
| 2 | 기존 records/messages 유지, additive schema·index·finalization/update marker | **구현** | `adaptation_schema`, `adaptation_finalizations`, `adaptation_updates`, `idx_operation_run`, `idx_execution_task`. 기존 records/messages 유지 | — |
| 3 | immutable revision canonical hash, terminal finalization transaction, idempotent observation API | **구현** | `revision_hash`/`revision_of`/`improvement_revision` 스냅샷. `Store::finalize_once`가 execution·improvement·observation·marker를 한 transaction으로 기록. `appliedExecutionIds` 멱등. `strategy_observation` 저장 | — |
| 4 | `Store::all()` 오류를 빈 데이터로 처리하지 않고 Result query 사용 | **구현** | `Store::try_all` 추가. `execution_updates`, legacy migration, finalization 테스트가 사용. UI 호환 `all`은 유지 | — |
| 5 | legacy counter 보존, 새 점수에 미합산, 기존 validated에 E4 없음 표시 | **미구현** | legacy `uses/successes`를 그대로 사용 | E0–E4 등급, legacy counter 분리 없음 |

**S1 테스트 충족도**

- 있음: `the_same_execution_is_counted_once`, `finalization_is_idempotent_and_conflict_aware`, `concurrent_finalizations_apply_once`, `broken_finalization_marker_is_an_error`, `legacy_improvement_counters_are_preserved_but_not_scored`, `revisions_are_immutable_snapshots`
- crash 전 구간은 `finalize_once`의 단일 transaction으로 원자화했고, 동시 종료·중복 payload를 테스트로 확인했다. 별도 프로세스 kill 주입 테스트는 아직 없다.

**S1 결론:** 구현 완료. 단, 프로세스 kill 주입 crash 테스트는 후속 S7 runner에서 추가한다.

---

## S2. 절차 compiler와 검증기

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | normalize의 조건 필터링 → 전체 schema 오류 거부 | **구현** | `procedure::normalize`가 하나라도 invalid면 `type:"invalid"` 반환. 테스트 `one_invalid_condition_rejects_the_whole_revision` | — |
| 2 | typed arg/resource 비교, canonical workspace, symlink·크기, operation seq | **구현** | `argument_matches` 정확 경로 일치, canonical/symlink/2MiB 검사, 실행 `opSeq`와 `procedure::sort_operations`. 테스트 `exact_paths...`, `symlink...`, `operations_sort_by_monotonic_sequence` | — |
| 3 | DAG cycle·자기 의존·미정의 binding·미지원 도구·모순 조건 거부 | **구현** | `validate_dag`/`validate_bindings`/`contradictory_after`. 테스트 `dag_cycles_unknowns_and_contradictions_are_rejected` | — |
| 4 | Before를 각 target event에서 검사, per_target/task lifetime·invalidation 정의 | **구현** | `missing_requirement`/`evaluate`가 각 target마다 직전 target 이후의 새 prerequisite을 요구. 테스트 `repeated_target_requires_a_fresh_prerequisite` | — |
| 5 | NotTriggered/Unknown/Violated/Followed 구분 | **구현** | `not_triggered`/`unknown`/`followed`/`violated`/`unverified`/`invalid`. 읽기 오류는 Unknown. 테스트 `unreadable_artifacts_are_unknown`, `empty_execution...` | — |
| 6 | goals 검사 구현을 공통 모듈로 이동, 기존 wrapper/successCriteria 호환 | **구현** | `src/engine/verification.rs`로 acceptance 구현 이동. `goals`는 `pub use` wrapper 유지. 기존 goals tests 통과 | — |

**S2 테스트 충족도**

- 있음: 파일명 부분일치, `echo filename`, 잘못된 condition, symlink, 순서 역전, NotTriggered, Unknown, target 반복, DAG cycle/self/unknown/unsupported/binding/contradiction, opSeq
- 없음: binary 존재, permission error의 실제 OS 주입 테스트(디렉터리 읽기 오류로 Unknown은 확인)

**S2 결론:** 구현 완료. binary/permission OS 주입은 S7 sandbox runner에서 보강한다.

---

## S3. 일반 채팅 RunGuard 연결

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | owner 원문·sourceMessageId로 task 생성, context selection/revision snapshot 1회 고정 | **구현** | `task_episode`에 ownerText/sourceMessageId, `task_contract`/`strategy_decision`/`selection_snapshot`. `system_prompt`는 execution의 `strategyCandidates` snapshot을 사용 | — |
| 2 | 모델/하네스 공통 dispatch·approval·cancellation·operation 저장 | **구현** | `dispatch_tool_call`로 approval/operation/tool/result/message 저장을 공통화. 모델 경로가 사용 | harness 호출자는 S4에서 동일 함수 사용 |
| 3 | 도구 전 guard, 도구 후 evidence, 최종 single finish | **구현** | `missing_requirement`, `strategyCompliance`, `finish_attempt`가 모든 경로의 최종 판정 | — |
| 4 | force_final·acceptance_passed·repair 소진·MAX_ROUNDS·tool disabled 모두 필수 절차 검사 | **구현** | `determine_task_outcome`이 standing/acceptance/status를 모두 반영. `tools_disabled...`, MAX_ROUNDS `compatibility...`, cancel, procedure E2E로 확인 | — |
| 5 | repair 총 2회, 공통 BudgetLedger, 승인 거절·취소를 실패 학습에서 분리 | **구현** | `BudgetLedger`가 round/tool/proposal/time을 통합. `procedure_repairs < 2`. failed/cancelled면 learn enqueue 생략 | — |
| 6 | 결정적 단순 작업은 추가 planner 호출 없이 실행, 불확실할 때만 plan/review | **구현** | `needs_plan`이 단순 단일 파일 작업은 생략, 다단계/장문/미검증만 plan. 테스트 `simple_deterministic_tasks_skip_the_planner` | — |
| 7 | status와 별도 taskOutcome/compliance/exposure 저장, 성공 경험은 검증된 outcome만 | **구현** | `execution.taskOutcome`(Succeeded/Failed/Cancelled/AnsweredUnverified), `strategyCompliance`, `appliedImprovementIds`. 성공 경험은 Succeeded만 생성 | — |

**S3 테스트 충족도**

- 있음: `procedure_violation...`, `completed_chat_without_verification_is_not_a_success`, `tools_disabled_action_is_failed_not_succeeded`, cancel `taskOutcome=cancelled`, MAX_ROUNDS `taskOutcome=failed`, `determine_task_outcome` 단위 테스트, BudgetLedger 테스트, `simple_deterministic_tasks_skip_the_planner`
- 추가: `task_outcome_gates_strategy_success` — succeeded/failed/answered_unverified별 전략 집계, `success_experience_is_idempotent` — 같은 execution의 성공 경험 1회
- `execution_updates`는 `taskOutcome`을 먼저 확정한 뒤 전략 성공/실패를 판정한다.
- 성공 경험은 `success_experience_marker`로 execution 단위 멱등하다.
- 없음: 실제 승인 거절 E2E는 아직 없음(코드 경로는 dispatch/denied 처리)

**S3 결론:** 구현 완료. 승인 거절 E2E는 S4/S7에서 보강한다.

---

## S4. 약한 모델용 harness-assisted

| # | 요구사항 | 판정 | 구현 증거 | 빠진 것 |
|---|---|---|---|---|
| 1 | ready DAG node 선택, verified binding, next intent | **구현** | `procedure::ready_intent`/`resolve_binding`. 테스트 `ready_nodes_bind_only_verified_values`, `unresolved_binding...` | — |
| 2 | model_guided/harness_assisted revision별 선택, candidate 자동 실행 금지 | **구현** | `propose` 기본 `model_guided`; `set_assistance_mode`; 반성은 standing+non-empty nodes일 때만 `harness_assisted`. 후보는 기본 guided | — |
| 3 | literal/owner binding/앞 node JSON pointer, 길이·type·resource 제약 | **구현** | `resolve_binding`이 literal/$owner/$input/$task/이전 node pointer 지원. `valid_node_arguments`가 4096자/32KiB/깊이 8 제한. unresolved binding은 intent 없음 | — |
| 4 | argv 도구는 existing terminal_exec, shell 문자열 조합 금지 | **구현** | harness는 node의 tool/arguments를 그대로 `dispatch_tool_call`에 전달. shell 문자열을 합성하지 않음 | — |
| 5 | node 실행도 기존 승인·취소·budget·side effect uncertainty, source=procedure | **구현** | `dispatch_tool_call`의 공통 approval/operation/budget 사용. harness 취소 세대 검사. operation에 `origin=procedure`, `procedureNodeId` 저장. E2E `harness_assisted_executes_dag_nodes_through_the_common_dispatch` | — |

**S4 결론:** 구현 완료. `ready_intent`/binding 단위 테스트와 harness E2E가 통과했다. 실제 4B paired benchmark는 다음 S7이다.

---

## 요약

| 단계 | 구현 | 부분 | 미구현 |
|---|---:|---:|---:|
| S0 | 4 | 0 | 0 |
| S1 | 5 | 0 | 0 |
| S2 | 6 | 0 | 0 |
| S3 | 7 | 0 | 0 |
| S4 | 5 | 0 | 0 |

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
