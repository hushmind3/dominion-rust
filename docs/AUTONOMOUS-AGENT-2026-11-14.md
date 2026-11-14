# DOMINION 자율 목표 에이전트 — 출시 후 Sub1 연동 (2026-11-14)

> 통합 심사 v3: **25점 자율 에이전트/하네스**. 모델이 파일을 읽고 명령어를 실행할 수 있다는 사실과 **목표가 최종 완료됐다는 사실**을 구별합니다. 여기의 구현 설명은 정적 코드 추적이며, 이 변경에서 테스트·빌드·유료 모델 호출은 실행하지 않았습니다.

## 1. 실제 작업 완수 경로

1. **사용자 입력**: `web/src/pages/Workflows.tsx` → `web/src/pages/workflows/GoalConsole.tsx::submit`. 목표, 실행자, 최대 반복 수, 선택적 기계 검증 조건을 작성합니다. API `/api/goals`와 `/api/jobs`를 이용해 계획/수정/결과/중단을 볼 수 있습니다.
2. **등록 및 영속 작업**: `src/api/goals.rs::create_goal`이 목표를 DB에 저장하고 `jobs::enqueue("goal")` 성공을 확인한 후 `jobId`를 목표 레코드에 영속화합니다. 등록 실패는 목표를 실패로 표시하며 성공 응답으로 속이지 않습니다.
3. **실제 실행**: `src/engine/jobs.rs::{next_queued,run_job}` → `src/engine/goals.rs::run` → `goal_control::complete`(계획) → `run_quiet_outcome` → `chat/runtime/turn.rs` → `tools.rs`/MCP. 모델의 말이 아니라 도구 성공 결과만 증거로 수집합니다.
4. **검증/수정**: `src/engine/verification.rs::{acceptance_gaps,successful_evidence}`가 파일/명령/성공 도구 영수증을 검사하고 별도 Reviewer 모델이 독립 검토합니다. 실패는 실제 도구 오류 및 `src/engine/recovery.rs::latest_failure_hint`를 포함하여 다음 실행 또는 Replanner 입력으로 전달합니다.
5. **완료 및 계승**: `goals.rs::finish`가 목표 최종 상태, `episode::set_root_outcome`, 감사 로그를 기록합니다. 완료된 목표는 `learning::enqueue_semantic[_for_agent_through]`로 연결해 후속 기억을 생성할 수 있습니다. 이것만으로 RSI의 독립 후손 채택을 주장하지 않습니다.

## 2. 새 기능: 보수적인 정확한 단계 복구

`src/engine/goal_review.rs`는 `reviewVersion=1`인 **검토 전용** 체크포인트를 정의합니다.

- 도구 실행 전: `goalState.inFlight=true`. 중단·크래시 시 외부 부작용을 중복 실행할 수 있어 자동 재실행하지 않습니다.
- 도구 실행 직후: `goal_review::snapshot`이 `inFlight=false`, `reviewPending=true`, `nextIteration`, 증거, 전사, 검토 미충족 항목, 오류 기록을 한 문서에 저장합니다. **그 후** Reviewer를 호출합니다.
- Reviewer 중 재시작: `jobs::recover`가 `goal_review::replayable` 검사에 통과한 경우에만 다시 큐에 넣습니다. `goals::run`은 저장된 결과에서 Reviewer만 재실행하고 이미 끝난 도구/모델 실행을 반복하지 않습니다.
- 이전 버전·손상·필수 증거 누락 체크포인트와 원격 도구 실행 중단은 `needs_review`. 판단을 위해 남겨둔 불확실 상태를 자동으로 성공 처리하지 않습니다.
- 목표가 먼저 `completed/failed/cancelled/needs_review`로 최종 저장됐지만 Job이 `running` 상태라면 `jobs::recover`가 목표의 영속 결과로 Job을 즉시 정합화합니다. 재실행 호출은 없습니다.
- `src/api/jobs.rs::cancel`은 큐에 남은 자율 목표까지 함께 취소 상태로 저장합니다. UX에 무한히 `queued`로 남지 않습니다.

