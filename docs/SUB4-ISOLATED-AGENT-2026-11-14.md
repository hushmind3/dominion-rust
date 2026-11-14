# 서브4 · 2026-11-14 자율 코딩 에이전트 Git 격리 및 승인 반영

## 실행 흐름

1. `POST /api/goals`에서 `isolateWorktree:true`로 목표를 등록한다. 일반 비코딩 목표는 false로 제출할 수 있다. UI `Goals.tsx`는 기본적으로 Git 격리를 권장한다.
2. `src/engine/goals.rs::run`이 목표 세션을 만든 직후 `goal_git::prepare`를 호출한다. 원본 Git 루트·깨끗한 작업트리·HEAD를 확인하고 별도 `data/goal-worktrees/{goalId}`와 `dominion/goal-{id}` 브랜치를 생성한다. 실패 시 원본 실행으로 넘어가지 않고 `needs_review`로 남긴다.
3. `src/engine/chat/runtime/tool_execution.rs::{deterministic_tool,dispatch_tool_call,run_frozen_replacements}`가 실제 도구 실행·효과 영수증의 기준 경로를 `goal_git::workspace_for_session`에서 가져온다. `tools.rs::delegate`로 생성한 다단계 하위 세션도 부모 `goalId`를 역추적해 같은 격리 트리를 사용한다. `file_write` 절대경로와 명시적인 `shell_exec.cwd`가 별도 checkout 밖을 가리키면 거절한다.
4. `src/engine/verification.rs::{baseline_checks_in,acceptance_gaps_in,resolve_path_from}`는 목표별 작업공간에서 기준 파일 버전·수행 후 결과를 검사한다. `src/engine/goals/report.rs::build`도 같은 작업공간의 파일을 실제 SHA-256 산출물 영수증으로 기록한다. `goals/git.rs`는 독립 Git HEAD와 diff 관측 자료를 남긴다.
5. `GET /api/goals/:id/changes`는 변경 diff, 작업 상태, 새 파일 내용 미리보기, SHA-256 검토 영수증 `reviewDigest`를 생성한다. 파일·diff가 너무 크거나 일부 새 파일 내용이 생략되면 자동 원본 반영은 차단된다.
6. 사용자가 UI에서 명시적으로 승인한 경우에만 `POST /api/goals/:id/apply`에 정확한 `reviewDigest`와 `acknowledgeReview:true`를 보낸다. `goal_git::apply`는 원본 HEAD==기준 HEAD·원본 작업트리 clean·하위 작업 종료·미리보기 완전·검토 해시 일치를 검사한 후 **격리 브랜치에서만** commit하고 원본에서 `git merge --ff-only`를 실행한다. `reset`, `force`, `stash`, 원본 수동 overwrite는 사용하지 않는다.
7. 사용자가 폐기를 선택하면 `POST /api/goals/:id/discard` → `goal_git::rollback`이 미반영 worktree만 제거한다. 이미 반영한 변경에 대해서는 폐기가 원본 반영의 취소로 오인되지 않도록 차단한다. 최종 테스트는 `tests/unit/goal_git_integration_tests.rs` 및 `src/engine/goal_git.rs` 내부 테스트에서 Git 저장소를 직접 만들어 적용·폐기·보호 동작을 검증한다.

## 설계상의 경계

- **Git worktree는 파일 작업 충돌 격리**이며 독립 OS 보안 샌드박스가 아니다. 승인된 임의 셸 명령이나 외부 MCP·HTTP 호출이 다른 파일·서비스에 영향을 줄 가능성은 일반 도구 승인 정책과 별도로 통제해야 한다.
- 검토 영수증은 원본 반영 직전에 다시 검사된다. 외부 동시 쓰기에 대한 트랜잭션 잠금이나 Git 서버 단위 전역 분산 락은 제공하지 않는다. 작업 체크아웃 내부의 Git stage/commit과 원본 fast-forward 사이 장애가 발생하면 격리 브랜치에 커밋을 보존한 채 명시적 수동 복구가 필요하다.
- 파일 변경의 관찰 기록과 모델의 실행 증거는 구분한다. Git diff가 존재한다는 사실만으로 목표 완료 판정을 내리지 않고, `verification::acceptance_gaps_in`와 실제 `execution_operation` 증거가 최종 게이트다.
- RSI의 후손 시스템 소스 지도 `rsi_system.rs::COMPONENTS`에 격리 모듈을 연결했지만 독립 A→B→A 실모델 성공률을 만들어내지는 않는다.

## 소스 구조

`api/goals.rs` (owner intent + review API) → `engine/jobs.rs` (durable job) → `engine/goals.rs` (plan/execute/review) → `engine/goal_git.rs` (prepare/isolation/review/apply/discard) → `chat/runtime/tool_execution.rs` + `tools.rs` (isolated work) → `verification.rs` / `goals/report.rs` (proof) → `Goals.tsx` (owner review).

2026년 레퍼런스: [VS Code Agents worktree/permissions](https://github.com/microsoft/vscode-docs/blob/main/docs/agents/run/agents-window.md), [2026-09 Codex worktree/agents review](https://www.harness.institute/research/codex-cli-agents-worktrees-review-load).


## 2026-11-14 · 하위·손자 에이전트 작업트리 수명 보호

`goal_git.rs::has_active_goal_descendants`는 queued/running job의 `input.sessionId`에서 `parentSessionId` 조상들을 따라 목표 `goalId`와 연결되는지를 검사한다. `rollback`과 `apply`가 동일한 확인 함수를 사용하므로 **직속 자식뿐 아니라 손자·중첩 위임 작업이 살아 있을 때** 작업트리 제거/반영을 차단한다. 순환이나 깊이 제한 초과의 불명확한 작업 계보도 차단한다. 다른 작업 목표에 속한 정상 세션은 이 목표의 활성 자손으로 계산하지 않는다.

검증 코드: `goal_git.rs`의 `isolated_goal_diff_and_discard_never_change_source_checkout` 손자 작업 시나리오 및 `cyclic_active_descendant_ancestry_blocks_source_worktree_mutation`. 연결·검사 소스는 작성했으며 이 변경에서 Rust 컴파일/CI 실행은 미확인이다.
