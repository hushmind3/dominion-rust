# 설계 2.4 구현 기록 — 2026-11-06

상위 기준: `docs/DOMINION-HARNESS-MASTER-DESIGN.md` (설계 2.4, 2026-11-06)
이 문서는 2.4의 11절 구현 순서 중 실제로 연결된 범위만 기록한다.

## 1. 실행 루프 적응 (11절 #1) — 1단계

### 연결한 동작

`chat/runtime.rs`의 tool loop에 **관측 → 가설 → 개입** 기록을 추가했다.

- 도구 호출이 실패하면 `failed_calls`에 `tool+arguments` 키를 저장한다.
- 같은 키의 호출이 다시 오면 **실행하지 않고 차단**한다.
  - tool 결과에 `repeatedFailedCall=true`, `previousError`를 반환
  - 모델에 "다른 도구/인자/probe를 선택하라"는 system 지시를 넣는다.
- 차단/실패 시 `adaptation_loop::record_tool_failure`가 execution에
  다음을 기록한다.
  - observation: kind=tool_failure, tool, argsDigest, expected=success, actual=error
  - hypothesis: "the same call will repeat the same failure", evidenceIds
  - intervention: change="different tool, arguments, or approach"
- 성공한 호출은 `failed_calls`에서 제거한다.
- adaptations는 실행당 최대 20개로 제한한다.

실제 실행 경로:

```
model tool_calls
  → procedure violation check
  → failed_calls check (block unchanged repeat)
  → dispatch_tool_call (common gateway)
  → ok? clear failure : record adaptation + system instruction
  → 다음 모델 판단
```

### 검증

E2E `a_failed_call_is_not_repeated_and_the_model_adapts`:

1. mock 모델이 `shell_exec exit 7` 호출
2. 같은 호출을 한 번 더 제안 → **실행되지 않음**
3. 모델이 `workspace_write adapted.txt=OK`로 변경 → 실행 성공
4. 결과:
   - `adapted.txt == "OK"`
   - execution.adaptations에 shell_exec 실패 observation 기록
   - operations에서 `shell_exec` **1회**, `workspace_write` **1회**

전체: `262 unit + 48 E2E + 16 benchmark` 통과.

## 2. 아직 아닌 것 (11절)

- #1의 typed Program(Observe/CallTool/Branch/Loop/Parallel), program counter,
  checkpoint 복원은 아직이다. 현재는 실패 반복 차단 + 모델 재판단 수준이다.
- #2 경험 규칙 추출·조건 predicate·반례 갱신은 기존 experience/learning과
  분리된 구현이 아직 없다.
- #3 typed Program 생성·해석·반례 수정 없음.
- #4 scope별 active revision 적용은 planning/recovery 슬롯 수준이다.
- #5 개선기(SearchEpisode/meta) 비교는 아직 없다.
- 비용/예상 진전/원인 구별 점수는 아직 계산하지 않는다.

## 3. 다음 구현 순서

1. Program IR(`Observe/CallTool/Branch/Loop`)과 checkpoint(frameStack,
   resolvedInputs, completed/pending operationIds) 최소 구현.
2. 실패 관측에서 branch predicate(exists/equals/compare)로 다음 경로를
   결정하는 실제 실행.
3. Experience에서 조건 predicate를 추출해 새 task에서 적용/배제/Unknown을
   판정하고 snapshot에 revision 기록.

## 4. Typed Program IR + checkpoint (11절 #3, 1단계)

### 타입/검증

`engine/program.rs`

- Nodes: `observe`, `call_tool`, `branch`, `loop` (typed/validated).
- Predicate: `exists`, `equals`, `compare`, `not`, `all`, `any`.
- inputs: literal 또는 `{from: observationId, path: JSON-pointer}`.
- `validate`는 revision/nodes/필수 필드/중첩 노드를 검사한다.

### Checkpoint

execution.programCheckpoint:

```
{schemaVersion, programRevision, status, frameStack, nextIndex,
 resolvedInputs, completedOperationIds, pendingOperationIds}
```

- 각 node 시작/도구 호출 전후/완료 시 저장.
- 재개 시 같은 revision이면 `nextIndex`부터 시작하고,
  `completedOperationIds`에 있는 node는 **재실행하지 않는다**.
- `pendingOperationIds`는 도구 호출 중임을 기록한다.

### 실행 + Branch

새 `execution` 정책 슬롯의 `parameters.program`을 runtime이 읽어,
model loop 전에 공통 `dispatch_tool_call` gateway로 실행한다.

E2E `a_typed_execution_program_takes_the_failure_branch`:

1. `shell_exec exit 7` 실패
2. `equals(try.exitCode,0)` predicate가 false
3. else branch의 `workspace_write fallback.txt=FALLBACK` 실행
4. 결과:
   - `fallback.txt` 존재, `primary.txt` 없음
   - execution.programCheckpoint.status=completed
   - completedOperationIds에 try/check/fallback
   - pending 비어 있음

### 재개 검증

unit test `program_resumes_from_a_durable_checkpoint_without_repeating_completed_tools`:

- `try`가 완료된 checkpoint(`nextIndex=1`, resolvedInputs.try.exitCode=7)를 저장
- 새 실행에서 program을 재개
- `try`는 재실행되지 않고(operation 1개만 존재), fallback branch가 실행됨
- checkpoint가 completed로 갱신됨

이것은 **runtime 함수 수준의 restart/resume 검증**이다. 프로세스 kill 후
자동으로 program을 이어받는 scheduler 통합은 아직 아니다.

### 한계

