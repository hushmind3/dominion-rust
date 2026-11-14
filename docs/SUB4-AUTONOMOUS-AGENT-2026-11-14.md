# 도미니온 자율 에이전트 실행 업그레이드 — 2026-11-14

> 대상: `hushmind3/dominion-rust@dominion-sub4`.
> 심사 기준: 자율 에이전트·하네스 25점, 전체 배점 100점. 사용자에게 **목표 입력 → 계획 → 실제 도구 실행 → 독립 검토 → 실패 수정/재계획 → 검증된 산출물**을 제공한다.
> 변경 검증 상태: **GitHub 소스 커밋 완료, Rust/TS 컴파일·테스트·CI·실제 API 모델 호출은 수행하지 않음**. 완주율·점수 상승이나 실제 비용/속도 개선을 입증한 기록이 아니다.

## 1. 실제 호출과 책임

```text
웹: WorkspaceShell('goals') → App.tsx → pages/Goals.tsx
       ├─ POST /api/goals                  목표 생성
       ├─ POST /api/goals/:id/plan         대기 중 사용자 계획 수정
       ├─ POST /api/goals/:id/cancel       사용자 중단
       ├─ POST /api/goals/:id/resume       외부 변경 확인 후 수동 이어가기
       └─ GET  /api/goals, /api/jobs/:id   실시간 작업·증거·파일 확인
API: src/api/goals.rs
       └─ durable `goal-job-<id>` → engine/jobs.rs::run_job
Engine: goals.rs::run
       ├─ task episode start_root              원래 주인 목표·검증 계약 고정
       ├─ goal_control.rs::complete            계획 모델 (실제 연결된 Provider)
       ├─ chat::TurnRequest → run_quiet_outcome 도구 실행
       │    ├─ chat/runtime/tool_execution.rs  도구별 실제 실행 기록/승인
       │    ├─ tools.rs::delegate               제한된 별도 세션/작업 위임
       │    ├─ verification.rs                  완료 자식 도구 증거까지 대조
       │    └─ operations/effect receipts       외부 부작용·중단 상태 기록
       ├─ verification::acceptance_gaps         독립 하네스의 기계적 조건
       ├─ goal_control.rs::complete            별도 검토 모델
       ├─ 실패→재계획→다시 실제 도구 실행
       └─ goals.rs::finish
            ├─ goals/report.rs::build          종료 직전 재검증, 파일 SHA-256
            ├─ completed 또는 needs_review    모델의 완료 선언만으로 성공 불가
            ├─ episode::set_root_outcome       같은 root에 결과 1회 귀속
            └─ 학습 큐                        실제 최종 완료에만 진입
```

## 2. 이 변경에서 실제 추가/수정한 것

