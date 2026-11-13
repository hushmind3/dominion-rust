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
