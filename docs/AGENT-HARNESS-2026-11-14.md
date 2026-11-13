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

## 완성도 상한과 남은 작업

- 전역 비용: \`tokenBudget\`은 목표 세션의 **모델 실행 토큰 사용량**을 기준으로 다음 실행을 막는다. planner/reviewer가 별도 \`llm::complete\`로 소비하는 모든 비용까지 계산한 정확한 청구 한도는 아니다. 토큰 사용량은 실제 청구 원화 금액이 아니다.
- 배포/PR/작업공간: GitHub·터미널 도구의 원래 승인 정책을 그대로 따르며 독립 워크트리, PR 전후 diff 관제 및 rollback UX는 이 변경만으로 완성되지 않는다.
- 병렬 스케줄러: 세션별 활성 위임 개수 점검은 원자적 동시 요청 제한이 아니다. 진정한 원자적 작업 분배/쓰기 충돌 방지는 후속 과제다.
- 테스트: 코드 안에 단위/목표 통합 테스트 케이스를 추가했으나 이번 세션에서는 테스트·컴파일·빌드·실모델·비용 벤치마크를 실행하지 않았다. 현행 심사 요건에 따른 **정적 연결 검토**만 수행했다.
- 진척 판단은 검증 조건과 성공 도구 관측의 구조적 변화에 기초한다. 도구 결과 자체가 고품질 성과라는 보장은 없으며, 이것이 실 모델 기반 완수율을 대체하지 않는다.

## 2026 참고자료

- OpenAI, *Codex as a platform: build on the open agent harness*, 2026-08-19 — https://developers.openai.com/blog/codex-as-a-platform
- OpenAI, *Harness engineering: leveraging Codex in an agent-first world*, 2026-02-11 — https://openai.com/index/harness-engineering/
- Anthropic, *Steering Claude Code: skills, hooks, subagents*, 2026-06-18 — https://claude.com/resources/articles/steering-claude-code-skills-hooks-rules-subagents-and-more
- Agent Planning Benchmark, 2026-06-03 / 최신 개정 2026-10-27 — https://arxiv.org/abs/2606.04874
- *Can Terminal Agents Trust Their Own Verification?*, 2026-09-30 — https://arxiv.org/abs/2609.38812
- *Progress Drift: Auditing False Progress in Long-Horizon LLM Agents*, 2026년 — https://aclanthology.org/2026.aacl-short.22.pdf

본 문서의 연결 관계는 2026-11-14 소스 변경에 근거하며, 외부 논문 수치를 DOMINION 실측 성능으로 전용하지 않는다.