| 영역 | 경로 | 결과 |
| --- | --- | --- |
| 목표 실행 진입 UI | `web/src/pages/Goals.tsx`, `web/src/App.tsx`, `web/src/components/WorkspaceShell.tsx`, `web/src/i18n/{ko,en}.ts` | 사이드바의 **자율 목표 실행**에서 목표·담당 에이전트·검증 조건 등록. 실행 현황·계획·재계획 이력·성공 도구·파일 지문·오류 확인 |
| 검증 가능한 최종 성과 | `src/engine/goals/report.rs` → `goals.rs::finish` | 최종 종료 시점에 `verification::acceptance_gaps`를 재평가하고 파일 존재/변경과 bounded SHA256을 기록. 조건이 깨졌으면 `needs_review` |
| 안정적인 작업 계보 | `src/api/goals.rs::create_goal` → `jobs::enqueue_once` | `goal-job-{goal_id}` 고정 ID와 `goal.jobId`를 연결. `task_episode`의 원본 계약·실제 도구 수신 경계 유지 |
| 실행 중단·계획 수정 | `src/api/goals.rs::{edit_plan,cancel_goal}`, `src/api/mod.rs` | 큐 상태에서만 계획 수정. 중단은 목표와 job 및 세션 취소에 연결. 실행 중 삭제는 차단 |
| 작업 복구 | `src/api/goals.rs::resume_goal` → `goals.rs::run` | `needs_review`/실패 후 사용자 확인으로 **새 job** 시작, 이전 세션·계획·성공 도구 증거·실행 횟수 계승, 과거 쓰기를 자동 재생하지 않도록 재확인 지시 |
| 검증 가능한 협력 | `src/engine/verification.rs::{delegated_successful_operations,is_action_evidence,command_matches}` | 부모 목표는 `delegate_task` 또는 `task_result`의 **완료된 자식 작업의 실제 도구 결과**만 사용. 자식 답변 텍스트·미완료 작업·쓰기 없는 수정 툴은 성공 증거가 아님 |
| 위임 비용 경계 | `src/engine/tools.rs::delegate` | 세션당 위임 작업 24개, 동시 queued/running 4개 제한 검사. 실행 시점 비원자적 집계이며 분산 워커에서 강한 원자적 제한이라는 주장은 하지 않음 |
| 코딩 작업 분류 | `src/engine/verification.rs::{looks_actionable,infer_criteria}` | `fix`/`refactor`/`debug`/테스트·PR·버그 수정 등의 목표를 행동 증거가 필요한 작업으로 분류 |
| 목표별 토큰 예산 | `api/goals.rs::create_goal` → `goals.rs::run` → `db.rs::session_usage` → `goals/report.rs` 및 `Goals.tsx` | 기본 150,000토큰(사용자 0~200만 조절). 다음 실행 턴 전에 실제 기록된 세션 사용량을 검사하고 한계 초과 시 `needs_review`로 종료. 보고서에 토큰·캐시 적중·호출 건수 포함. 계획/독립 검토 모델 호출의 실청구 상한은 아님 |
| 검증 시나리오 작성 | `tests/unit/goal_report_tests.rs` · `tests/unit/agent_delegation_proof_tests.rs` · `tests/unit/goals_api_lifecycle_tests.rs` | 거짓 보고, 실제 파일 변경과 회귀, 자식 종료 코드 성공·실패 및 미완료 위임, 목표 생성·계획 수정·중단·수동 재개 계약 검사 소스 작성 (**미실행**) |

## 3. 25점 에이전트 목표와 남은 실제 증명

현재 연결된 부분: 영속 큐, 목표별 모델 계획, 도구 실행, 재시도·재계획, 정적 체크와 독립 모델 검토, 도구 승인 및 중단, 개별 위임 세션, 결과물 지문, GUI, 상태 확인, 수동 복구, 기존 학습·RSI 계보.

여전히 점수 상한을 좌우할 작업:
1. **대규모 프로젝트 자동 탐색·Git 결과 통합**: 한 목표 아래 변경 파일 목록, tree SHA와 diff를 소스에 귀속하고 독립 테스트·검토 후 commit/PR까지 책임지는 범용 코딩 하네스. 기존 `tools.rs` Git·터미널·코드 검색을 더 강하게 한 lifecycle로 묶어야 함.
2. **병렬 위임의 원자적 작업 경계**: 현재 위임 한도는 store 스캔 기반. 실제 다중 worker 안전한 원자적 예약, child outcome 결합, 취소 전파, 주인 목표의 부작용 검증과 격리 수준을 강화.
3. **복구의 실제 외부 상태 재검증**: `needs_review` 뒤 사용자 확인 + 새 작업은 연결됨. remote idempotency keys, 외부 API GET/diff와 원격 영수증 비교를 제공자별로 자동 검증하는 단계는 별도.
4. **완수율과 비용 데이터셋**: 목표 토큰 예산은 기록된 다음 executor 턴 진입을 제어할 뿐 실결제 상한은 아니다. 저장소 변경·PR·브라우저·MCP·멀티에이전트·장기 복구 과제를 실제 모델로 반복 수행하고 `goal completed / needs_review / failed` 비율, 회귀·토큰·KRW·지연을 객관적으로 측정. 정적 소스만으로 Claude Code·Codex 동급 완주를 입증할 수 없음.
5. **목표 평가 ↔ RSI**: 별도 frozen oracle/paired evaluator가 채택한 개선만 후손에 계승. 일반 작업의 주관 평가나 새로운 UI를 독립 RSI 성공으로 승격시키지 않음.

