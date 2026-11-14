# DOMINION Rust 하네스 · 공개 SDK 소스 분석과 자체 구현 (2026-11-14)

## 원칙

SDK를 패키지로 가져오거나 실행 엔진을 교체하지 않는다. **도미니온만의 정책·승인·권한·실행·검증·상태 저장의 결정권을 유지**한다. 외부 소스는 비교 대상으로만 삼는다. 아래는 공식 GitHub의 실제 구현 파일을 열어 실행 흐름을 읽고 DOMINION 소스와 대조한 결과다. 코드 복붙이나 SDK 종속성 추가 없음.

| 오픈소스·실제 분석 파일 | 코드에서 확인한 알고리즘 | 도미니온 자체 구현·기존 구조 |
|---|---|---|
| [Qwen-Agent · `qwen_agent/agents/fncall_agent.py`](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/agents/fncall_agent.py) | 모델 → 함수 호출 해석 → 실행 결과 → 다음 모델 호출의 한도 있는 반복 | `src/engine/chat/runtime/turn.rs` 실행 루프에 **행동 없는 작업자 1회 재요청**과 미수행 결과 검토 처리 |
| [Qwen-Agent · `qwen_agent/llm/fncall_prompts/qwen_fncall_prompt.py`](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/llm/fncall_prompts/qwen_fncall_prompt.py) | `✿FUNCTION✿`/`✿ARGS✿`의 별도 함수 파서 | `src/engine/chat/protocol.rs`: 2가지 마커를 대상 이름 allowlist와 JSON object 검사 후 DOMINION `ToolCall`로 변환 |
| [Qwen-Agent · `qwen_agent/llm/fncall_prompts/nous_fncall_prompt.py`](https://github.com/QwenLM/Qwen-Agent/blob/main/qwen_agent/llm/fncall_prompts/nous_fncall_prompt.py) | `<tool_call>`로 호출 표현, 사고 구역의 거짓 호출은 실행하지 않음 | 자체 `outside_thinking`로 `<think>...` 구역 제거, 네이티브 제공자가 호출을 텍스트로 보냈을 때도 한정 파싱. 승인 경로 우회 불가 |
| [OpenAI Agents Python · `run_internal/run_loop.py`](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/run_loop.py) 및 [`tool_use_tracker.py`](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/tool_use_tracker.py) | 구조화된 tool/output와 모델 종료 구분, 실행자별 도구 사용 추적 | `turn.rs`: 파일 작업에서 실제 `execution_operation`이 없으면 완수 선언을 막고 1회 행동 재시도. 기존 DB 영수증·작업 checkpoint 유지 |
| [Codex Rust · `tools/orchestrator.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/orchestrator.rs) | 샌드박스 정책과 승인 요구를 따로 판단, 권한 상승 재시도는 새 검토 필요 | 기존 DOMINION `tool_dispatch.rs`·`tools/gateway.rs`의 승인·소스 SHA·정책 경로 **그대로 유지**. 모델이 쓴 임의의 JSON이 새 권한을 얻지는 못함 |
| [Google ADK · `tools/_batch_executor.py`](https://github.com/google/adk-python/blob/main/src/google/adk/flows/llm_flows/tools/_batch_executor.py) / [`context/_compaction.py`](https://github.com/google/adk-python/blob/main/src/google/adk/flows/llm_flows/context/_compaction.py) | 병렬 함수 반환 시 호출/응답 관계 및 상태 병합, 맥락 축약 후 orphan tool response 복원 | 기존 DOMINION `continuity.rs` working-view와 `order_tool_exchanges` 유지. 이번에는 병렬 효과를 임의로 합치지 않음 |
| [Deep Agents · `middleware/summarization.py`](https://github.com/langchain-ai/deepagents/blob/main/libs/deepagents/deepagents/middleware/summarization.py) | 문맥 사용량 임계치 측정, 최종 도구 결과 보존, 과대 문맥이면 더 작은 요청으로 재시도 | `tool_selection.rs`: 원본 사용자 목표에 따라 **7개 소스 탐색/수정 도구** 중심으로 자동 선택해 작은 모델의 첫 컨텍스트를 축소. 요약 모델을 새로 호출하는 오버헤드는 없음 |
| [Claude Agent SDK Python · `_internal/session_resume.py`](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/session_resume.py) / [`_internal/query.py`](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/query.py) | 일시 중단·명시적 재개·도구 허가·세션 기록 복원 | 기존 `jobs.rs`/결과 불확실 tool checkpoint 유지. 미확인 부작용 자동 재실행 금지 |
| [OpenAI Agents · `run_internal/run_steps.py`](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/run_steps.py) | 모델 답변 → 도구 작업 → 다음 턴을 실제 상태를 기준으로 판정 | `goals.rs`: 미충족 검증 조건이 있고 실제 작업/검증 개선도 0인 경우 모델 reviewer 호출 대신 **결정적 검사 결과로 다음 행동·재계획에 전달**. 상태를 바꾸지 못할 리뷰 추론만 생략 |

## 이번 자체 코드 경로

1. `src/engine/chat/runtime/tool_selection.rs::autonomous_code_task`는 소스 작업인지 판단한 뒤 **이미 등록된 도구 정의** 중 읽기·검색·부분수정·쓰기만 우선 노출한다. 도구 권한·정책·승인·MCP 설정은 변경되지 않는다. 일반 대화와 비코딩 작업은 기존 전체 도구 선택 정책이다.
2. `src/engine/chat/provider/stream.rs`가 실제 Ollama/OpenAI-compatible 토큰 스트림을 받은 뒤 `src/engine/chat/runtime/turn.rs`가 네이티브 함수 호출 유실을 감지한다. 모호한 일반 서술은 실행하지 않는다. **명시적인** `<tool_call>`, Qwen 구형 `✿FUNCTION✿`, 단독 JSON tool intent만 파싱한다.
3. `src/engine/chat/protocol.rs`가 모델의 사고 구역을 격리하고 JSON object·허용 도구 이름을 검사하며, 기존 `runtime/tool_dispatch.rs`에서 원래 정책대로 승인 후 실행된다. 해당 작업의 실행 결과는 **실제 `execution_operation` 영수증**에 기록된다.
4. 코딩 작업에 도구 실행이 전혀 없다면 `turn.rs`는 모델에게 단 1회 실제 읽기 행동을 요구한다. 반복해서 도구 호출이 없으면 `needs_review`을 명시한 오류로 종료한다. 작업이 완료됐다는 모델 문장만으로 성공 처리하지 않는다.
5. 외부 유료 API와 일반 채팅에는 임의로 180초 제한을 부여하지 않는다. **로컬 모델로 실행하는 부모 검증 소스 작업**에서만 네트워크 한도를 기본 180초로 조정해 480초 평가가 단일 무응답 호출에 잠식되는 상황을 줄인다.
6. 엄격한 작업 검증에서 상태 변화가 없을 때는 모델 reviewer를 호출해도 디스크 검증을 바꿀 수 없다. `goals/progress.rs::reviewer_needed`로 실제 새 action 또는 검사 통과 개선이 있는 경우에만 재검토하고, 생략은 `reviewHistory.method`와 감사 기록에 남긴다.

## 증거·시험 / 남은 제한

- Qwen3 4B 이전 **독립 오라클 0/2**, 2개 과제 각각 480초 타임아웃, 최종 파일 SHA 변화 0. 해당 점수는 변경하지 않는다.
- `protocol.rs` 단위 테스트: Qwen `✿FUNCTION✿`/JSON tool intent·허용 이름·사고 구역 부작용 차단.
- `runtime/tool_selection.rs` 테스트: 코딩 작업 7개 도구 우선, shell 미승인인 경우 shell을 창조하지 않음, 비코딩 작업 원래 동작 보존.
- `chat/tests.rs` 통합 테스트: **실제 Rust HTTP provider 경로**에 모의 모델 토큰 스트림을 공급해 텍스트 `<tool_call>` → 실제 `workspace_read` → `execution_operation` 영수증 → 모델 후속 요청까지 확인. **모의 공급자 테스트로 실모델 Qwen 성공을 주장하지 않는다.**
- `goals/progress.rs`: 변경 없는 검증 실패는 추가 reviewer 추론을 생략하는 조건을 순수 검사로 검증한다.
- 실모델 다시 측정 전까지 **새 완수율, 원가 절감률, 원화 청구액, 품질 상승률은 미측정**. CPU 기반 4B의 툴 파서 지원 여부와 최종 소스 결과를 동일 독립 오라클로 비교해야 한다.