**의미**: 목표 실행과 검토 사이의 크래시로부터 안전하게 복구할 수 있는 새 경계를 추가했습니다. 모든 종류의 중단 작업을 자동 재개할 수 있다는 주장은 아닙니다.

## 3. 새 기능: 증거 기반 프로젝트 테스트 조건

`src/engine/goal_verification.rs::inferred_checks`는 사용자가 명시적으로 *테스트*를 요구하고 검증 가능한 진입점이 있을 때만 완료 조건을 추가합니다.

- Rust `Cargo.toml` → `cargo test`
- Go `go.mod` → `go test ./...`
- JavaScript `package.json`에 실제 test 스크립트가 존재 → `npm test`, `pnpm test`, `yarn test` 중 명시적 패키지 관리자에 따라 선택
- `pytest.ini` → `pytest` (다른 진입점이 없는 경우)

최대 2개 자동 완료 조건. 명령은 고정 문자열이며 사용자 문장을 셸 명령으로 조립하지 않습니다. 사용자가 `successCriteria`를 직접 지정한 경우 그 명시 조건이 우선합니다. 이 모듈은 테스트를 **실행하지 않습니다**. 실행자에게 실제 도구 영수증을 요구하며, 검증 조건을 만족하지 않으면 목표 완료를 거부합니다.

## 4. 사용자 인터페이스

`GoalConsole.tsx`는 일회성 프롬프트/순차 워크플로와 구분되는 작업 모드입니다.

- 목표 입력 → 실행자 선택 → 최대 단계/선택 완료 조건 → `/api/goals` 작업 등록
- 실행 중 목표 목록 3초 간격 갱신, 상태·현재 단계·현재 계획 확인
- 검토 `gaps`, 이전 재계획 및 실제 성공 도구 증거 수, 검토 기록 표시
- `/api/jobs/:id/cancel`에 연결된 중단과 해당 Session 대화 열기

변경 전 `/api/goals`는 있지만 이 UI의 주요 워크플로 화면에는 연결되어 있지 않았습니다. 이 수정으로 사용자가 일반 작업을 직접 자율 목표로 시작할 수 있습니다.

## 5. 심사 기준에 따른 미완결 경계

- **병렬 서브에이전트**: `decompose.rs`, `workflows.rs`, `rooms.rs` 기반이 있어도 `goals::run`이 검증 가능한 DAG를 동시에 실행/합성하도록 연결된 것은 아닙니다.
- **코드 diff / Git PR**: `tools`의 소스 수정과 기록은 있지만 일반 Goals UI에서 변경된 diff/승인/되돌리기가 완결된 통합 기능은 아닙니다.
- **검증 명령 자동 실행**: `goal_verification`은 기계 완료 조건을 요구하지만 명령 자체를 독자적으로 스폰하지 않습니다. 목표 실행자 도구·권한 정책에 의존합니다.
- **도메인 사이 RSI**: 성공/실패 기록은 `episode`와 `learning`으로 계승할 수 있지만, 일반 Goal의 실패 원인을 RSI 개선 프로그램/방법 후보로 자동 채택하는 독립 A/B 경로는 이 변경에 추가하지 않았습니다.
- **실증**: 유료 모델별 완수율·비용·토큰·시간은 측정하지 않았습니다. 이번 세션은 `cargo test`, npm, CI, 브라우저 실행, 벤치마크를 수행하지 않았습니다.

## 2026년 근거

