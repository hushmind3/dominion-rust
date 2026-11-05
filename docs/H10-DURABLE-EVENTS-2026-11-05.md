# H10 durable events / H7 fixture 확장 — 2026-11-05

## 1. D13 durable event sequence

기존 `Bus`는 broadcast 512만 사용했고 `Lagged`를 무시해 느린 소비자의 이벤트가
사라졌다. 다음을 실제 엔진에 연결했다.

- `event_log(seq INTEGER PRIMARY KEY, data TEXT)` 테이블 추가 (db.rs).
- `Bus::new(store)`가 마지막 seq에서 시작한다. 재시작해도 seq가 재사용되지 않는다.
- `Bus::emit`이 이벤트에 `seq`를 붙여 durable log에 append한 뒤 broadcast한다.
  로그는 256개마다 오래된 항목을 정리해 최근 2,000개 수준으로 제한한다.
- `GET /api/events?after=<seq>` 또는 SSE `Last-Event-ID` 헤더로 끊긴 지점부터
  재생한다. `Lagged`가 발생하면 broadcast를 포기하지 않고 durable log에서
  누락 구간을 다시 읽는다.
- SSE `id:` 필드에 seq를 넣어 브라우저 재연결 계약과 맞췄다.

테스트:
- `events_are_durable_and_replayable_after_a_disconnect`
- `the_log_is_bounded_without_losing_recent_events`

아직 아님: 장기 soak에서의 로그 크기/지연 측정, 클라이언트 ack 기반 보존 기간,
다중 프로세스에서의 seq 조정(단일 엔진 전제).

## 2. H7 fixture 확장

runner는 `BENCH_FIXTURE`로 두 작업군을 지원한다.

| fixture | task | oracle |
|---|---|---|
| `audit_report` | report 생성 + audit 노드 | `harness_audit.txt=audit-harness`, `harness_report.txt=REPORT-OK` |
| `update_config` | `config_input.txt` 읽고 `config.txt`에 `mode=final` 저장 | `config.txt=mode=final`, `config_input.txt=mode=draft` 유지 |

- `BENCH_HOLDOUT=1`이면 manifest에 `evaluationRole=holdout`을 기록한다.
- fixture task/oracle/seeds가 manifest와 snapshot에 기록된다.
- 잘못된 fixture id는 실행 전에 거부한다.

아직 아님: holdout 전용 예약 규칙, fixture별 최소 표본, 작업군 간 전이 측정.

## 3. H10 load / crash matrix (2026-11-05)

E2E로 실제 엔진 프로세스를 사용해 두 시나리오를 실행했다.

### load: 병렬 세션

`load_matrix_completes_parallel_sessions_without_cross_talk`

- 6개 세션에 동시에 chat 요청 (mock provider)
- 결과: 6/6 execution `completed`, active 0, 세션 간 응답 교차 없음
- 통과 출력: `test result: ok. 1 passed`

### crash: 실행 중 프로세스 kill → 재시작

`crash_drill_leaves_no_running_execution_after_restart`

- 4개 세션 실행 중 엔진 프로세스 종료
- 같은 data root로 재시작
- 결과:
  - `status=running` execution 0
  - queued/running job 0 (wake resume job이 끝날 때까지 대기)
  - execution 수 ≥ 4, 모든 상태가 interrupted/completed/failed/cancelled
  - `run_wake` 결정이 실제로 기록됨
- 통과 출력: `test result: ok. 1 passed` (4.27s)

아직 아님: 장시간 soak, 실제 부하(수십~수백 세션), restore/backup drill,
다중 기기 ACL.
