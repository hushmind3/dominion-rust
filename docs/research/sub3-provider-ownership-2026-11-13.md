# 서브3 — API 모델 제공자 모듈 경계 (2026-11-13)

`src/api/providers.rs`의 기존 공개 HTTP 핸들러를 보존하며 7개 하위 모듈로 분리했다: `auth_sources`, `discovery`, `cli_credentials`, `codex_login`, `provider_store`, `connection_test`, `model_list`. OAuth 인증, 무료 모델 목록, 저장소, 실시간 모델 카탈로그를 별도의 수정 가능한 단위로 구분했다.

후손 `rsi_system` 설계도의 `models` 대표 소스에 새 API 파일을 모두 등록하고 `source_role`도 갱신했다. 소스 전체 해시와 분류를 단위 회귀 검증 대상으로 추가했다. 기존 provider ID, 로그인 세션, 모델 선택, 예산·평가 경계의 로직 및 스키마는 그대로 보존했다.

2026 참고: OpenAI Agents SDK의 하네스·컴퓨트 경계 — https://openai.com/index/the-next-evolution-of-the-agents-sdk/ (2026-04-15), HyperAgents의 진화 가능한 메타 절차 — https://ai.meta.com/research/publications/hyperagents/ (2026-03-24).

빌드·컴파일·CI·실모델 호출 없이 정적 코드 이전 및 연결성만 점검했다.