- `loop`는 typed/validated이지만 실행 시 명시적 unsupported 오류.
- branch 내부 branch/loop는 아직 미지원(명시적 오류).
- 프로그램 합성(모델이 Program을 생성/수정)은 아직 없다.
- execution 슬롯은 아직 benchmark force activate로 테스트하며, 실험 gate를
  통한 실사용 경로는 다음 단계다.
- `observe`는 last observation 복사 수준이다.

전체: `269 unit + 54 E2E + 16 benchmark` 통과.

## 5. R1 실험 정산 경화 (2026-11-06)

### 증거 재계산

`complete()`는 runner가 보낸 verdict/delta/units를 그대로 믿지 않는다.

- runner가 `trials`(pair/arm/status/auditOk/reportOk/taskOutcome/compliance/
  requiresCompliance/errorClass/…), `manifest`(pairs/seed/oracleVersion/
  fixture/model), `manifestHash`를 제출한다.
- 서버는 manifest hash를 검증하고, **raw trials에서 ITT 성공 delta/units/
  verdict를 다시 계산**한다.
- 불일치, 누락 arm, pair 누락, hash 불일치면 승격하지 않고 request를 completed로
  남기되 `promotionError=evidence rejected`를 기록한다.
- 통과 시 재계산 값으로 policy/strategy gate를 호출한다.

### crash-safe promotion

- settling 진입 시 `promotionIntent {kind,targetId,experimentId}`를 함께 저장.
- 승격 전에 target의 `lastExperiment.experimentId`가 이미 같은 실험이면
  재적용하지 않고 그 결과를 승격 결과로 사용한다.
- settle 만료 회수 시 intent와 target을 대조한다.
  - 이미 적용됨 → request를 `completed`로 복구(재적용 금지)
  - 미적용 → `queued`로 반환
- `evidenceVerification`과 `promotionAppliedAt`을 request에 기록한다.

### 테스트

- `completion_rejects_fake_or_incomplete_evidence`
  (성공 수치 조작, treatment arm 누락 → 승격 거부)
- `a_crashed_settlement_recovers_without_reapplying_promotion_and_stale_claims_fail`
  (승격 후 crash, settling 만료 → 재적용 없이 completed, 재시작 후 유지;
  만료 claim complete → 409)
- `only_one_runner_can_settle_and_promote_an_experiment` (동시 complete 1승)
- `consecutive_failed_adaptations_stop_the_no_progress_loop`
  (적응 실행 2회 연속 실패 후 도구 반복 중단, wire 4회에서 정지)

## 6. 모델 기반 Adaptive Execution (설계 2.4 §4)

실패 시 단순 차단을 넘어, 모델이 원인 가설과 서로 다른 개입을 생성한다.

- 실패 관측: expected=success, actual=error, tool, arguments, environmentVersion.
- `adaptation_prompt`로 모델에게 JSON을 요구:
  hypotheses(claim + predictions), interventions(tool/inputs/hypothesisIds/
  expectedChecks/estimatedCost).
- `choose_intervention`:
  - 실패와 동일한 tool+inputs는 환경이 바뀌지 않으면 제외
  - score = expectedChecks 수 + 연결된 hypothesis 수 − estimatedCost
- 선택된 개입은 **공통 dispatch_tool_call**로 실제 실행된다(origin=adaptation).
- 결과에 대해 각 hypothesis의 prediction을 predicate로 평가해
  supported/refuted/unknown을 기록한다.
- 환경 버전은 성공한 side-effect 도구마다 증가한다. 이전 실패와 같은 호출이라도
  환경 버전이 바뀌었으면 재시도할 수 있다.
- 연속 적응 실행이 진전 없이 2회 실패하면 반복을 중단한다.
- execution에 hypotheses/interventions/selected/result/hypothesisResults/
  adaptationCheckpoint(environmentVersion/failureSignature/selectedIntervention)를
  저장한다.

E2E `a_model_generated_intervention_runs_and_refutes_its_hypothesis`:
- `shell_exec exit 7` 실패
- 모델이 `workspace_write` 개입 2개를 생성
- 잘못된 가설은 `refuted`, 개입은 실제 실행됨
- 환경 변경 후 같은 shell 호출 재시도가 허용되어 2회 실행됨
- adapt1/adapt2 파일 생성, adaptationRecords 확인
- 환경 변경 후 동일 shell 호출 재시도가 허용되어 2회 실행됨

## 7. Program 노드 확장 (설계 2.4 §7)

기존 Observe/CallTool/Branch에 다음을 추가했다.

- `sequence`: child 노드를 순서대로 실행
- `parallel`: 현재 순차 fallback으로 실행하고 checkpoint에 표시
- `verify`: predicate checks를 평가해 `lastVerify` 기록, 실패 시 모델에 알림
- `delegate`: `delegate_task`를 공통 gateway로 실행
- `loop`: predicate가 true인 동안 body를 최대 maxIterations까지 실행,
  `loopIterations` 저장
- `model_decision`: program을 paused로 저장하고 모델 loop에 제어 반환

E2E `a_program_consumes_sequence_verify_loop_and_model_decision_nodes`:
- branch에서 fallback 파일 생성
- sequence로 seq.txt 생성, verify 통과
- loop 1회 실행(loop.txt), model_decision에서 paused
- checkpoint status=paused, loopIterations=1

### 한계

- parallel은 실제 병렬이 아니라 순차 fallback이다.
- loop/sequence child는 call_tool/observe만 지원한다.
- ModelDecision 이후 program 재개는 아직 자동이 아니다.
- process kill 후 program 자동 재개는 runtime 함수 수준 checkpoint resume만 검증됐다.
- 개선기(§9) 비교와 scope별 deployment는 아직이다.
