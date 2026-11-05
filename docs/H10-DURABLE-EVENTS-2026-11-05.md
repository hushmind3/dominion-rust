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

## 4. restore drill (2026-11-05)

- `POST /api/system/backup` → `Store::backup_to` (`VACUUM INTO`)가 WAL 포함
  일관된 스냅샷을 `data/backups/dominion-<timestamp>.sqlite`로 만든다.
- E2E `backup_snapshot_restores_the_engine_after_a_total_db_loss`:
  1. 대화 1회 실행 (completed execution + assistant message)
  2. backup 생성
  3. 엔진 종료 후 DB를 backup으로 교체, WAL/SHM 제거
  4. 같은 data root로 재시작
  5. execution `completed`와 원래 assistant 메시지가 복원됨
- 통과: `test result: ok. 1 passed` (전체 E2E 33 통과에 포함)

아직 아님: 정기 백업 스케줄, 보존 기간/암호화, 라이브 restore API,
다중 기기 ACL과 대규모 soak.

## 5. 정기 백업 (2026-11-05)

`engine/maintenance.rs`

- `DOMINION_BACKUP_INTERVAL_SECS` (기본 3600, 0이면 비활성) 주기로
  `Store::backup_to` 실행.
- `DOMINION_BACKUP_KEEP` (기본 5) 개만 남기고 오래된 snapshot 삭제.
- 성공/실패를 `backup.created`/`backup.failed` audit로 기록.
- 시작 시 `engine::maintenance::start(state)`가 background task를 띄운다.

검증:
- `retention_keeps_the_newest_snapshots_only` (unit): 5개 중 2개 유지, 다른 파일 보존
- `scheduled_backup_creates_a_snapshot_without_a_request` (E2E): interval=1초로
  엔진을 띄우면 요청 없이 `data/backups/*.sqlite`가 생성됨

아직 아님: 백업 암호화/보존 기간 정책, 오프사이트 복제, 라이브 restore API,
장시간 soak, 다중기기 ACL.

## 6. Backup encryption / offsite / live restore (2026-11-05)

### 암호화 envelope

- `Vault::seal_bytes/open_bytes` (ChaCha20-Poly1305, 12-byte nonce + ciphertext).
- `maintenance::backup_now`:
  1. `VACUUM INTO` 임시 파일
  2. 임시 파일을 vault 키로 암호화
  3. `data/backups/dominion-<timestamp>.sqlite.enc` 저장
  4. 평문 임시 파일 즉시 삭제
- 키는 기존 `data/secret.key`(0600)이며, 백업과 함께 오프사이트로 복사되지 않는다.
  키가 없으면 복구할 수 없다는 것이 명시적 전제다.

### Offsite

- `DOMINION_BACKUP_OFFSITE_DIR`가 있으면 암호화 파일을 그 디렉터리로 복사하고
  `DOMINION_BACKUP_KEEP` 기준으로 정리한다.
- audit `backup.encrypted`에 경로/크기/offsite 여부를 남긴다.

### Live restore + maintenance mode

- `POST /api/system/maintenance {enabled}`
- maintenance 동안 GET과 `/api/system/*` 외의 write는 **503**.
- `POST /api/system/restore {path, confirm:true}`
  - maintenance가 아니면 409
  - `data/backups` 안의 `.enc`만 허용 (canonical path 검사)
  - vault로 복호화 후 `Store::replace_database`가 store mutex 안에서
    WAL checkpoint → 기존 연결 close → 파일 교체 → 재오픈
  - 복구 후 `migrations::run` 재적용
- `GET /api/system/backups`로 스냅샷 목록 조회.

### 검증

- `binary_backups_round_trip_and_reject_tampering` (vault)
- `encrypted_backup_round_trips_and_reopens_as_sqlite`
  - `.enc` 파일이 SQLite 평문으로 시작하지 않음
  - 복호화 결과를 Store로 열어 레코드 확인
  - VACUUM 임시 파일이 남지 않음
- `scheduled_backup_creates_a_snapshot_without_a_request`
  - 1초 주기로 `.enc` 생성 + offsite 복사 확인
- `live_restore_requires_maintenance_and_preserves_backup_executions` (E2E)
  - backup 후 추가 실행 1건 생성 → maintenance 없이 restore는 409
  - maintenance 중 POST /api/sessions는 503
  - restore 후 execution 1건으로 롤백, 원본 assistant 메시지 보존
  - maintenance 해제 후 쓰기 재개

전체: `242 unit + 38 E2E + 10 benchmark` 통과.

### 남은 것

- 오프사이트 원격 전송(S3/SSH)과 보존 기간/immutability.
- 키 회전/키 백업 정책.
- 라이브 restore 중 진행 중 job/runner의 명시적 drain.
- 장시간 soak, 다중기기 ACL.

## 7. Versioned backup keys and rotation (2026-11-05)

### Keyring

- `data/backup-keys/key-<version>.key` (32 bytes, 0600)
- `data/backup-keys/active` (현재 version)
- 백업 envelope: `DOMBAK2` + `u32 LE version` + 12-byte nonce + ciphertext
- `KeyRing::load_or_create`가 없으면 version 1을 생성한다.
- 기존 `secret.key`는 provider secret과 legacy snapshot용으로 유지된다.

### Backup / restore

- `maintenance::backup_now`는 active backup key로 암호화한다.
- restore는 envelope version으로 키를 찾는다.
  - versioned: 해당 version 키가 없으면 복호화 실패 (조용히 넘어가지 않음)
  - legacy(`DOMBAK2` 없음): 기존 vault key로 fallback
- maintenance mode에서는 정기 백업 daemon도 tick을 건너뛴다.

### Rotation

`backup_keys::rotate(state, keep_old)`:

1. 기존 모든 `.enc`를 현재 키(또는 legacy vault)로 복호화
2. 새 version 키 생성, 모든 백업을 **temp 파일**로 재암호화
3. 새 키 파일과 active version 기록
4. temp를 원본으로 rename
5. `keep_old=false`면 이전 키 삭제, true면 보존
6. audit `backup.keys_rotated`

중간 실패 시 원본 백업은 그대로 남고 새 active로 전환되지 않는다.
rotation은 **maintenance mode에서만** API로 허용된다.

### API

- `GET /api/system/backup-keys` → `{activeVersion, versions}`
- `POST /api/system/backup-keys/rotate {keepOld}` (maintenance 필요)
- `POST /api/system/restore`는 회전 후 새 키로 복호화한다.

### 검증

- `rotation_reencrypts_backups_and_drops_old_keys`
  - version 1 → 2, 재암호화 1건, old key 삭제
  - 새 키로 DB 복호화 후 레코드 확인
- `keep_old_retains_the_previous_key_version` (retained [1,2])
- `a_missing_key_version_cannot_decrypt`
- E2E `backup_key_rotation_reencrypts_and_restores`
  - maintenance 없이 rotate → 409
  - maintenance에서 rotate keepOld=false → active 2, key-1 삭제
  - **회전 후 restore** 성공, execution 1건 보존
  - keepOld=true 재회전 → versions [2,3]

전체: `252 unit + 42 E2E + 14 benchmark` 통과.

### 남은 것

- 원격 키 escrow / HSM, 키 서명.
- provider secret vault(`secret.key`) 회전은 별도 절차다.
- 원격 offsite 전송(S3/SSH), 보존 기간/immutability.
- 장시간 soak, 다중기기 ACL.
