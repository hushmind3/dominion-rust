# H4 RunCoordinator reducer + automatic wake (2026-11-05)

## 1. Phase reducer

`src/engine/run_state.rs`가 실행 phase를 명시적 상태기계로 관리한다.

```
queued → running → (waiting_approval) → verifying → finalizing → succeeded
             └──────────── failed / cancelled / interrupted
```

- 터미널 phase에서는 `start`, `finalize`, `interrupt`가 거부된다.
- `transition`은 순수 함수이며 잘못된 전이는 audit(`run.phase_rejected`)만 남기고
  phase를 바꾸지 않는다.
- 실행 생성 시 `phase=queued`, lease 획득 후 `start`, finalize에서 최종
  status를 `phase_for_status`로 매핑한다.
- 재시작 복구(migration/jobs)는 running 실행을 `phase=interrupted`로 종결한다.

테스트: `a_run_cannot_restart_after_a_terminal_phase`,
`crash_recovery_can_interrupt_running_and_waiting_runs`,
`final_status_maps_to_one_terminal_phase`.

## 2. 자동 wake/retry

`src/engine/wake.rs`가 시작 시 interrupted 실행을 한 번만 평가해
`run_wake` 결정을 남긴다.

| 조건 | 결정 |
|---|---|
| 이미 succeeded | `skipped` |
| `outcomeUnknown`/중단된 operation | `needs_review` |
| 미해결/중단 승인 | `needs_review` |
| 완료된 side-effect 도구(`workspace_write`, `shell_exec`, `github_api`, `delegate_task` 등) | `needs_review` |
| 요청/세션 복구 불가 | `needs_review` |
| 부작용 없음 | `queued` + `resume` job 생성 |

- wake는 `wake:{executionId}` idempotency로 두 번 생성되지 않는다.
- `resume` job은 `run_quiet_resume` → `run_turn_resume`을 사용한다.
  `record_input=false`이므로 **원래 사용자 메시지를 중복 저장하지 않는다.**
- taskId/taskRevision을 그대로 넘겨 같은 task 계약으로 재개한다.
- 외부 결과가 불확실하면 자동 재실행하지 않는다.

API: `GET /api/run-wakes`

E2E `a_clean_interrupted_run_is_woken_and_resumed_after_restart`:
- 모델 호출 중 프로세스 kill → 같은 data root 재시작
- `run_wake.decision=queued`, `resume` job completed
- 실행이 2개 이상, 사용자 메시지는 1개 유지
- 통과 출력: `test result: ok. 1 passed` (전체 E2E 30 통과에 포함)

## 3. 남은 것

- 목표/대화/워크플로의 planning 정책(reactive/outline/graph) 선택을 reducer에 통합.
- wake 재시도 횟수/백오프 정책, 사용자 알림 UI.
- lease fencing token과 다중 프로세스 조정.
