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

전체: `267 unit + 49 E2E + 16 benchmark` 통과.
