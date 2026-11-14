# 중국 4개 Agent Harness 소스 비교 → DOMINION 독립 Rust 구현
**조사·변경:** 2026-11-14 / 브랜치 `dominion-sub4`

## 결론
DOMINION은 기존 Rust 엔진과 TypeScript 클라이언트 SDK를 보유한다. 중국 프로젝트 4개를 설치하거나 그들의 정책·실행기를 가져오는 대신, 원본 실행 경계·도구 프로토콜·샌드박스 수명주기·작업 복구 방식을 검토해 Rust 소스에서 독립 구현한다. **이번 코드 변경은 Qwen 전용 구형 Tool Grammar의 Rust 파서다.** 나머지는 구조적 갭과 구현 계획이며 완료된 기능으로 세지 않는다.

## 실제 참고한 원본 소스와 전용 학습점

| 원본 | 검사한 파일/근거 | 구현 가치 | DOMINION 현황/갭 |
| --- | --- | --- | --- |
| Alibaba AgentScope | [`src/agentscope/agent/_agent.py`](https://github.com/agentscope-ai/agentscope/blob/main/src/agentscope/agent/_agent.py): reasoning/acting/model/permission/reply/context compression 단계의 middleware | 모델 실행 전후 개입을 **모델 프롬프트가 아닌 타입화된 Rust 훅**으로 분리 | `src/engine/chat/runtime/`는 있으나 외부 확장 등록형 Hook API 없음 |
| ByteDance DeerFlow 2.1 | [`docs/ARCHITECTURE.md`](https://github.com/bytedance/deer-flow/blob/main/docs/ARCHITECTURE.md), [`sandbox/sandbox_provider.py`](https://github.com/bytedance/deer-flow/blob/main/backend/packages/harness/deerflow/sandbox/sandbox_provider.py): Harness→Application 단방향 의존, provider lifecycle, RunStore, subagent runtime | 1회 실행·유휴 재사용·프로세스 복구 사이에 **작업 소유권과 샌드박스 범위**를 유지 | `tools.rs`는 호스트 명령 실행을 지원하나 격리 provider 추상화와 소유권 lease 연결은 미구현 |
| Qwen-Agent | [`llm/function_calling.py`](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/llm/function_calling.py): native `use_raw_api` / text grammar 갈림길, [`nous_fncall_prompt.py`](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/llm/fncall_prompts/nous_fncall_prompt.py): `<tool_call>`, [`qwen_fncall_prompt.py`](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/llm/fncall_prompts/qwen_fncall_prompt.py): `✿FUNCTION✿`/`✿ARGS✿` | Provider별 출력 문법을 공통 내부 ToolCall로 **정규화** | `chat/protocol.rs`의 Hermes/DSML/JSON에 구형 Qwen grammar를 **`chat/qwen_compat.rs`로 추가**, 도구 allowlist 검증 공유 |
| OpenManus | [`app/agent/toolcall.py`](https://github.com/FoundationAgents/OpenManus/blob/main/app/agent/toolcall.py): think→act, tool name lookup, arguments decode, tool-result observation, finally cleanup; [`manus.py`](https://github.com/FoundationAgents/OpenManus/blob/main/app/agent/manus.py): MCP 동적 연결/해제 | Registry와 lifetime에 따라 도구별 연결 정리 | DOMINION `mcp.rs`와 `tools.rs`는 동작. 동적으로 추가되는 Rust 도구의 스코프/종료 계약을 SDK화해야 함 |

## 이번 신규 Rust 구현: Qwen 호환

- **경로:** `src/engine/chat/qwen_compat.rs`; `chat/protocol.rs`에서 실제 채팅 실행 경로로 연결
- **지원:** `✿FUNCTION✿: tool_name` 다음 `✿ARGS✿: {"key":"value"}`를 내부 `(name, JSON arguments)`로 변환. 여러 호출 순서를 유지하고, 앞의 응답 본문을 보존
- **실행 전 경계:** 활성 도구의 이름 집합에 없는 도구는 실행하지 않음. JSON 객체만 허용, 비정상 인수는 실행하지 않음. 추론 `<think>`의 호출 예시는 실행하지 않음. 최대 출력 크기와 호출 개수 제한
- **테스트:** 단일/연속 호출, 목록 밖 도구 및 불법 인수, 추론 내용 배제, 본문 보존에 대한 Rust 단위 테스트 5개
- **라이선스/분리:** 알고리즘은 독립 구현. 외부 Python 런타임·AgentScope 정책·LangGraph·Qwen-Agent 패키지를 의존성에 추가하지 않음
- **검증 상태:** 새 Rust 코드가 CI에서 컴파일/실행되는지 **최신 Actions 완료 결과를 별도로 확인해야 함**. 유닛 테스트 존재만으로 통과 주장 금지

## 다음 우선순위 — 코드 변경이 아닌 실행 계획

1. **DeerFlow식 수명주기를 Rust에 독립 구현:** `SandboxProvider` / `SandboxLease` / `SandboxScope(user, task)`와 `acquire, health, exec, release`. 최초 Local provider, 추후 OCI/Docker provider. 사용자 명령은 유지하며 프로세스·파일·자격증명 격리를 실행 인프라에서 담당.
2. **AgentScope식 Hook 등록 API:** `BeforeModel, AfterModel, BeforeTool, AfterTool, BeforeCompaction, AfterTurn` 단계, 실행 순서·오류·상태변경의 명확한 타입 계약. 사용자 지시를 거절하는 상위 정책을 덧씌우는 목적으로 사용하지 않음.
3. **DeerFlow식 원자적 batch delegation:** 작업별 owner/worker lease, heartbeat, fencing token, 재시작 이후 SQL 기반 회수. 기존 `events.rs` 영속 시퀀스 및 `goals.rs`를 재사용.
4. **Rust Native SDK 분리:** 애플리케이션 `axum`에 의존하지 않는 `dominion-agent` 크레이트로 harness/tool/sandbox 타입과 실행 진입점을 제공. 앱은 SDK를 import하며, SDK→앱 역참조를 CI에서 금지.
5. **실제 점수 검증:** Qwen API native / Nous 텍스트 / legacy grammar 각각 동일 파일 수정 과제를 10회 이상 실행하고, 성공률·잘못된 실행·총 토큰·중복 사이드이펙트·재시작 복구를 저장한다.

## 원본 버전 주의
DeerFlow 2.0은 2026-06-25 출시, 2.1은 2026-09-24 출시된 것으로 공식 릴리스 자료에서 확인했다. 일부 구형 Qwen grammar는 신규 Qwen3의 기본 출력이 아닐 수 있다. **네이티브 함수 호출을 이용할 수 있는 모델은 그대로 네이티브 실행 경로를 우선**하고, 호환 파서는 텍스트 형식을 실제로 출력한 경우에만 작동한다.
