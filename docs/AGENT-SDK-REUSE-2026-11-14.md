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
