# DOMINION sub3 — 자율 에이전트 하네스와 재귀 개선 연결 (2026-11-14)

## 심사 관점

25점 영역은 도구 호출 개수가 아닌 **목표 수락 → 프로젝트 이해 → 계획 → 실제 실행 → 증거 기반 검토 → 수정·재계획 → 완료/복구** 연결성을 측정한다. 기반 모델 가중치는 고정이며 학습·LoRA·파인튜닝을 RSI 실행 경로에 넣지 않는다. 서브3은 이 연결을 향상시키지만 소스 심사 점수를 직접 선언하지 않는다.

## 실제 호출 경로

1. **작업 등록과 식별**: \`web/src/pages/Operations.tsx\`의 \`GoalWorkbench\` → \`POST /api/goals\` (\`src/api/goals.rs::create_goal\`) → 목표·작업 기록 \`goal.jobId\` ↔ \`job.input.goalId\` → \`jobs::enqueue\` → \`jobs::run_job\` → \`goals::run\`. 등록 실패 시 고아 대기 목표가 남지 않도록 되돌린다.
2. **정적 프로젝트 사전 조사**: \`goal_workspace::snapshot\`은 워크스페이스에 대한 bounded read-only 디렉터리 맵을 만든다. 링크·숨김 폴더·빌드 산출물을 무시하고, 파일 100개·폴더 50개·3단계까지만 읽는다. 같은 목표의 계획·실행·재계획은 저장된 동일 스냅샷을 사용한다. 파일 이름은 신뢰할 수 없는 데이터이며 코드나 지침 본문은 실제 도구로 읽어야 한다.
3. **실제 에이전트 수행**: \`goals::run\` → \`goal_control::complete\`(plan) → \`run_quiet_outcome\` → \`chat::run_turn_background\` → 공급자·MCP·파일·터미널·브라우저·GitHub 도구. 도구 이벤트가 \`TurnStats\` 증거에 남는다. \`delegate_task\`는 독립 하위 작업의 개별 세션과 durable job을 생성하고 \`task_status\` / \`task_result\`로 수집한다. 위임 깊이 4, 활성 하위 작업 6개에 대한 예산형 점검이 있다. 병렬 생성의 원자적 할당은 현재 적용되지 않아 별도 완성 과제다.
4. **검증**: \`verification::successful_evidence\`, \`is_action_evidence\`, \`acceptance_gaps\`는 도구의 실제 성공과 파일·명령 조건을 검사한다. 새로 추가된 \`workspace_replace\`, \`workspace_replace_range\`의 성공도 동작 증거다. 위임자의 \`task_result\`가 completed이고 실제 하위 쓰기·실행 도구가 있는 경우에만 결과를 상위 목표의 동작 증거로 연결한다. 모호한 모델 주장이나 성공하지 않은 도구 결과는 검증으로 승격되지 않는다.
5. **수정 및 재계획**: \`goal_progress::advanced\`는 신규 성공 도구 관측 또는 해결된 결정론적 검증 조건을 진척으로 계산한다. 같은 호출 ID만 바꾼 중복 결과는 진척이 아니다. 진척이 없는 횟수가 \`replanAfter\`에 도달할 때에만 최대 \`maxReplans\`까지 계획을 재작성한다. 성공 시 진척 대기를 초기화한다. \`planHistory\`와 \`reviewHistory\`를 보관한다.
6. **작업 복구**: \`jobs::recover\` → \`goal_recovery::safe_review_checkpoint\`는 완료된 실행의 \`reviewPending\`만 재큐잉한다. \`goals::run\`이 \`nextIteration-1\`의 저장된 실행 증거·오류를 복구해 **검토만** 재시도하며 도구 작업은 재전송하지 않는다. \`inFlight\` 실행·손상된 체크포인트·미확정 외부 작업은 여전히 \`needs_review\`로 멈춘다. 긴 작업의 비멱등 재실행은 금지한다.
7. **사용자 관제**: 활동 화면 \`GoalWorkbench\`에서 목표 등록, 모델 선택, 토큰 상한, 계획 버전, 검증 조건, 도구 증거, 미충족 조건, 취소, 완료 토큰 원장을 확인한다. \`/api/jobs/:id/cancel\`은 목표 상태도 \`cancelled\`로 동기화한다. 완수 기록은 \`goal.receipt\`·\`goal.finished\` 감사 이벤트와 \`episode::set_root_outcome\`에 남는다. 기존 성공 목표의 의미 학습도 유지한다.

## 변경 경계

- \`src/engine/goals.rs\`: 목표·도구·검증·재계획의 실제 오케스트레이션. 내부 테스트 328줄은 \`tests/support/engine/goals.tests.rs\`로 분리.
- \`src/engine/goal_workspace.rs\`: 안전하고 저렴한 파일 구조 사전 지도.
- \`src/engine/goal_recovery.rs\`: 완료된 실행과 미확정 실행의 복구 정책 경계.
- \`src/engine/goal_progress.rs\`: 검증 근거 연결형 진척 판정.
- \`src/engine/verification.rs\`: 수정 도구 및 검증된 위임 작업을 동작 증거로 인정.
- \`src/engine/jobs.rs\`: 프로세스 재시작 시 검토 재개/작업 재실행 구별.
- \`src/engine/tools.rs\`: 제한된 위임 폭·길이, 기존 독립 세션 기반 멀티에이전트.
- \`src/api/goals.rs\`, \`src/api/jobs.rs\`: 내구성 목표-작업 연결, 토큰 예산 및 일관된 취소.
- \`web/src/components/GoalWorkbench.tsx\`: 목표 입력과 검토·증거 관제.

## 2026-11-14 추가 연결: Git 격리·PR·diff·되돌리기

- \`POST /api/goals\`이 \`isolateWorktree:true\`를 받으면 \`goal_git::prepare\`가 현재 워크스페이스의 깨끗한 Git HEAD에서 \`dominion/goal-<id>\` 브랜치와 별도 worktree를 만든다. 원본 변경이 있거나 Git 루트가 아니면 실패하며 원본에 쓰기로 대체하지 않는다. \`goal.gitWorktree\`에 원본·기준 SHA·분리 경로를 지속 기록한다. 선택하지 않은 비코딩 목표에는 기존 경로를 유지한다.
- \`chat/runtime/dispatch.rs\`의 정상 도구와 결정적 치환은 \`goal_git::workspace_for_session\`이 검증한 경로를 사용한다. \`delegate_task\`로 만든 하위 세션도 목표 ID와 동일 worktree를 계승한다. \`verification::baseline_checks_in\` / \`acceptance_gaps_in\`은 해당 worktree의 실제 파일을 검증한다.
- 소스 변경·검증·자동 복구는 \`goals::run\`의 기존 재계획·재시도 루프를 재사용한다. \`verifyCommands\`를 명시하면 \`command_succeeded\` 완료 조건을 삽입하므로 모델이 검사를 실행하고 **정확히 성공한 도구 결과**를 남겨야 완료된다. 실제 명령은 기존 \`terminal_exec\`/\`shell_exec\`의 승인 정책을 따른다.
- \`publishPr:true\`는 격리 작업공간에서만 유효하다. 모델은 \`git\` 도구로 원격 브랜치를 올리고 \`github_api\`를 통해 PR을 만들어야 한다. HTTP 4xx/5xx는 실패 처리하며 \`verification::github_pr_created\`는 201 + PR 번호 + GitHub PR URL을 근거로 판단한다. 자동 PR 제출은 별도로 선택해야 하며 자동 병합하지 않는다.
- \`GET /api/goals/:id/changes\`는 기준 SHA 이후 Git 추적 변경 diff, status, untracked 경로를 읽는다. 내용은 최대 약 100,000자, 민감 파일은 UI 표시 전에 별도 필터링하지 않으므로 작업공간 접근 권한이 있는 사용자만 열어야 한다.
- \`POST /api/goals/:id/rollback\`는 실행 종료 후 명시적 UI 확인을 받아 **그 목표가 만든 로컬 worktree만 폐기**한다. 원본 체크아웃과 GitHub 원격 브랜치/PR은 변경하지 않는다. 살아 있는 하위 세션이 있으면 폐기하지 않는다.
- \`web/src/components/GoalWorkbench.tsx\`에 격리 브랜치·PR 제출·Rust/웹 검사 선택, 변경 diff 미리보기, 명시적 로컬 폐기 버튼을 연결했다.

**실제 CI:** GitHub Actions \`.github/workflows/ci.yml\`은 푸시와 PR 시 웹 타입검사·번들 빌드, Rust release 테스트(리눅스·맥), Rust test binary 컴파일 및 제한 회귀(Windows)를 실행한다. 새 Git worktree 격리/폐기 및 PR 증거 테스트도 같은 Rust suite에 포함돼 있다. 테스트 결과는 실제 워크플로 run URL/상태를 확인해 보고하며 통과 전에는 통과로 쓰지 않는다.

## 완성도 상한과 남은 작업

- 전역 비용: \`tokenBudget\`은 목표 세션의 **모델 실행 토큰 사용량**을 기준으로 다음 실행을 막는다. planner/reviewer가 별도 \`llm::complete\`로 소비하는 모든 비용까지 계산한 정확한 청구 한도는 아니다. 토큰 사용량은 실제 청구 원화 금액이 아니다.
- 배포/PR/작업공간: 독립 Git worktree·PR 생성 증거·읽기전용 diff·로컬 폐기 경로까지 구현했다. 원격 PR 폐기, 자동 병합, 파일별 변경 승인, CI 결과를 목표 실행 세션으로 되돌리는 자동 웹훅은 별도 구현 대상이다.
- 병렬 스케줄러: 세션별 활성 위임 개수 점검은 원자적 동시 요청 제한이 아니다. 진정한 원자적 작업 분배/쓰기 충돌 방지는 후속 과제다.
- 테스트: 단위·목표 통합·Git worktree 격리 테스트를 추가했으며 2026-11-14 사용자의 명시적 요청에 따라 GitHub Actions CI를 실행한다. 실모델 API 호출, PR 생성 실계정 테스트, 유료 비용 측정은 CI 범위 밖이다.
- 진척 판단은 검증 조건과 성공 도구 관측의 구조적 변화에 기초한다. 도구 결과 자체가 고품질 성과라는 보장은 없으며, 이것이 실 모델 기반 완수율을 대체하지 않는다.

## 2026 참고자료

- OpenAI, *Codex as a platform: build on the open agent harness*, 2026-08-19 — https://developers.openai.com/blog/codex-as-a-platform
- OpenAI, *Harness engineering: leveraging Codex in an agent-first world*, 2026-02-11 — https://openai.com/index/harness-engineering/
- Anthropic, *Steering Claude Code: skills, hooks, subagents*, 2026-06-18 — https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- Agent Planning Benchmark, 2026-06-03 / 최신 개정 2026-10-27 — https://arxiv.org/abs/2606.04874
- *Can Terminal Agents Trust Their Own Verification?*, 2026-09-30 — https://arxiv.org/abs/2609.38812
- *Progress Drift: Auditing False Progress in Long-Horizon LLM Agents*, 2026년 — https://aclanthology.org/2026.aacl-short.22.pdf

본 문서의 연결 관계는 2026-11-14 소스 변경에 근거하며, 외부 논문 수치를 DOMINION 실측 성능으로 전용하지 않는다.

## 소스 변경의 명시적 사용자 승인 및 원본 적용 (서브3)

- `GoalWorkbench → GET /api/goals/:id/changes → goal_git::changes → reviewed_patch`가 미리보기 Git 패치와 SHA-256을 제공하며 미추적 파일을 표시합니다. 큰 패치(96KiB 초과)는 이 경로로 승인하지 않고 PR 검토가 필요합니다.
- 사용자 확인 뒤 `POST /api/goals/:id/apply`에서 SHA-256 64자리를 전달합니다. `goal_git::apply_reviewed`는 작업 완료 상태, Git 원본 HEAD, 원본 무변경 상태, 미추적 파일, 진행 중 작업, 패치의 SHA를 재검증한 다음 `git apply --check`를 거쳐 원본에 적용합니다. 적용 후 다시 diff SHA를 검사하고 감사·영수증을 저장합니다.
- 이 작업은 명시적 승인 없이는 원본에 쓰지 않습니다. 원격 배포나 PR 자동 병합을 수행하지 않으며, 승인 패치의 실제 사용 성공률은 별도 검증 대상입니다.


## 2026-11-14 · 서브3 실증 채점 대응: 최신 소스 검증과 원자적 재개

**코드 실행 경로**: `POST /api/goals` → `Store::transaction`에서 최초 goal/job 동시 저장 → `jobs::run_job` → `goals::run` → `chat/runtime/dispatch.rs`의 실제 `execution_operation` 기록 → `verification::acceptance_gaps_in(workdir)` → 검토 결과/목표 영수증. 수동 재개는 `POST /api/goals/:id/resume` → `Store::transaction_if_status`에서 **상태 CAS·목표 커서·새 job·checkpoint 모두 같은 SQLite 트랜잭션**으로 커밋한다. 이전 도구 실행이 불확실하면 커서를 앞으로 옮기고 자동 재실행하지 않는다.

**이전 통과 테스트 재사용 금지**: `verification::fresh_command_matches`는 영속된 성공 셸 영수증(`tool`, `arguments`, `result.exitCode`, `runId`, `opSeq`, `startedAt`, `finishedAt`)을 현재 검증 명령과 대조한다. 마지막 성공 시험 뒤 `workspace_write/replace/replace_range/replace_function/file_write` 등 성공한 소스 수정이 있으면 **이전 테스트 통과를 무효화**하고 현재 소스에서 재검사를 요구한다. 동일 실행의 `opSeq`가 최우선이고 다른 실행은 시간 순서를 사용한다. 순서가 불분명한 기록은 보수적으로 실패 처리한다. 성공한 조회와 실패한 파일 수정은 검증을 무효화하지 않는다.

**Git 격리 워크스페이스 일치**: 목표 전·후 진전 판단과 최종 완료 판단이 동일한 `&workdir` 경로에서 실제 파일 상태를 검사한다. 파일 변경 조건이 원래 체크아웃을 참조해 잘못된 진전/정체를 기록하는 문제를 수정했다.

**사용자 요청 기반 실제 테스트**: `goal_checks::inferred_checks`는 목표에서 테스트 수행 의도가 명시된 경우에만 `Cargo.toml`, `go.mod`, `package.json`의 실제 테스트 진입점 또는 `pytest.ini`에서 최대 2개 고정 검증 명령을 찾아 초기 acceptance contract에 등록한다. 생성한 명령에 자연어 사용자 입력을 끼워 넣지 않으며, 프롬프트의 모델 자기보고를 통과 영수증으로 인정하지 않는다.

**회귀/실측 구분**: Rust 테스트는 동일 실행 편집→시험, 시험→후속 편집, 다른 실행의 편집, 무시해야 하는 조회·실패한 수정, 동시 재개 요청에서 단일 작업 생성, SQLite 트랜잭션 내 작업 저장 실패시 전체 롤백을 검사한다. 3OS GitHub Actions 결과는 해당 HEAD와 연결해서 확인한다. 이 수정만으로 외부 유료 모델의 장기 코딩 완수율이나 RSI 실제 성능 향상 수치를 주장하지 않는다.

**실사용 성과 집계 진입점:** `GET /api/goals/metrics` → `goals::measured_goal_summary`는 실행 후 저장된 `goal.receipt`와 모델 사용량 요약을 집계합니다. `GoalsPage`는 종료된 목표, 하네스 완료, 모델 호출을 동반한 완료, 추정이 아닌 토큰 계측을 동반한 완료를 각각 보여줍니다. 모의 테스트·빈 기록·에이전트 자기보고를 독립 실모델 벤치마크 점수로 환산하지 않습니다. 리포트에는 `independentBenchmark=false`를 명시합니다.

## 2026-11-15 · 서브에이전트 동시성 원자적 예약

`delegate_task`는 종전의 `all("job") → 개수 판단 → session.put → jobs.enqueue` 경쟁 경로를 사용하지 않습니다.
`tools::delegate → jobs::enqueue_delegation → Store::reserve_delegation`이 SQLite 트랜잭션 안에서
동일 부모 세션의 `queued/running` 위임 잡을 세고, 정원(현재 6) 내에서만 **자식 session + job을 함께 커밋**합니다.
정원 초과는 어떤 레코드도 생성하지 않으며, job 삽입 오류도 session이 롤백됩니다. 종료한 위임은 다음 자리로
재할당됩니다. 정원 내에서의 작업 시작·도구 실행·부모/자식의 Git worktree 공유는 기존 Rust 경로를 유지합니다.

- 검증 소스: `tests/support/engine/tools.tests.rs::concurrent_delegate_reservation_is_atomic_and_enforces_hard_budget`
  (32개 동시 예약에 최대 6개 채택, session/job 쌍, 완료 후 슬롯 재사용, 오류 롤백).
- 운영 원가: 병렬 상한은 과다 지출 위험을 줄이는 경계이지 모델 사용료나 편집 충돌까지 직렬화하지 않습니다.
  동일 worktree에서의 **충돌 없는 파일별 병렬 스케줄링**, 부모 취소의 하위 도구 중단·전파, 독립 SWE-bench
  실모델 pass@1 검증은 별도 평가 대상입니다.
- 실행 검증: 본 코드 변경은 소스 및 데이터 원자성 회귀 테스트를 추가합니다. CI가 실제 성공하기 전까지
  컴파일·완주를 확정된 성능 수치로 보고하지 않습니다.

## 2026-11-15 · 실제 부모/하위 에이전트 Stop 연동

`AppState::cancel_session` → `Store::cancel_session_family`은 **단일 SQLite 트랜잭션**으로
`session.parentSessionId`의 전체 delegated 후손을 찾아, 부모·자식·손자의
`session_cancel` epoch 및 관련 `queued/running` 작업·목표를 동시에 중단 처리합니다.
상관없는 다른 세션은 그대로 둡니다. `reserve_delegation`은 같은 DB 잠금에서
부모의 `cancelled` epoch를 조회하고 거부하므로, 경합 결과는
**위임 선행→중단됨** 또는 **중단 선행→위임 거부**뿐입니다.
재시작 이후에도 각 자식의 중단 epoch가 남고, 활성 Rust 모델/도구 루프는
기존 `is_cancelled` 검사에 의해 추가 라운드를 진행하지 않습니다.

`state::tests::stop_propagates_to_nested_delegates_and_keeps_unrelated_sessions_running`은
부모/자식/손자 전파, 형제 세션 보존, 영속 epoch, 사후 위임 거부를 검사합니다.
`state::tests::concurrent_stop_and_delegation_never_leave_an_active_orphan`은 경쟁 조건에서
실행 중인 하위 작업이 남지 않는 두 가지 합법 상태만 인정합니다.
부모/자식이 이미 시작한 외부 비멱등 효과는 자동으로 되돌리거나 성공으로
재해석하지 않습니다. 검증 완료 여부는 해당 HEAD의 CI 결과와 별도로 구분합니다.

## 2026-11-15 · 하위 작업의 실제 소스 기여를 부모 완수 검증에 연결

이전 `tools::task_payload`는 DB에서 순서가 불명확한 `execution_operation`을
`take(20)`으로 잘라 20회 이상 탐색 후의 마지막 파일 수정 영수증을 누락할 수 있었습니다.
이를 `Store::session_execution_operations`의 scoped 영수증을 사용해
실제 성공한 행동 최대 16개를 우선 확보하고, 최근 문맥을 포함해 총 24개로
묶어 부모에 전달하도록 바꿨습니다. 부모에 전송한 출력 텍스트가 4096바이트를 넘으면
앞 1800자와 SHA-256 및 원래 길이를 제공하며, `operationsTotal/operationsOmitted`를
별도로 기록합니다. 전체 원본 도구 영수증은 SQLite에 남습니다.

`verification::source_mutation`이 `completed` 상태의
`delegate_task/task_result` 안에 들어있는 성공한 소스 수정 영수증을
64개 이내의 bounded walk로 실제 확인하게 했습니다. 부모가 다른 작업을
마쳤다는 텍스트 주장만으로는 `requires_source_change`를 통과하지 못합니다.
`is_action_evidence` 역시 `workspace_* / file_write`에 `written=true`를
요구하므로 무변경/실패한 쓰기로 행동 완료를 위조할 수 없습니다.

회귀: `tools::tests::delegated_result_preserves_late_edits_after_many_reads`,
`verification::inference_tests::delegated_writes_satisfy_source_mutation_but_no_op_or_read_does_not`.
독립 SWE-bench 실모델 평가와는 별개입니다.

## 2026-11-15 · UI 중단 명령과 실행 세션 연결

`POST /api/jobs/:id/cancel`의 종전 구현은 `job.sessionId`만 읽었습니다.
`run_prompt`의 첫 체크포인트 이전 또는 재개 작업에는 `job.input.sessionId`,
`goalState.sessionId` 또는 원본 `goal.sessionId`만 저장돼 있을 수 있습니다.
현재 `api::jobs::cancellable_session`이 해당 실제 세션을 찾아
`AppState::cancel_session → Store::cancel_session_family`로 전달합니다.
중복 Stop은 기존 `cancelled` 응답을 그대로 반환하고 다른 완료 작업은
계속 409로 보호합니다. 모델/셸/하위 도구의 각 중단 검사는 기존 Rust 경로를 유지합니다.

`api::jobs::tests`의 `stop_reaches_input_session_before_first_job_checkpoint`,
`stop_uses_goal_session_even_without_job_checkpoint`는 실행 중 작업의
실제 입력 소유자와 목표 소유자 각각을 Stop API에서 검증합니다.

## 2026-11-15 · Stop의 승인 대기 해제

종전에는 한 번 요청한 도구 승인이 `DOMINION_APPROVAL_TIMEOUT_SECS`만큼
대기하는 동안 부모/하위 실행의 Stop 상태와 분리될 수 있었습니다.
`AppState::cancel_session`은 취소 트랜잭션 완료 후
`chat::close_session_approvals`를 부모·모든 위임 후손 세션에 수행해
`approval.pending`을 CAS 취소하고 live oneshot 송신자를 제거합니다.
`request_approval`은 승인 정책 평가 이전과 승인 결과 수신 이후에
세션의 실제 취소 상태를 확인하므로, 늦은 allow/자동 승인으로
취소된 도구를 새로 실행하지 않습니다.

회귀 `approval::tests::stopping_a_session_closes_live_approval_without_timeout`은
실제 pending 기록과 live receiver를 만들고 Stop 후 500ms 이내
승인 대기 해제·DB 취소·반복 실행 거부를 검사합니다.

## 2026-11-15 · 장기 보관 이력과 활성 작업 스케줄 분리

`jobs::next_queued`는 기존에 `Store::all("job")`으로 완료·실패·중단·검토
이력까지 400ms 간격으로 역직렬화했습니다. 사용자가 다수의 장기 목표,
자동화, RSI 세대를 돌릴수록 전체 이력에 비례해 CPU·DB 락 비용이 증가하던
구조입니다. 이제 `Store::live_jobs()`가 `queued/running` 문서만
SQLite 상태 인덱스에서 반환하고 기존 priority/room/학습 동시성/
CAS 스케줄 판단을 그대로 적용합니다. 작업 이력 보관과 API 전체 목록은
변경하지 않습니다. DB 읽기 오류는 audit에 남깁니다.

`jobs::tests::worker_scheduler_reads_only_live_jobs_not_terminal_history`에서
256개 종료 이력과 2개 활성 작업을 같이 저장해, 실제 worker 선택과
종료 기록 보존·직렬화가 보존되는지 검사합니다.

## 2026-11-15 · 위임 작업 전체 토큰 예산 집계

`Store::session_usage(root)`는 한 세션의 사용량만 집계합니다.
목표가 `delegate_task`로 별도 `sessionId`의 후손 작업을 생성하면
부모가 토큰 상한 이하로 보이면서 실제 호출 비용이 늘어나는 구멍이 있었습니다.
`Store::session_tree_usage(root)`는 재귀 SQLite CTE의 `delegated=true`
후손들을 **같은 SQL 스냅샷**으로 집계하고 전체 `model.usage` 감사 기록의
`promptTokens/completionTokens/totalTokens`를 합산합니다.
세션 부모·모델 사용량 조회용 SQLite 인덱스를 추가했습니다.

`goals::goal_budget_exhausted`는 부모·자식·손자 합산 사용량을 검사하고,
`chat::runtime::turn::goal_token_budget_error`는 위임 자식의 각 모델 라운드와
하네스 노드 시작 전 같은 상위 목표 예산을 적용합니다. 집계 실패 시
비용 제한을 우회하지 않습니다. `goals::finish` 영수증에는 후손 비용을
포함합니다. 독립 세션은 제외하며, 한도 초과에 도달하기 전 이미 시작된
병렬 모델 요청의 실제 청구량까지 소급 중단할 수는 없습니다.

회귀: `owner_steering_turn_tests::delegated_goal_budget_includes_all_descendant_model_usage`
(부모/자식/손자 합산, 무관 세션 제외, 상한 초과 차단, 명시적 무제한).


## 2026-11-15 · 권한 감쇠형 점진 도구 발견

2026-07-28 MCP 클라이언트 지침의 catalog → search → inspect/execute 흐름을
서브3 Rust 일반 채팅 엔진에 연결했습니다. `tool_definitions`가 사용자 및
에이전트 허가/모델 정책을 거친 유일한 탐색 대상입니다.
각 라운드의 `tool_route`로 먼저 작게 선택한 뒤, 숨긴 도구나 스키마
예산 초과가 있을 때만 `dominion_tool_search`를 노출합니다.
검색은 외부 API 호출 없이 문자열/한국어 동의어로 도구명 및 설명을 검색하고
유효한 다음 라운드에 스키마를 추가하며, 작업 승인이나 도구 실행 권한을 넓히지 않습니다.
검색 호출은 도구 예산에 포함되고 동일 요청 반복을 억제하며,
실제 tool-call/result를 메시지 이력과 감사 기록에 남깁니다.
기존 RSI 작업과 그룹 에이전트 내부 도구 스키마·검증 알고리즘은 건드리지 않습니다.
Rust 회귀는 `tool_discovery::tests` 3개로 허가 목록 격리,
검색 후 공개 및 총 스키마 바이트 상한, 불필요한 오버헤드 부재를 확인합니다.
2026-07-28 MCP 공식: https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/docs/2026-07-28/develop/clients/client-best-practices.mdx
