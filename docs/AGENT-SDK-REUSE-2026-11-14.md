# 외부 Agent SDK 기술조사 및 실모델 증거 게이트 (2026-11-14)

## 관찰된 문제
기존 DOMINION의 `/goal`은 채팅 명령인데 별도 메뉴·성과 페이지가 개발/평가 자원을 차지했다. 이번 커밋부터 사이드바/라우트는 제거하고 /goal 명령과 SQLite 감사·인간승인 로직만 유지한다. 무엇보다 이미 존재하는 공식 에이전트 런타임과 성능을 비교하지 않은 채 사내 도구 루프만 쌓아온 문제를 분리한다.

## 2026년 실제 공개 소스

| 프레임워크 | 재사용 가능한 능력 | Rust 제품에서의 채택 판단 |
|---|---|---|
| [GitHub Copilot SDK](https://github.com/github/copilot-sdk) | **2026-06-02 GA**, Rust 공식 크레이트 `github-copilot-sdk`, Copilot CLI 기반 도구/계획/파일 편집/세션/하위 에이전트/MCP/권한 핸들러/BYOK | **우선 실모델 비교 대상**. Rust 네이티브 호출 가능. Copilot 인증 또는 BYOK 필요하며 오픈소스 SDK≠무료 무제한 모델 |
| [Swiftide](https://docs.rs/swiftide/latest/swiftide/) | Rust 도구 루프, 타이핑된 작업 그래프, 라이프사이클 훅, MCP, OTel, RAG, pause/resume | 기존 도미니온 Rust 에이전트 런타임에 부분 대체/통합 가능한 후보. CLI/모델과 결합 전 실능력 비교 |
| [OpenAI Agents SDK JS/TS](https://github.com/openai/openai-agents-js) | Handoffs, sessions, MCP, guards, tracing, sandbox agent | TypeScript 별도 런타임이므로 Rust 본체 완전 교체보다 격리된 비교 어댑터 |
| [Pydantic AI](https://github.com/pydantic/pydantic-ai) | typed capabilities + Temporal/DBOS/Restate 등 durable execution 지원 | Python 서비스 의존·운영 복잡도 고려, 재시작/요청 저널링의 비교 기준 |
| [Microsoft Agent Framework](https://github.com/microsoft/agent-framework) | 여러 모델/역할, 순차·병렬·handoff·group graph | Python/.NET/Go 위주; Rust 본체에 기법을 반영하되 외부 프로세스 통합비 비교 |
| [Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-python) | Claude Code agent runtime, 도구, CLI 및 세션 | 별도 런타임·실서비스 비용과 재배포 약관 확인 후 선택 |

## 실제 SDK 호출 파일
`tools/copilot-sdk-probe`는 **본체와 분리된 별도 Cargo 실행기**다. README에 SDK 문서만 링크한 게 아니라 `Client::start → create_session → send_and_wait → get_events → disconnect → stop`을 실제 SDK API로 호출한다.

- 본체 기본 빌드에 추가 종속성 없음. Rust 1.94 이상 필요. Copilot SDK 1.0.34 패키지와 인증(또는 BYOK 모델 키)이 별도로 필요하다.
- 테스트할 때 임의로 생성한 정확한 마커를 `proof.txt`에만 기록하며 모델 프롬프트에는 정답을 싣지 않는다.
- 읽기 `view`만 SDK 권한으로 허용한다. shell/edit/MCP/원격 변경은 차단한다. 승인 범위를 넓히려면 별도의 격리·인간승인 연동이 필요하다.
- 한 번의 실모델 호출에서 최종 **루트 에이전트** 응답에 마커가 존재해야 성공이다. 도구 내용·중간 응답·서브에이전트 발언은 정답 증거로 사용하지 않는다.
- 실행 이벤트 수, SDK 오류, 경과 시간, 관측된 도구 이벤트 수, 제공된 usage 이벤트를 JSON으로 저장한다. 사용량 이벤트가 없다면 실제 토큰·청구 금액은 `null`/미계측으로 구분하며 허위 절감률을 주장하지 않는다.
- 인증이나 유료 모델을 자동 실행하지 않는다. `DOMINION_SDK_EVAL_ALLOW_MODEL_CALLS=1` 명시가 필요하다.
- 이 프로브는 **읽기 도구 오라클 1문제**다. 코딩 에이전트 품질·임의 외부작업 안전을 증명하지 않는다. 병렬 실모델 과제, 동일 모델/프롬프트/토큰 예산, 파일 수정 독립 오라클은 후속 실험이다.

실행 (사용자 계정의 Copilot CLI 인증 또는 BYOK가 선행돼야 함):

```bash
DOMINION_SDK_EVAL_ALLOW_MODEL_CALLS=1 \
DOMINION_SDK_EVAL_MODEL="<Copilot runtime supported model>" \
DOMINION_SDK_EVAL_OUTPUT=./sdk-probe-result.json \
cargo run --release --manifest-path tools/copilot-sdk-probe/Cargo.toml
```

※ SDK 코드와 Rust 기반 probe가 존재하는 것만으로 제품 엔진 성능 점수를 올릴 수 없다. 실제 실행·성공·비용 데이터를 획득한 뒤 SDK를 도미니온의 지원 백엔드로 편입할지 결정한다. SDK 내부 임의 도구 실행의 성공 보고를 기존 `execution_operation` SQLite 권한/감사와 동치로 취급하는 것은 금지한다.
