# 도미니언 서브3 · 2026 공개 Agent SDK 실제 적용

## 왜 자체 에이전트 실행기를 계속 확대하면 안 되는가

2026-11-14 기준으로 비교한 공식 프로젝트와 실제 적용 기준.

| 프로젝트/라이선스 | 공개된 실제 기능 | DOMINION 적용 | 운영 경계 |
|---|---|---|---|
| [GitHub Copilot SDK (MIT)](https://github.com/github/copilot-sdk) | 자체 계획·도구 실행·파일 수정·세션·모델·MCP가 포함된 Copilot CLI harness, Rust SDK `github-copilot-sdk` 1.0.34 | **선택적 Copilot SDK Rust 어댑터 구현**, `src/engine/copilot_harness.rs`, 워크플로 단계 UI에서 선택 | Copilot CLI 인증과 사용료는 SDK 라이선스와 별개 |
| [Agent Client Protocol Rust SDK (Apache-2.0)](https://github.com/agentclientprotocol/rust-sdk) | ACP 기반 에디터↔코딩 에이전트, 권한/세션, MCP-over-ACP/프록시 | 다음 교체 가능한 에이전트 백엔드 표준 후보 | ACP는 프로토콜, 모델 품질·비용 보증이 아님 |
| [Rig Rust agent library](https://github.com/0xPlaygrounds/rig) | 모델 Provider 공통화, 도구/후크/평가/메모리, 로컬 GGUF/인스트루먼트 | 내부 모델·도구 추상화·차후 오케스트레이션 통합 후보 | coding-agent CLI는 Linux/macOS 위주, Windows 저사양 지원 요건 확인 필요 |
| [RMCP 공식 Rust SDK](https://github.com/modelcontextprotocol/rust-sdk) | MCP 2026-07-28, 도구 서버/클라이언트, transport/long-running tasks | 기존 MCP 모듈의 장기 교체 후보 | 현재 내부 MCP 경로를 무단으로 변경하지 않음 |

## 실코드: 추가한 GitHub Copilot SDK 백엔드

### 빌드·준비

- 기본: `cargo build --release`는 기존 DOMINION 모델·기억·MCP/채팅을 유지하고 외부 SDK 의존성 코드를 실행하지 않는다.
- 선택: `cargo build --release --features copilot-harness`로 **공식 Rust SDK**를 빌드한다.
- 로컬 머신에서 GitHub Copilot CLI에 인증하고, 필요한 경우 `COPILOT_CLI_PATH`에 CLI 경로를 지정한다. 코드 라이선스가 공개됐다는 이유로 API/모델을 무료로 취급하지 않는다.
- 런타임 상태: `GET /api/agent-harnesses`. `copilot.compiled=true`는 **SDK 코드 빌드 사실**을 의미한다. 외부 인증/실작업 성공의 증거는 아니다.

### 앱 실제 실행 경로

1. 워크플로 단계 편집기의 `에이전트 실행 엔진`에서 **공식 GitHub Copilot SDK**를 선택한다. 기본값은 기존 DOMINION 에이전트 엔진.
2. `POST /api/workflows/:id/run` → 영속 job → `workflows::run`의 단계 디스패치에서 `copilot_harness::run`으로 이동.
3. `github_copilot_sdk::Client::start`가 인증된 Copilot CLI harness를 시작. `SessionConfig`에 모델과 권한 핸들러를 적용한 다음 `session.send_and_wait`로 실제 엔진 실행.
4. `assistant.message` 이벤트가 반환한 원문을 DOMINION 세션의 `write_message`로 보존. 이후 단계/워크플로 결과물은 이미 구현한 `stepReceipts`, `workflow_artifact` 데이터 경로를 재사용.
5. 정책: 기본은 **DenyAllHandler**. `copilotAllowTools=true` **그리고** DOMINION `policy.autoApproveAll=true`가 모두 성립할 때만 **ApproveAllHandler**. UI에서 사용자에게 표시. SDK 내부 도구 이벤트는 영속 로컬 도구 실행 영수증으로 오인하지 않음.
6. 인증·컴파일 또는 본문 획득 오류는 명확한 에러를 반환; 기존 Ollama·로컬·BYOK 모델로 무조건 요금이 드는 자동 폴백을 시도하지 않음.
7. `agent.sdk_turn` 감사 기록은 SDK 런타임 세션과 요청 모델, 도구 승인 범위를 남기고 `billingVerified=false`로 표시. 외부 SDK 모델 사용량을 DOMINION 원화·토큰 절감률로 **추정해 기록하지 않음**.

### CI 검증

- `scripts/sub3-connection-hub.test.mjs`: 실제 SDK Rust 호출 여부, frontend/backend 연결, `/goal` 독립 메뉴 제거 정적 회귀.
- `.github/workflows/sub3-copilot-sdk-check.yml`: 공식 `github-copilot-sdk`를 사용하는 기능이 **Rust 타입 검사와 빌드를 통과하는지** 확인, Cargo.lock 확정. 성공 여부는 Actions 실행 결과로 판단.
- 외부 Copilot CLI 실제 인증·모델 호출·코드 수정·장기 완수·비용 계측 E2E는 별도 현실 환경이 필요하다. 아직 통과했다고 보고하지 않음.

## UX 정리

`/goal`은 코딩 작업의 명령어이며 제품 독립 메뉴가 아니다. 서브3의 `web/src/components/WorkspaceShell.tsx`에서 Goal 사이드바를 제거하고 `App.tsx`에서 독립 화면 라우팅을 제거했다. `/goal` 명령, Goal API, 작업 로그/활동 화면은 유지.

## 다음 기준

평가·상용성은 SDK 설치/코드량이 아니라 **실제 인증된 실행의 성공 횟수, 독립 테스트 통과율, 비용/토큰, 복구**, 그리고 기존 로컬 모델 대비 개선 여부를 측정해야 한다. 공식 SDK조차 완수율은 별개다.
