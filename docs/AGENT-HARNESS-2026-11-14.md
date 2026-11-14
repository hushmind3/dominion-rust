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