- [OpenAI: Codex as a platform (2026-08-19)](https://developers.openai.com/blog/codex-as-a-platform) — 실행 루프·세션·도구·실패·승인
- [OpenAI: Harness engineering (2026-02-11)](https://openai.com/index/harness-engineering/) — 에이전트 가독성과 실제 기계적 아키텍처 불변 조건
- [Anthropic: Steering Claude Code (2026-06-18)](https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more) — Skills·Subagent·Hooks/컨텍스트 분리
- [SWE-Cycle (2026-05-13)](https://arxiv.org/abs/2605.13139) — 이해·구현·검증 전 주기 완수 평가
- [Failure as a Process (2026-07-10)](https://arxiv.org/abs/2607.09510) — 실패 조기 발견과 복구
- [PAUSE (2026-10-21)](https://arxiv.org/abs/2610.29870) — 구현 전 계획 간섭과 저렴한 복구 전략

**평가 원칙:** 사용자 목표 → 실제 도구/모델 실행 → 저장된 증거 → 독립 검토 → 재계획 → 영속 결과 → 계승이라는 실제 연결만 성취로 셉니다. 소스만 변경한 것을 모델 작업 완주로 계산하지 않습니다.


## 6. 추가 연결: 목표 비용·품질 계측과 실행 중 계획 수정

- `src/engine/goal_metrics.rs::final_report`: `goals::finish`에서 실제 Goal/Session 감사기록을 읽어 `verifiedCompleted`, `iterations`, `replans`, `reviewCount`, `durationMs`, 성공 도구 증거 수, 검증 조건 수, 미충족 항목 수 및 `session_usage`의 모델 호출/토큰/추정 호출 정보를 영속 기록합니다. 가격 미보고를 임의의 0원 또는 추정 할인액으로 바꾸지 않고 `billingAmount:null`을 남깁니다. Goal 세션 기반 사용량은 해당 세션에 귀속된 호출을 나타내며, 별도 planner/reviewer 호출은 포함되지 않을 수 있습니다.
- `jobs::recover`는 목표가 먼저 최종 완료된 경우 `metrics`를 포함한 결과를 Job으로 복원합니다. `jobs::recover`에서 unknown external effect의 실제 세션 검사는 `goalState.sessionId` 및 `workflowState.sessionId`까지 확인합니다. 안전한 검토 체크포인트라도 같은 작업 세션에 미확정 외부 부작용이 있으면 자동 재개하지 않습니다.
- `src/api/goals.rs::update_plan` → `POST /api/goals/:id/plan` → `src/engine/goal_plan.rs::apply_owner_override` → 다음 `goals::run` 실행 반복. 사용자는 GoalConsole에서 계획을 직접 수정합니다. `planOverride`은 독립 필드에 저장되며 기존 성공 기준·실행 권한·도구 호출 중간 상태를 변경하지 않습니다. 실제 다음 실행 반복의 시작에서 CAS로 소비한 뒤 `planHistory`에 `owner_override`를 기록합니다. Review-only 재개 중에는 먼저 이전 작업의 검토를 끝내고 다음 반복에만 반영합니다.
- `src/engine/goal_intake.rs::inspect`: 작업 공간의 최상위 파일 최대 48개와 실제 존재하는 주요 manifest에 한정된 프로젝트 조사 결과를 1회 생성·영속화하고 planner/executor에 제공합니다. 숨김 파일·링크·원문 소스 파일을 임의로 읽지 않으며 파일 이름은 모델에게 지시가 아닌 신뢰하지 않는 프로젝트 데이터로 표시합니다. Executor는 `code_search`·범위 읽기·독립 하위 조사 `delegate_task` 도구를 활용하도록 지시받습니다.
- `GoalConsole.tsx`: 완료 검증, 시간, 도구 증거 수, 검토 라운드 및 보고된 토큰 사용량을 표시하고, 실행 중 계획 수정과 선택적 저비용 Planner/Reviewer 지정이 가능해졌습니다.

**관련 정적 검토/테스트 소스:** `src/engine/goal_plan.rs`, `goal_intake.rs`, `goal_metrics.rs`, `goal_review.rs`, `goal_verification.rs`, `goals/tests.rs`, `jobs/tests.rs`. 함수와 회귀 테스트를 추가했으며 이번 세션에서 실행·빌드·CI를 하지 않았습니다. 측정된 작업 완수율 및 기준 점수 상승은 별도 실제 평가가 있어야 합니다.


## 7. 2026-11-14 후속 구현: 병렬 조사·코드 diff·명시적 복원

- `src/engine/tools/batch.rs::delegate_batch`가 `delegate_tasks` 도구의 1~4개 병렬 조사 작업을 실제 `gateway::delegate`/영속 Job/`jobs::start_now`로 연결합니다. 각 자식은 독립 Session과 `delegatedReadOnly=true` 권한 축소를 갖습니다. 작업 중·대기 중인 같은 부모의 병렬 조사는 최대 4개이며, 완료는 부모가 `task_status/task_result`로 수집합니다. 부모만 소스 변경·결과 통합을 수행합니다. **코딩 서브에이전트의 별도 worktree 간 소스 자동 병합과는 다릅니다.**
- 읽기 전용 권한은 프롬프트 문장이 아니라 `tools/batch.rs::delegation_allows`로 검사합니다. 모델이 볼 수 있는 Tool Schema는 `chat/runtime/turn.rs`에서 줄이고, 일반 네이티브 디스패치(`tools/dispatch.rs`)와 MCP/모델 도구 실행(`chat/runtime/tool_dispatch.rs`) 및 결정적 직접 실행 모두 같은 제한을 적용합니다. `shell_exec`, 소스 쓰기, Git API 변경, MCP, 브라우저 상태 변경, 추가 위임은 이 모드에서 차단됩니다.
- `src/engine/goal_changes.rs::baseline`은 **모델/도구 호출 이전** Git HEAD와 작업 공간 clean 상태를 기록합니다. `preview`는 실제 성공한 쓰기 도구의 `written:true` 증거에 나타난 경로만 `git diff HEAD`로 비교합니다. `undo`는 **목표 종료 + 시작 상태 clean + HEAD 동일 + 해당 목표 도구의 실제 쓰기 증거 + 현재 파일 SHA-256 동일 + 원본 Git 일반 파일 및 60KB 제한 + 다른 작업 미실행**에서만 지정된 1개 파일을 복원합니다. 파일 변형은 기존 DOMINION의 per-file mutation lock을 이용합니다. 전체 `git reset`/`git clean`/자동 브랜치 삭제는 없습니다. 외부 에디터가 동시에 바꾸는 경쟁까지 완전히 배제할 수 있다고 주장하지 않습니다.
- `src/api/goals.rs::{changes,undo_change}`와 `/api/goals/:id/changes`, `/api/goals/:id/undo`를 통해 실제 제품 API가 연결됩니다. `GoalConsole`의 `GoalChanges.tsx`는 파일별 diff, 복원 제한 사유, 복원 전 확인 대화창과 서버 결과를 표시합니다.
- 검증 소스: `tools/batch.rs`는 허용 도구 정책 단위 테스트, `goal_changes.rs`는 실제 Git 임시 저장소의 baseline·diff·해시 검증·복원 테스트를 포함합니다.

### 엔진/RSI와의 책임 경계

일반 Goal은 `episode`·`learning`에 실증 결과를 보내지만 RSI의 동결 평가 결과를 생성하거나 특정 후손을 자동 채택하지 않습니다. 자율 병렬 조사는 **읽기 전용**이므로, 둘 이상의 에이전트가 같은 공유 작업 공간에 소스를 동시에 쓰고 `git merge`로 해결할 수 있다고 가장하지 않습니다. 복수 실행자의 독립 worktree 소스 생성·컴파일·순차 승인 병합은 아직 별도 구현 범위입니다.

**검증 현황:** 이전 Sub1 HEAD `79383ad`는 GitHub CI `43531904929`에서 npm 타입 검사·React 빌드·`cargo test --release`가 성공했습니다. 이후 병렬 도구와 diff/복원 구현에서도 GitHub Actions가 실행되고 있으며, 그 새 커밋의 검사 결과를 최종 확인해야 합니다.
