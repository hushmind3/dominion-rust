# 서브3 — 재귀 개선 가능 실행기 모듈 분해 (2026-11-13)

## 실제 변경
- `src/engine/chat/runtime.rs`는 기존 4개 공개 호출 진입점과 공용 실행 정책만 보유하는 얇은 파사드로 축소했다.
- `src/engine/chat/runtime/{intent,turn,adaptation,tool_catalog,execution_program,dispatch,planning,finalization}.rs`가 기존 실행 알고리즘과 저장 증거 스키마를 각각 보존하면서 책임별 컴파일 모듈이 됐다.
- 기존 `chat::runtime` 외부 공개 함수와 상대 경로, `chat_runtime.tests.rs` 테스트 연결을 유지했다. 내부 조력 함수는 가장 좁은 부모 모듈 가시성으로 이전했다.
- `src/engine/rsi_system.rs::COMPONENTS`의 하네스 대표 소스에 8개 모듈을 추가했다. 동적으로 전체 소스를 해시하는 것 외에 **후손이 전달받는 작은 개선 대상 목록**에서도 실행기 내부를 직접 발견할 수 있다.
- `tests/support/engine/rsi_system.tests.rs`는 각 개선 가능 모듈을 하네스 대표 소스로 노출하는지, 부분 소스 수정이 세대 소스 해시를 바꾸는지 검사한다.

## 2026 연구 참고
- Meta HyperAgents (2026-03): 발전시키는 프로그램과 개선 기법 자체를 수정 가능한 구조로 취급하는 원리 — https://ai.meta.com/research/publications/hyperagents/
- Recuris (2026-08): 실패 증거를 책임 단위의 변경과 재검증에 연결하는 메모리·하네스 경계 — https://arxiv.org/abs/2608.24876
- AIDE² (2026-09): 실제 채택한 소스 개정이 다음 세대 실행기를 구성하고 독립 작업에서 이득을 재평가하는 구조 — https://arxiv.org/abs/2609.26457

## 범위·검증
현재 코드는 알고리즘을 대체하거나 새 RSI 성능을 검증한 결과가 아니다. 구조 분해와 후손 계획 연결만 변경했으며 기준 모델 가중치·학습 경로·평가·채택 권한은 건드리지 않았다. 지시대로 빌드·컴파일·테스트·CI·유료 모델 실행은 수행하지 않았다. 실제 심판 점수는 별도 재채점으로만 확정할 수 있다.
