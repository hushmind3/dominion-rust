# 2026 Agent SDK·하네스 실제 소스 조사 — DOMINION 자체 구현

조사 기준: 2026-11-14. 아래 직접 코드 확인은 GitHub 원본에서 해당 파일의 구현/상태 필드와 함수 단위를 열어본 것이며, 실모델 성능 검증과는 다릅니다. 외부 SDK/CLI를 도미니언 판단·실행 엔진으로 호출하지 않습니다.

## A. 코딩 에이전트 소스 (실제 코드 확인)

| 프로젝트 | 검사한 원본 소스 | 비교한 핵심 설계 |
|---|---|---|
| OpenCode | [session/processor.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts) | 스트리밍 중 도구 pending/aborted/completed 전이, 모델 사용량, Git snapshot |
| OpenHands SDK | [agent.py](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/agent.py), [parallel_executor.py](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/parallel_executor.py) | 도구별 자원 잠금과 병렬 실행, 중지 도구 뒤 잔여 호출 제한, action/result 구분 |
| Hermes Agent | [conversation_loop.py](https://github.com/NousResearch/hermes-agent/blob/main/agent/conversation_loop.py), [tool_dispatch_helpers.py](https://github.com/NousResearch/hermes-agent/blob/main/agent/tool_dispatch_helpers.py) | 공유 경로/부작용에 따른 도구 배치 분할, interactive 도구의 병렬 금지, session/compression 분리 |
| SWE-agent | [agents.py](https://github.com/SWE-agent/SWE-agent/blob/main/sweagent/agent/agents.py), [history_processors.py](https://github.com/SWE-agent/SWE-agent/blob/main/sweagent/agent/history_processors.py) | 코드 환경의 명시적 관측, 오류·시간 제한, 대화 이력 프로세서 |
| mini-SWE-agent | [default.py](https://github.com/SWE-agent/mini-swe-agent/blob/main/src/minisweagent/agents/default.py) | 최소 실행 루프·모델·환경을 분리한 A/B 비교 기준 |
| Aider | [base_coder.py](https://github.com/Aider-AI/aider/blob/main/aider/coders/base_coder.py) | 코드 편집/파일 문맥과 저장소 변경 작업을 연결한 코딩 세션 |
| Gemini CLI | [client.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/client.ts) | context compression, Turn, tool scheduler, retry/backoff, token limits |
| Qwen Code | [client.ts](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/core/client.ts), [tool-search.ts](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/tools/tool-search.ts) | core/deferred 도구, 이름 지정 검색/키워드 검색, 검색 후 호출 브리지 |
| Kimi CLI | [soul/agent.py](https://github.com/MoonshotAI/kimi-cli/blob/main/src/kimi_cli/soul/agent.py) | 승인 런타임·백그라운드 작업·스킬/MCP·하위 에이전트 책임 분리 |
| OpenManus | [manus.py](https://github.com/FoundationAgents/OpenManus/blob/main/app/agent/manus.py) | 도구 호출형 범용 에이전트·파일 편집·브라우저/MCP·종료 도구 |
| Qwen-Agent | [assistant.py](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/agents/assistant.py) | 함수 호출과 모델 추상화, 다단계 도구 결과의 회수 |

## B. 범용 SDK·장기 실행 프레임워크 (실제 코드 확인)

| 프로젝트 | 검사한 원본 소스 | 비교한 핵심 설계 |
|---|---|---|
| OpenAI Agents SDK | [run_loop.py](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/run_loop.py), [tool_execution.py](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/tool_execution.py) | handoff/tool/final/approval 분리, 중단된 도구 처리와 상태 저장 |
| Pydantic AI | [_tool_search.py](https://github.com/pydantic/pydantic-ai/blob/main/pydantic_ai_slim/pydantic_ai/toolsets/_tool_search.py), [file-read dedup](https://github.com/pydantic/pydantic-ai/blob/main/src/pydantic_ai_harness/pydantic_ai_harness/compaction/_deduplicate_file_reads.py) | 설치/허가/노출 구분, 지연 도구 공개, 압축 경계와 파일 관측 중복 제거 |
| GitHub Copilot SDK Rust | [session.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs), [types.rs](https://github.com/github/copilot-sdk/blob/main/rust/src/types.rs) | 취소 안전한 세션 대기, 도구 검색, 이벤트 ID 및 세션 복원. 런타임 미채택 |
| Swiftide Rust | [context_compression.rs](https://github.com/bosun-ai/swiftide/blob/main/swiftide-agents-contrib/src/hooks/context_compression.rs) | 에이전트 후크, 계획/변경 근거를 유지한 이력 압축 |
| LangGraph | [_loop.py](https://github.com/langchain-ai/langgraph/blob/main/libs/langgraph/langgraph/pregel/_loop.py) | 체크포인트/보류된 쓰기, 다음 단계 실행과 완료 단계 상태를 분리 |
| DeepAgents | [subagents.py](https://github.com/langchain-ai/deepagents/blob/main/libs/deepagents/deepagents/middleware/subagents.py) | 하위 에이전트를 도구로 래핑한 별도 작업 문맥/미들웨어 |
| Google ADK | [runners.py](https://github.com/google/adk-python/blob/main/src/google/adk/runners.py) | 실행 Runner와 세션/호출 문맥/이벤트 스트림 분리 |
| Microsoft Agent Framework | [_workflow.py](https://github.com/microsoft/agent-framework/blob/main/python/packages/core/agent_framework/_workflows/_workflow.py) | graph edge, checkpoint storage, workflow 상태·이벤트 구분 |
| Strands Harness SDK | [event_loop.py](https://github.com/strands-agents/harness-sdk/blob/main/strands-py/src/strands/event_loop/event_loop.py), [agent.py](https://github.com/strands-agents/harness-sdk/blob/main/strands-py/src/strands/agent/agent.py) | 공급자 중립 실행, 메시지/도구/이벤트 재귀 루프·후크. 2026 monorepo 이동 반영 |
| Mastra | [agent.ts](https://github.com/mastra-ai/mastra/blob/main/packages/core/src/agent/agent.ts) | 에이전트와 background, 채널, scorer, auth의 분리 |
| Agno | [agent.py](https://github.com/agno-agi/agno/blob/main/libs/agno/agno/agent/agent.py) | agent 외부의 memory, compaction, compression, learning, storage 구성요소 |
| Hugging Face smolagents | [agents.py](https://github.com/huggingface/smolagents/blob/main/src/smolagents/agents.py) | 모델 주도 도구 호출/코드 실행과 제한/단계 결과 구조 |

## C. 2차 소스와 직접 조사 구분

[2026-08-04 6종 하네스 비교](https://github.com/SteveVitali/coding-agent-harness-study/blob/main/research/12-final-synthesis.md)는 원저자의 pinned-commit 코드 조사다. 여기서 Pi(작은 agent-core/상층 세션/확장), Cline(IDE-SDK 호스트 분리), Goose(MCP와 ACP 양쪽 프로토콜), OpenCode/mini-SWE의 설계 비교를 추가로 검토했다. 이 부분은 **본 조사자가 각 프로젝트 최신 소스 전체를 직접 검증한 결과가 아님**을 구분한다. OpenCode [processor.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts) 및 Goose [agent.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/agent.rs)의 실제 경로도 별도로 확인했다.

## D. 종합한 실행 원칙: 이름이 아닌 상태·효과·증거 기준

1. 도구 상태를 **설치 → 에이전트에게 허가 → 모델에게 광고 → 실제 호출 → 결과 영수증**으로 분리한다. 검색은 허가를 부여하지 않는다. 도미니언: chat/runtime/tool_discovery.rs, tool_selection.rs, turn.rs.
2. 모델이 성공이라고 말한 것과 **도구 호출 성공·외부 효과 확정·최종 검증**은 별개의 상태다. 도미니언: engine/effect.rs, verification.rs, execution_operation.
3. 병렬 실행은 개수보다 **자원 겹침과 효과 유형**을 기준으로 해야 한다. 원격/MCP/쓰기 도구를 제어 없이 병렬화하지 않는다. 도미니언은 현재 읽기만 선택적으로 병렬 실행하며 예산/취소를 별도 검사한다.
4. 체크포인트 재개는 단순히 inFlight 표시를 해제하는 게 아니라, **마지막 체크포인트 뒤의 기록된 실제 연산을 확인한 뒤 재실행 가능성을 판정**해야 한다. 이번 구현: jobs::recover → workflows::interrupted_step_safe_to_repeat → workflows::run. 로컬 파일 관측만이면 안전 재시도. 쓰기/셸/MCP/웹/불명확한 실행/레거시 시각 부재는 사용자 검토.
5. 긴 문맥의 **원본 SQLite 증거는 유지**, 각 Provider의 모델 요청에서만 동일한 파일·범위·결과의 과거 중복을 줄인다. 도미니언: engine/chat/provider.rs.
6. 하위 에이전트는 부모 전체 문맥 복제가 아니라 필요한 계약·근거·산출물만 받아야 한다. DeepAgents/ADK/OpenAI Agents 등에서 분리. 도미니언 기존 delegate_task/workflow의 차기 개선 대상.
7. 모델/Provider 품질과 토큰 단가를 분리하되, 실제 작업 성공률·캐시·청구액·실패 재시도까지 포함해 비교해야 한다. SDK 존재만으로 품질 점수는 없다.

## E. 이 조사로 실제 변경한 DOMINION 코드

- chat/runtime/tool_discovery.rs: 숨긴 MCP/네이티브 도구의 온디맨드 공개, 정규 권한 집합 재검사, 다음 턴 스키마 복원.
- chat/runtime/turn.rs: 병렬 읽기 예산·취소 확인, 광고되지 않은 도구 호출 거부.
- chat/provider.rs: 중복 파일 읽기 결과의 모델 입력본 한정 압축, 원본/버전 보존.
- workflows.rs + jobs.rs: **중단된 워크플로 단계의 유효한 로컬 읽기만 자동 복구**하는 단계 시작 경계·영수증 검사·감사 기록과 단위 테스트. 완결된 외부 도구 효과의 자동 재실행은 하지 않음.
- 별도의 Copilot/OpenAI/Claude/Strands/Python 실행 SDK 의존성은 추가하지 않음. 제품 Rust 실행기와 상태 원장은 도미니언 소유.

## F. 아직 증명하지 못한 것

외부 SDK를 20개 넘게 조사했어도, 코드 구조 비교만으로 실모델 완수율/안정성/원가의 우위를 증명할 수는 없다. CI 컴파일과 모의 DB·영수증 테스트는 별개다. 실제 모델을 동일 과제·동일 예산으로 돌려 편집 성공, 원본 반영, 안전한 중단 재개, 토큰·캐시 적중·청구액까지 측정해야 제품 성능 점수로 환산할 수 있다. 영상·그룹대화·RSI 전체의 능력을 이번 코드 변경만으로 개선했다고 주장하지 않는다.