## 4. 품질·안전 계약

- 모델이 `complete:true`라고 주장해도 실제 `acceptance_gaps`가 비어 있지 않으면 완료 판정 금지.
- 코드 변경 도구는 `written:true`가 있을 때에만 **실행한 행동**으로 인정. 파일 자체의 조건을 추가 검증한다.
- 위임은 부모의 도구 권한을 넘을 수 없고, 완료된 하위 도구 결과가 반환된 경우만 검증에 참여한다. 완료되지 않은 비동기 위임은 `task_result`로 실제 완료를 확인해야 함.
- 외부 변경 상태가 불확실한 작업은 자동 replay하지 않는다. 사용자가 결과를 확인하고 이어가기 명령을 내리면 **새 job**으로 복구하며 기존 증거와 원본 목표를 보존한다.
- 마지막 결과물 보고서의 파일 해시는 작업공간 내부 파일만, 최대 8MiB를 읽어 계산한다.
- 모델 기반 검토·실제 검사 모두 필요한 구조이며, 완료 판정은 별도 실행/벤치마크로 평가해야 한다.

## 5. 2026 기준 비교 자료

- OpenAI Codex Agent Harness (2026-08): https://developers.openai.com/blog/codex-as-a-platform
- Anthropic Claude Code agent customization: https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- LoopsBench, long-horizon agent loops (2026-08): https://arxiv.org/abs/2608.00267
- Red Hat real-world SWE benchmark (2026-10): https://github.com/redhat-et/rh-swe-bench

2026 경쟁 프레임의 목표는 벤치마크 점수를 주장하는 것이 아니라, **원본 목표와 실제 검증 가능한 산출물의 동일한 실행 계보**를 만드는 것이다. 기존 심판 결과의 `50/100`은 새 변경 전 비교이며, 이 소스 변경에 따른 재채점은 하지 않았다.

## 6. Git diff · 하위 작업 중단 전파 (CI 실행 대상)

- `goals/git.rs::{snapshot,report}`는 목표의 첫 도구 실행 전에 저장소 HEAD와 시작 상태를 동결하고, 완료 시 HEAD 변경·수정 파일·상태·최대 24KiB diff 미리보기를 기록한다. 단순한 저장소 변경 관측이며 실제 에이전트의 수정 증거는 `execution_operation`과 별도 구분한다. 저장소 루트가 작업공간과 같을 때에만 조회하고 임의 셸 문자열은 조합하지 않는다.
- `state.rs::cancel_delegated_descendants`는 부모 세션의 중단을 모든 하위 위임 세션으로 전파한다. 자식의 영속 취소 epoch와 queued/running job 취소 표시를 갱신하여 이전 모델 호출이 다시 활성화되지 않게 한다.
- `tests/unit/goal_git_report_tests.rs`와 `tests/unit/state_delegation_cancel_tests.rs`에 실제 Git 변경 탐지와 다단계 위임 취소·재시작 후 취소 유지 회귀 검사를 추가했다.
- 변경은 `rsi_system.rs`의 실제 compiled descendant 역할 지도에 등록했다. 모델 가중치 학습과 무관하다.

## 7. CI 결과 해석

`.github/workflows/ci.yml`와 `.github/workflows/sub4-proof-audit.yml`는 서브4 HEAD push에서 자동 실행된다. 타입검사, Vite 빌드, Rust release 모든 타깃 테스트, 계보·구조 감사를 수행하며, 중간 커밋의 중복 실행은 concurrency 설정에 따라 취소된다. `sub4-native-reproduction.yml`와 `sub4-system-live-rsi.yml`는 별도 고비용 실모델 증명 캠페인이므로 CI 성공과 동일시하지 않는다. 실모델·유료 영상 API 실행 결과는 별도 영수증으로 검증한다.

## 8. 격리 worktree와 원본 반영의 실제 연결

