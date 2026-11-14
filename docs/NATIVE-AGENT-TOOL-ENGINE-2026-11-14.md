# DOMINION 독립 에이전트: 공개 SDK 구현 분석 → 자체 Rust 엔진 확장 (2026-11-14)

## 설계 참조와 코드 적용 위치

* **[OpenAI Agents SDK](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/tool_execution.py)** — 모델이 현재 제공된 도구를 잘못 호출한 경우 도구 오류를 동일 모델에 반환하는 회복 루프, 도구 사용 이력, 실행 전 인수 검증. DOMINION은 동일 코드를 사용하지 않고 `src/engine/chat/runtime/{tool_route,tool_schema}.rs`에 별도 구현했다.
* **[Rig Agent Rust](https://github.com/0xPlaygrounds/rig/blob/main/crates/rig-agent/src/agent/engine.rs)** — 모델에 전달한 도구 카탈로그 스냅샷을 실행 시점에 보존하여 호출을 검증하고 모델·도구 호출 예산을 분리. DOMINION의 `turn.rs`, `budget.rs`에서 라운드 도구 목록을 고정하고 병렬 호출을 원자적으로 과금하도록 구현.
* **[ACP 공식 Rust SDK conductor](https://github.com/agentclientprotocol/rust-sdk/blob/main/src/agent-client-protocol-conductor/src/conductor.rs)** — 전송/프록시/권한 소유 경계 분리. DOMINION은 라우터가 승인 범위를 넓힐 수 없도록 **기존 허용된 카탈로그의 부분집합만 선택**한다.
* **[GitHub Copilot SDK Rust 세션](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs)** — 모델 턴·세션 이벤트·취소·전송 수명주기를 구분하는 설계 관찰. 외부 SDK나 CLI 런타임은 DOMINION 제품에 포함하지 않는다.

이 프로젝트는 외부 SDK를 라이브러리로 호출하거나 다른 에이전트의 정책을 실행하지 않는다. 판단·도구·승인·사용량·영속화는 모두 도미니언 Rust 소유다.

## 실제 엔진 호출 흐름

`chat::run_turn_body` → 등록된 모델/프로바이더 → 기존 허용 도구 목록 → 각 모델 라운드마다 `tool_route::select` → **해당 라운드의 광고된 도구 목록** → 모델 응답 → `tool_schema::validate` 및 미광고 도구 차단 → 기존 `dispatch_tool_call` 승인/효과 봉인/SQLite `execution_operation` → 다음 라운드.

라우터가 사용하는 관찰 자료는 현재 실행의 DB `execution_operation.status/tool`이며 이전 모델이 출력한 지시문은 결정을 바꾸지 못한다. 실모델 외부 응답에서 판단이 아니라 성공·실패 상태만 관찰한다.

| 상태 | 도미니언 자체 실행기 동작 |
|---|---|
| 코드 수정 작업 최초 | 소스 검색·읽기·범위 수정·파일 쓰기·셸 테스트 동시 제공 |
| 수정 성공 | 소스 확인·테스트 도구 유지; 수정 도구도 제거하지 않음 |
| 수정 실패 | 현재 소스 버전 재관찰, 검색과 범위 수정 우선 |
| 관련 MCP 질의 | 이름·설명을 대조하여 실제 질의와 관련된 **이미 허용된** MCP만 우선 노출 |
| 제공하지 않은 도구 이름 | 실행 거부, 제공 가능한 도구 목록과 함께 모델에 도구 오류 전송 |
| JSON 스키마 불일치 | 실행 전 거부; 필수 필드·형식·enum·배열 등 오류를 모델에 전달 |
| 병렬 읽기 | 동일한 라운드·제안·실행 예산에 원자적으로 할당. 초과 시 기존 순차 루프로 복귀 |
| 문맥이 불명확 | 기존 전체 목록 유지. 라우터가 근거 없이 기능을 숨기지 않음 |
| 그룹대화·RSI | 각자 기존 별도 엄격 정책·검증을 유지하며 일반 채팅 라우터로 덮어쓰지 않음 |

실제로 노출된 도구의 이름과 상태는 `audit(tool.native_route)`, `audit(tool.unadvertised_blocked)`, `audit(tool.schema_rejected)`에 기록된다. 사용자는 **활동 화면**의 도구 선택·입력 검증 패널에서 확인할 수 있다.

## 독립 재현 사례와 검증 경계

`src/engine/chat/runtime/tool_route.rs`에는 모델/API 비용 없이 수행하는 동결 사례 테스트가 있다.

* 기존 `trim_tool_definitions`는 비슷한 도구 스키마 크기에서 **카탈로그 앞쪽의 날씨 MCP**를 선점하고, 동일 예산에서 사용자가 요청한 **Jira MCP**가 제거되는 상황.
* 새 `tool_route`는 동일 원본 카탈로그와 동일 정확한 바이트 예산에서 요청에 맞는 Jira MCP를 유지하고 날씨 MCP를 제외해야 한다.
* 읽기·수정·검증 동시 제공, 실패 이후 재관찰, 이미 허용된 도구만 반환, 원본 스키마 순서 유지도 테스트한다.
* `tool_schema.rs`에는 잘못된 필수 인수, 형식, enum, 추가 속성, 중첩 객체/배열, 부분 스키마 호환성 테스트.
* `budget.rs`에는 병렬 승인 시 제안·실행 카운터를 원자적으로 처리하는 테스트.

**측정 단위 구별:** 이 fixture는 도구 스키마의 실제 직렬화 **바이트**를 비교한다. 실모델 토큰 청구액, 실모델 완수율, 작업 품질, 외부 서비스 성공률은 측정한 적 없다. 이에 따라 특정 비율의 실사용 비용 절감이나 독립 과제 성공을 주장하지 않는다.

검증: `cargo test --release`, `npm run typecheck --prefix web`, `node --test scripts/sub3-connection-hub.test.mjs`. 별도 [CI](https://github.com/hushmind3/dominion-rust/actions/workflows/ci.yml?query=branch%3Adominion-sub3).

## 2026-11-14: 외부 SDK와 자체 SDK의 차이

SDK는 외부 런타임을 가져오는 패키지일 수도 있고, 제품 자체 엔진을 프로그램에서 호출하는 클라이언트일 수도 있다. GitHub Copilot SDK는 기본적으로 Copilot CLI 서버와 JSON-RPC로 통신한다. DOMINION은 그 서버·정책 대신 자체 Rust 하네스를 그대로 유지하면서 첫 번째 공용 SDK `sdk/js`를 제공한다. `chat.run/stream`의 SSE, `goals.run/wait/resume`의 durable goal/job, `events.watch`의 시퀀스 재생, `approvals.resolve`의 기존 권한 소유 경계를 모듈 API로 열었다. [독립 SDK 실행 경로 및 테스트](../docs/DOMINION-AGENT-SDK-2026-11-14.md)를 참조한다. 별도 입력 거절/정책 엔진은 추가하지 않는다.
