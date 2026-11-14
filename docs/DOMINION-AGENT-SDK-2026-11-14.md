# 서브3 — DOMINION 독립 에이전트 SDK (2026-11-14)

외부 SDK 호출을 에이전트 구현이라고 이름 바꾸지 않는다. DOMINION Rust가
모델 선택, 계획·수행·검증·수정 루프, 도구 실행, MCP, 세션, 승인, 내구성
작업, Git 격리, 영수증과 RSI를 소유한다. 외부 앱이 그 엔진을 바로 사용하는
첫 번째 공용 SDK는 sdk/js 에 있으며 외부 코파일럿 런타임을 포함하지 않는다.

호출 경로:
- chat.run/stream → POST /api/chat → engine::chat::run_turn → model/tool
  loop → execution_operation → 저장된 assistant/message → SDK run 반환
- goals.create/run/wait → POST /api/goals → SQLite goal + job 원자 기록
  → jobs::run_job → goals::run → 검증/재계획 → GET /api/goals/:id 영수증
- goals.resume → POST /api/goals/:id/resume → 상태 CAS/체크포인트
- goals.changes/applyReviewed → Git patch SHA-256 검토 → 명시적 원본 적용
- events.watch → GET /api/events?after=seq → 내구성 SSE 재전송/중복 제거
- approvals.pending/resolve → 기존 실행 권한/승인 시스템
- jobs.cancel → 기존 Rust 취소, SDK AbortSignal은 클라이언트만 중지

SDK는 사용자 텍스트를 검사해 임의 거절하는 정책을 도입하지 않는다.
기존 Rust의 실행권한/도구/승인 경계는 SDK가 우회하지 않는다.
README와 sdk/js/README.md를 진입점으로 유지한다.

참고 원문 (2026년 확인):
- OpenAI Agents SDK: https://developers.openai.com/api/docs/guides/agents/sdk
- OpenAI running agents: https://developers.openai.com/api/docs/guides/agents/running-agents
- GitHub Copilot SDK architecture: https://github.com/github/copilot-sdk
- GitHub Copilot SDK GA (2026-06-02): https://github.blog/changelog/2026-06-02-copilot-sdk-is-now-generally-available/
- ACP Rust SDK: https://github.com/agentclientprotocol/rust-sdk

검증 범위: sdk/js/test 의 Node.js Mock Fetch+SSE 프로토콜 테스트.
실제 외부 유료 API 요청/장기 과제 완수율 측정은 해당 테스트가 입증하지 않는다.

추가 Node 실행 계약: `@dominion/agent-sdk/node`의 `DominionRuntime.start({binaryPath,port:0})` → `node:child_process`로 DOMINION Rust 바이너리 실행 → `DOMINION_DESKTOP_PARENT` stdin 종료 수명주기 → stdout 실제 바인드 포트 파싱 → `/api/health.instanceId` 검증 → `runtime.client`로 기존 도구·세션·승인·goal 엔진 호출. 타사 CLI/SDK 실행 파일은 시작하지 않음. `sdk/js/test/runtime.test.mjs`에서 프로세스 소유·종료 계약을 모의 검증.

비밀 보호: SDK HTTP 요청의 목적지 origin이 configured baseUrl과 다르면 실행 전에 거부하며 HTTP 리다이렉트를 따라 토큰을 전송하지 않는다. 종료 시 부모 stdin EOF 후 응답하지 않는 **자체 자식 프로세스에만** TERM/KILL 단계가 적용된다. 이를 Mock Fetch/ChildProcess 테스트로 확인한다.