`goal_git::prepare`는 격리 요구 목표의 원본 Git 루트·clean HEAD를 검증해 새 checkout을 마련하며 실패 시 원본에 쓰지 않는다. `chat/runtime/tool_execution.rs`의 실제 모델 도구 실행·효과 영수증과 `goals::run`의 기준 검사, `goals/report.rs`의 최종 증거가 같은 격리 경로를 사용한다. `goal_git::workspace_for_session`은 위임된 자식 세션의 부모 연결을 따라간다. 소유자가 `GET /api/goals/:id/changes`에서 파일·새 파일 미리보기·`reviewDigest`를 본 다음 `POST /apply`를 명시적으로 승인할 때만 격리 브랜치 커밋과 원본 fast-forward가 일어난다. 원본 HEAD·dirty tree 변경을 강제 덮어쓰지 않는다. 불필요한 변경은 `POST /discard`로 미반영 worktree만 폐기한다.

전체 경로, 테스트 범위, 알려진 파일 격리와 OS 샌드박스의 차이는 [SUB4-ISOLATED-AGENT-2026-11-14.md](SUB4-ISOLATED-AGENT-2026-11-14.md)에서 구분한다.

## 9. 병렬 읽기 전용 조사 및 RSI 운영 영수증

`tools/batch.rs::delegate_batch`를 통해 상위 에이전트는 최대 4개의 탐색 작업을 동시에 생성하고 `task_result`에서 결과를 모은다. 자식 세션에는 `delegatedReadOnly`가 저장되며 모델 도구 스키마와 실행기, 별도 MCP 호출의 진입점에서 쓰기와 추가 위임이 차단된다. 부모는 실제 증거를 확인한 후 최종 수정과 완료를 담당한다.

`goals::finish`는 상태와 도구·파일 검증 영수증을 `goal_receipt`에 멱등 기록하고, `rsi/planner/focus.rs::generation_context`에서 좁은 범위의 비신뢰 관측 데이터만 활용한다. 이는 실제 사용자 과제의 문제 지점을 다음 세대 소스 조사 후보로 제시하지만 RSI의 독립 평가·채택 및 후손 계승 실증으로 승격되지 않는다. 자세한 호출 경로는 [SUB4-V3-VIDEO-RSI-AGENT-2026-11-14.md](SUB4-V3-VIDEO-RSI-AGENT-2026-11-14.md).


## 2026-11-14 · 자율 목표 단일 토큰 원장과 역할별 호출 차단

`src/engine/goal_control.rs::GoalControl::complete`는 `llm::complete_for_session`을 사용해 계획·검토·재계획 모델 호출을 목표 실행 세션의 `model.usage` 감사 원장에 귀속한다. 호출 **전** `db::Store::session_usage`의 `totalTokens`를 조회해 `goal.tokenBudget` 이상이면 새 helper 호출을 거부한다. 동일 세션의 실행기 `run_quiet_outcome` 사용량도 포함하므로 계획/검토만 별도 무료 예산으로 실행되는 누락을 줄인다. `0`은 기존처럼 제한 없음.

실행 진입점: `api/goals.rs` → `jobs::run` → `goals.rs::run` → `goal_control::complete` → `llm::complete_for_session` → `gateway::record_usage` → `db::session_usage` → `goals/report.rs::build(finalReport.sessionUsage)` → `Goals.tsx`.

`src/engine/goal_control.rs` 회귀 테스트는 실제 로컬 모의 HTTP 응답의 `usage`를 DB에 기록해 1번째 호출 9토큰, 예산 8토큰 도달 이후 2번째 요청 무전송, 예산 `0`으로 변경한 뒤 재개를 검사한다. 소스에 테스트를 작성했으며 **CI·컴파일·실모델 사용량 시험은 아직 실행하지 않았다**.

**제한:** 사용량이 응답 뒤 보고되므로 첫 호출이 남은 한도를 초과할 수 있다. 미보고 usage, 서버측 재시도와 별도 도구·영상 요금은 원화 청구액 상한으로 보장되지 않는다. 정확한 선불 예약/동시 병렬 하위 작업 공유 한도는 향후 별도 원가 원장·원자적 admission이 필요하다. 참고: Microsoft TokenOps, 2026-08-13 (https://commandline.microsoft.com/tokenops-real-time-run-scoped-cost-control-ai-agents/).
