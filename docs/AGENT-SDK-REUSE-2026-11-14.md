# 외부 에이전트 기술의 참고 원칙과 도미니언 자체 엔진 개선 (2026-11-14)

## 소유·실행 원칙

**도미니언은 외부 Copilot, Claude Code, Codex SDK의 실행기나 정책 엔진을 호출하는 래퍼가 아니다.** 연구·GitHub 구현은 설계 참고 자료로만 활용한다. 런타임의 에이전트 판단, 작업 계획, 도구 선택·호출, 모델/Provider 라우팅, 실행 승인, 감사/복구, 기억·RSI는 `src/engine` 및 `src/api`가 책임진다. 고정 가중치 공개 모델과 사용자의 API 모델은 도미니언 자체 gateway로 실행한다.

### 2026년 외부 기술에서 참고할 **개념** (소스 재사용/의존 없음)

| 참고 프로젝트 | 참고할 기법 | 도미니언 자체 구현 위치 | 채택 경계 |
|---|---|---|---|
| [GitHub Copilot SDK](https://github.com/github/copilot-sdk) | 대화 턴의 도구 이벤트, 세션 식별, 승인 흐름, 복구 이벤트 | `chat/runtime/turn.rs`, `tool_execution.rs`, `effect.rs`, `run_state.rs` | **Copilot CLI, SDK, 실행 정책 미사용** |
| [Swiftide](https://github.com/bosun-ai/swiftide) | 도구 정의를 작업 맥락에 맞춰 노출하고 실행 전후를 관측 | `chat/runtime/tool_selection.rs`, `algorithms.rs`, `tools.rs` | Rust 도미니언 내부 알고리즘 독립 유지 |
| [OpenAI Agents SDK](https://github.com/openai/openai-agents-js) | 도구/에이전트 전환 추적, 발언 라우팅, 출력 검증 경계 | `rooms/speakers.rs`, `workflows.rs`, `verification.rs` | OpenAI SDK/정책/세션 의존 없음 |
| [Microsoft Agent Framework](https://github.com/microsoft/agent-framework) | 단계 분기·역할 할당·협업 결과 합성·체크포인트 | `workflows.rs`, `rooms.rs`, `jobs.rs` | 런타임 교체 없이 독립 구현 |
| [Pydantic AI](https://github.com/pydantic/pydantic-ai) | durable execution, 단계별 구조화 결과, 복구 | `jobs.rs`, `db.rs`, `lease.rs` | Python/외부 스케줄러 필수 아님 |

## 이번에 직접 구현한 개선: 작업 의도 기반 도구 스키마 예산 편성

이전에는 도구 스키마가 모델 context budget을 넘기면 **원본 등록 순서대로** 스키마를 넣었다. 예를 들어 앞의 영상 도구 스키마가 900바이트이고 전체 예산이 400바이트이면 뒤의 셸·파일 도구가 충분히 작아도 사라질 수 있었다. 스키마 등록 순서가 모델의 실제 작업 능력을 결정하는 오류다.

`src/engine/chat/runtime/tool_selection.rs::trim_tool_definitions`는 다음처럼 도미니언 자체에서 개선했다.

1. 맥락 예산 안에 전체 도구가 들어가면 어떤 도구도 생략하지 않고 원래 순서를 유지한다.
2. 넘칠 때만 **현재 턴의 한국어/영어 요청 의도**, 고유 도구명, 도구 설명 단어를 점수화한다. 파일 편집·읽기·셸 검증·웹 검색·기억·위임에 따라 관련 도구에 우선권을 준다.
3. 점수가 같으면 작은 스키마부터 선택해 비용을 아낀다. 최종적으로 원래 상대 순서를 지켜 provider에 전달한다.
4. 첫 도구가 너무 커서 나머지가 모두 빠지는 문제를 제거하고, 전체가 예산보다 큰 경우 가장 작은 유효 도구를 유지한다.
5. 이는 도구를 허가하거나 실행하지 않는다. 기존 읽기 전용 위임·RSI 허용 도구·권한 검사 및 실제 실행 영수증 로직은 그대로 유지된다.

원본 등록 순서 때문에 관련 셸·파일 도구가 사라지는 경우, 충분한 예산의 무회귀, 모든 스키마 초과, 이름 없는 스키마 제외까지 테스트한다. **외부 SDK/모델/CLI 의존성은 0개**다.

## 검증 수준과 후속 과제

- Rust 컴파일/테스트 CI 통과 여부와 별개로 실모델 완수율 개선을 주장하지 않는다. 동일 작업·모델에서 스키마 토큰, 선택된 도구, 실제 작업 완수율, 도구 실패, 캐시 재사용, 실제 청구 비용까지 비교해야 정식 최적화로 인정한다.
- 모델 지능의 향상은 타사 실행 엔진을 붙여 얻지 않는다. 자체 planner·tool selector·feedback·memory·RSI의 결과로 증명한다.
- `/goal`은 채팅 명령이다. 별도 제품 메뉴는 제거했고 오래된 북마크는 활동 화면으로 연결된다.

## 2026-11-14 소스 수준 대조: SDK의 *구조만* 도미니언 실행기에서 재구현

깃허브 실제 구현 비교:
- [Pydantic AI `toolsets/_tool_search.py`](https://github.com/pydantic/pydantic-ai/blob/main/pydantic_ai_slim/pydantic_ai/toolsets/_tool_search.py): 사용 가능한 목록과 모델에게 공개된 도구를 분리하고, 검색을 통해 공개하며, 컨텍스트 압축 후 가시성을 되살림. 특히 검색 가능한 것과 별도 권한 검사가 필요한 것을 분리한다.
- [OpenAI Agents SDK `run_internal/run_loop.py`](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/run_loop.py) · [`tool_execution.py`](https://github.com/openai/openai-agents-python/blob/main/src/agents/run_internal/tool_execution.py): tool/handoff/final 구분, 승인과 인터럽트 이후 정확한 단계 재개, tool task 취소/settle.
- [Copilot Rust SDK `session.rs`](https://github.com/github/copilot-sdk/blob/main/rust/src/session.rs): 도구 검색 등록, 세션 이벤트 버퍼, idle waiter cleanup, cancel-safe 결과 대기. **CLI/SDK 실행 엔진을 도미니언에 가져오지 않는다**.
- [Swiftide context-compression](https://github.com/bosun-ai/swiftide/blob/main/swiftide-agents-contrib/src/hooks/context_compression.rs): 계획·Git diff·원본 사용자 지시 보존, 과도한 도구 결과 압축 시 최신 관측 보존.
- [Microsoft Agent Framework progressive disclosure](https://github.com/microsoft/agent-framework/blob/main/python/samples/02-agents/mcp/mcp_progressive_disclosure.py): MCP 도구의 검색·공개 분리, 필요시 상세 스키마 제공.
- [Claude Agent SDK session resume](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/session_resume.py): 세션 상태 재구성/재개시 도구·인증·임시 경로 분리.
- [Anthropic advanced tool use](https://www.anthropic.com/research/advanced-tool-use): 광범위한 스키마의 초기 노출 대신 도구 검색으로 필요시 스키마를 불러오는 접근.

**구현:** `src/engine/chat/runtime/tool_discovery.rs` 신규, `turn.rs`와 `runtime.rs` 실제 모델 턴에 연결.

- 새 외부 SDK 없이 이미 허가된 **네이티브 + 설치 MCP 도구 목록**에서만 검색한다. 기존 `delegatedReadOnly`/RSI 제한 이후의 카탈로그이므로 검색이 권한 확대가 될 수 없다.
- 큰 카탈로그에 한해 `dominion_tool_search`라는 **도미니언 내부 함수 도구**를 모델에게 보여주고, 첫 턴에는 작업 적합한 작은 스키마를 선택한다. 작업 도중 검색을 요청하면 기능명과 한정된 설명을 도구 메시지로 반환한 다음 **다음 추론 턴에 실제 스키마를 추가**한다.
- 공개된 도구명은 실행 체크포인트에 저장하고 다음 턴부터 우선 노출한다. 한 턴에 검색은 최대 4회, 총 공개 20개. 외부 효과나 API 호출 없이 검색이 동작한다.
- 제공하지 않은 도구를 모델이 만들어 호출해도 실행하지 않는다. 이전에는 병렬 읽기 경로에서 광고되지 않은 도구가 바로 실행될 가능성이 있었으므로 스키마 경계 검사와 병렬 예산/취소를 같은 턴에 강화한다.
- 언어가 다른 한국어 요청도 Github/파일/웹/기억 등 의도 별칭을 이용해 적합한 이름을 찾는다.
- 이 경로는 모델 품질을 이미 증명한 것이 아니라, 기존에는 불가능했던 **실행 중 도구 발견→스키마 공개→호출** 경로가 실제 Rust 함수로 연결됐다는 뜻이다. 실모델 벤치마크 수치는 별도.


초기 `tool.discovery` 알고리즘이 core/query-relevant로 스키마를 줄여도 숨겨진 설치 MCP까지 검색할 수 있어야 한다. 따라서 코드에서 **전체 설치 목록 → 사용자/세션 권한 필터 → 검색 가능한 카탈로그**, 별도의 **모델 초회 노출 목록 → 다음 턴 공개 목록**을 유지한다. 룸의 단순 잡담(도구 비요청)은 어떤 검색 함수도 보여주지 않는다.

## 원가 구조 개선: 모델별 문맥 압박 시 출처를 보존하는 중복 읽기 제거

실제 코딩 에이전트는 수차례 같은 파일을 읽는다. `provider.rs::fit_tool_context`에서 모델 입력 여유보다 도구 관측 결과가 클 때만 `deduplicate_repeated_file_reads`를 수행한다. `assistant.tool_calls`의 작업명·경로·읽은 줄 범위를 도구 ID와 연결하고, **이후 읽기 결과의 바이트가 완전히 같은 경우**에만 앞쪽 도구 응답을 간결한 참조로 대체한다. 이전/이후 파일 내용이 다르면 둘 다 남기고, 에러 응답·다른 파일·다른 읽기 범위는 변형하지 않는다. SQLite 원본은 수정하지 않는다. 큰 컨텍스트 Provider의 캐시 가능한 원본 대화도 유지한다. 중복 파일 읽기 벤치/실모델 청구액은 별도로 측정한다.

## 단계 복구: 외부 하네스 동작 방식에서 독립 설계를 추출

[LangGraph `_loop.py`](https://github.com/langchain-ai/langgraph/blob/main/libs/langgraph/langgraph/pregel/_loop.py)의 체크포인트 경계, [OpenHands `parallel_executor.py`](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/parallel_executor.py)의 자원별 실행 충돌 판단, [Hermes `tool_dispatch_helpers.py`](https://github.com/NousResearch/hermes-agent/blob/main/agent/tool_dispatch_helpers.py)의 부작용 기반 배치 분할을 대조했다. 우리 워크플로 엔진은 중단된 작업이 실제 로컬 관찰만 수행한 경우에도 전부 사용자 검토로 보냈다. 이 구분을 `jobs::recover`와 `workflows::run` 경로에 통합했다. 스키마·상태·실행 소유권은 DOMINION 단독이며 **SDK 의존성 없음**. 이미 실행했을지 모르는 네트워크/셸/MCP/쓰기와 오래된 체크포인트는 자동 재실행하지 않는다.

## 2026-11-14 SDK 원본 확인: Rust 지원과 판단권의 실제 경계

- [GitHub의 2026-06-02 정식 출시 공지](https://github.blog/changelog/2026-06-02-copilot-sdk-is-now-generally-available/) 및 [Copilot SDK 저장소](https://github.com/github/copilot-sdk): **Rust 공식 패키지 `github-copilot-sdk` 제공**, 기본 `bundled-cli`로 Copilot CLI 실행기 포함. Rust SDK도 JSON-RPC로 Copilot 런타임을 제어하므로 **SDK 패키지를 설치하면 하네스 판단/계획/도구 실행이 Copilot 측으로 넘어간다**. 커스텀 prompt/hook으로 개별 권한·지시를 바꿀 수 있어도 외부 실행 엔진의 전체 동작을 도미니온 코드가 소유하는 것은 아니다.
- [Codex SDK 공식 문서](https://github.com/openai/codex/blob/main/sdk/typescript/README.md): TypeScript SDK는 Codex CLI와 JSONL로 통신해 스레드·실행·도구 이벤트를 제공한다. Codex용 앱 서버도 고유 sandbox/approval 정책을 갖는다. 도미니온 자체 실행 루프와 동일한 뜻이 아니다.
- [OpenAI Agents SDK](https://developers.openai.com/api/docs/guides/agents/sdk): SDK가 도구 호출 루프를 관리하되 호스트가 도구 구현·승인 결정을 소유할 수 있다. 하네스 구성 요소의 구조적 참고 자료로 활용하며 기존 Rust 엔진을 불필요하게 교체하지 않는다.
- [Copilot SDK Hooks](https://docs.github.com/en/copilot/how-tos/copilot-sdk/features/hooks): user prompt, 전후 tool use, 권한 요청 훅을 제공한다. 훅이 존재한다는 사실만으로 외부 런타임 정책이 제거되는 것은 아니다.

**제품 선택:** 도미니온은 사용자 지시·도구·모델·세션·실행 증거를 Rust에서 계속 소유한다. 외부 SDK 자체 정책이나 입력 검열/거절 규칙을 무조건 상속하지 않는다. 도구 승인·파일 범위·실행 성공 판정은 요청을 거절하기 위한 콘텐츠 필터가 아니라 실제 실행과 사용자 통제의 경계다. Provider 자체 제한까지 호스트 애플리케이션에서 제거했다고 주장하지 않는다.

### 구현: 검색 자료가 사용자 요청을 덮어쓰지 않도록 실행 메시지 순서 수정

`src/engine/chat/context.rs::place_retrieval_before_latest_user`는 검색된 기억/경험을 **최신 실제 사용자 메시지 앞에** 배치한다. 기존 `runtime/turn.rs`는 이를 최신 사용자 메시지 뒤에 역할 `user`로 보냈고, 모델이 메모리/검색 기록을 사용자의 새 지시로 오인할 위험이 있었다. 최신 사용자 원문을 보존하고, 검색 자료에 `_dominion_origin: retrieval-data`를 남기며, 이미 짝지어진 assistant/tool 프로토콜은 유지한다. 이 프레임은 서버가 보관한 원문 대화나 새 사용자 요청을 만들지 않는다. 외부 Copilot/Claude/Codex 실행 엔진의 정책은 일절 호출하지 않는다.

동일한 `identity_prefix`가 일반 채팅과 그룹 대화에 적용돼 **사용자의 직접 요청 수행 / 검색·도구·동료 텍스트는 참고 자료**라는 원칙을 공유한다. `context.rs`에 최신 지시 우선·도구 결과 연결 무회귀·빈 검색 문맥 테스트를 추가했다. **GitHub에 소스를 반영했으며 이 변경의 cargo 컴파일·CI 및 실모델 실행은 아직 하지 않았다.**
