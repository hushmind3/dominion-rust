# DOMINION · 2026-11-14 통합 업그레이드 소스 경로

## 엔진 → 하네스 → 알고리즘 통합 체크

- 진입점: `src/main.rs` → `src/api/rooms.rs` / `src/api/chat.rs` / `src/api/video.rs` / `src/api/rsi.rs`
- 공통 소유권·실행: `src/engine/jobs.rs`, `src/engine/chat/runtime/turn.rs`, `src/engine/chat/continuity.rs`
- 그룹: `rooms/routing.rs::plan_speakers` → `speaker_window` → `rooms.rs::run_locked` → `routing::tiered_model_pool` → `run_room_quiet` → `gateway::record_usage_tokens`. 정책은 저장된 방 스냅샷에서 읽고, 사용자의 명시적 선택이 모든 계층 라우팅보다 우선합니다.
- 영상: `video.rs::enqueue` → `video/film.rs::spec` / `video/director.rs::shot_prompt` → `jobs::enqueue` → `video.rs::run` → `video/film.rs::run` → `video/ltx.rs::run` → `FFmpeg`. 요청별 과금 횟수 확인, 개별 제출 불확실성 기록, 완료 클립 재사용.
- RSI: `api/rsi.rs` → `jobs.rs` → `rsi/execution/campaign.rs` → `rsi/planner/descendant.rs` → `rsi/execution/worker.rs` → `rsi_worktree.rs` → `rsi_method_generalization.rs`, 채택된 소스와 개선법을 다음 세대가 재사용합니다. 모델 가중치를 학습시키지 않습니다.

## 2026년 근거 및 차이

- LTX 공식 2026년 8~11월 Changelog: https://docs.ltx.io/api-changelog
- LTX 2.5 고화질·네이티브 멀티쇼트·프레임별 지원 길이: https://docs.ltx.io/models/ltx-2-5/
- `video/ltx.rs::maximum_duration` 규칙을 `video/film.rs::spec`와 단일 클립 경로에서 같이 참조해 24/25/48/50FPS의 720p~4K 공식 제한을 실행 전 확인합니다.
- ICML 2026 LASE 선택적 에이전트 논의: https://proceedings.mlr.press/v306/yoa26a.html
- ICML 2026 SALE 소형/대형 모델 전략 경매: https://proceedings.mlr.press/v306/alazraki26a.html
- 2026년 프롬프트 캐시 운영 지침: https://developers.openai.com/api/docs/guides/prompt-caching

## 출시 이후 연결 확인 대상

1. LTX 작업 종료/중단/복귀 및 클립별 출력 파일 무결성, 동일 스펙 MP4 오디오 결합·플레이어 재생
2. 새로운 적응형 방과 이전 방의 호출 경로 비교, 사용자 전체 토론 명령의 우선권
3. 프롬프트 캐시 히트율, 실수금/실토큰 기반 비용, 저비용/고성능 티어의 실제 성능 및 품질
4. RSI 채택 → 독립 평가 → 후손 재컴파일·재계획·방법 계승의 다세대 검증
5. 영상 장편 제작의 쇼트 간 인물/조명 일관성은 프롬프트 유도이며 프레임 참조 기반의 완전 보장이 아닙니다.

이번 변경은 구조와 실제 호출 경로의 소스 통합입니다. CI·빌드·실모델·영상 벤치마크는 실행하지 않았으며, 절감률·품질 상승을 실측 성과로 주장하지 않습니다.
