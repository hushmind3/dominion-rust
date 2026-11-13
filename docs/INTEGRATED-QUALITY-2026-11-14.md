# DOMINION 통합 완성도 — Sub1 (2026-11-14)

> 출시된 Rust 저장소의 **소스 기준 통합 구조**입니다. 실제 API 유료 호출, 컴파일, CI, 테스트 및 벤치마크는 이 수정에서 실행하지 않았습니다. 수치 절감률·영상 성공률·RSI 승률을 측정 성과로 주장하지 않습니다.

## 통합 기준: 엔진 × 하네스 × 알고리즘

| 사용자 능력 | 엔진/실제 호출 경로 | 하네스·정책 | 개선 알고리즘/경계 | 현 상태 |
|---|---|---|---|---|
| 채팅·모델·도구 | `src/api/chat.rs` → `src/engine/chat/runtime/turn.rs` → `chat/provider/stream.rs` → provider | 승인·취소·실행 기록·사용량은 기존 `runtime/`과 `gateway.rs`; 도구 실행 `tools/{dispatch,workspace,edit,network,process}.rs` | `tool_selection.rs`의 도구 스키마 선별; `continuity.rs`에서 대화 참여자/모델별 prefix 재사용 | 기존 실행 경로 유지 |
| 그룹 대화 | `src/api/rooms.rs` → `jobs.rs` → `src/engine/rooms.rs::run_locked` → `run_room_quiet` → chat/runtime | owner 메시지 1개/공통 대화 기록·참가자별 상태·명시적 권한·사용량 | `rooms.rs::effective_rounds`의 신규 비용 모드, `speaker_window`과 `room_context.rs::compile`의 관련성 기반 선택 | 이번 변경에서 비용 모드 API/UI/실행 연결 |
| 그룹 캐시 | `chat/room_context.rs` → `continuity.rs::prepare/project` → `provider/stream.rs` | provider별 지원 힌트 협상, 원문 영수증, 참가자별 working view | stable prefix/프레임 ID·변경된 evidence 재조립 | 기존 경로 활용; 캐시 할인률 미측정 |
| Runway 영상 | `web/src/pages/Video.tsx` → `src/api/video.rs::create_job` → `src/engine/video.rs::enqueue` → `jobs.rs` → `video/runway.rs::render` | Vault에 API 키, frozen job, 원격 task ID, 재제출 불확실 시 `needs_review`, MP4 저장 | 2026-06 multi-shot 레시피, 720/1080p, 자동 5컷 또는 3~5컷 사용자 샷, 오디오 설정 및 무료 사전 검증 | 이번 변경에서 완결 경로 연결 |
| LTX 영상 | 동일 진입점 → `video.rs::enqueue` → `video/ltx.rs::spec/run` | Vault `video:ltx:api`, 비동기 작업/상태/다운로드, 비용 발생 전 사양 검증 | 2026 LTX-2.5 Fast/Pro, 720p~4K, 네이티브 오디오, 모델별 허용 길이 | 챔피온 소스 비교 후 호스팅 엔진으로 통합 |
| 결과/업로드 | `video::run` → `output.mp4` → `api/video.rs::content` → `Video.tsx` 프리뷰 → YouTube 업로드 job | 사용자 지정 privacy, durable job 상태 | 성공한 결과만 출력/업로드로 연결 | 기존 제공 기능 보존 |
| RSI 후손 | `src/api/rsi.rs::start` → `jobs.rs` → `rsi_evolve.rs` → `rsi/execution.rs` → `rsi/worker.rs` → tools/worktree → frozen 평가 → 채택 | 원본 소스, 영수증 검증, 동결 proof/contract, 비채택 대조군 | `rsi/{method,programs,system,meta_controller,descendant_plan}` 계승·후손 계획/방법 재진화 | 기존 핵심 루프 유지, 이번 변경에서 가중치 학습 연결 없음 |

## 1. 그룹 대화 비용 최적화: 실제 구현

- `src/api/rooms.rs`: `costMode`는 `quality | balanced | economy`. 사용자 선택을 저장하고 검증합니다.
- `web/src/pages/Rooms.tsx::RoomSettings`: 비용 모드 선택 UI. **신규** 대화방은 `balanced`, 변경 전 방은 `quality` 기본값으로 유지합니다.
- `src/engine/rooms.rs::effective_rounds`: 사용자 메시지가 짧고 단순한 경우 반복 라운드를 줄이고 분석·구현 요청은 더 많이 유지합니다. `maxTurns > 0` 직접 지정 시 정책이 덮어쓰지 않습니다.
- 기존 `speaker_window`는 명시적 전체 호출·멘션·선택을 우선하며, `room_context.rs`의 owner 상태·실제 도구 결과·참가자별 메시지 경계는 비용 절감을 위해 삭제하지 않습니다.
- 새 정책은 **유료 모델 추가 호출 없이** Rust에서 결정합니다. 이것은 라운드 수 감소 기능이며 **입력/출력 토큰의 실제 40~80% 절감이라는 주장과 다릅니다**.
- 기존 `gateway::record_cache_usage`는 제공자 응답에서 실제 보고된 cached prompt token만 기록하며, UI의 캐시 적중률은 측정된 요청에 대해서만 나타납니다. Provider TTL·미지원 캐시 키는 공급자별로 다릅니다.

**상호작용**: `RoomSettings` → `rooms normalize` → `job roomSnapshot` → `effective_rounds` → `run_room_quiet` → `room_context::compile` → `continuity::project` → provider wire. 코스트 모드는 참가자별 identity prefix와 provider 선택 규칙을 바꾸지 않습니다.

## 2. 영상 턴키 방식: 실제 API 계약

- **Runway**: `video/runway.rs::recipe_input`에서 옵션 검증 및 사용자 정의 shot의 길이 합을 고정합니다. 지원: 5/10/15초, auto 5샷, custom 3~5샷, 720/1080p, 9:16·16:9·1:1, 선택적 audio. 공식 [2026-06 Multi-Shot 문서](https://docs.dev.runwayml.com/recipes/multi-shot-video/). 저장된 job 입력은 생성 설정의 단일 진실 공급원입니다.
- **LTX**: `video/ltx.rs::spec`에서 모델·해상도·FPS·길이 조합을 확인하고 `ltx::run`이 비동기 유료 task를 한 번 제출합니다. `ltx-2-5-pro`는 6/8/10초; `ltx-2-5-fast`는 6~20초 짝수(긴 영상은 720/1080p·24fps). 최대 4K·native audio 제공. 공식 [LTX API](https://docs.ltx.video/) 및 [LTX API Portal](https://docs.ltx.video/api-reference).
- 실행: `video::enqueue` **사전 계약 검증** → `jobs::enqueue` → `video::run` → provider 작업/상태 영수증 → 파일 크기·MP4 헤더 확인 → `public_job`·브라우저 미리보기. 원격 결제 결과가 불명확하면 중복 제출 대신 `needs_review` 상태입니다.
- 영상은 제공자 API 비용이 발생합니다. 로컬 학습·고성능 GPU를 필수 의존성으로 요구하지 않습니다. 고품질 생성은 지정한 제공자가 수행합니다.

## 3. 구조 분리와 책임

- `src/engine/rooms.rs`에서는 대형 inline 테스트를 `src/engine/rooms/{pool_tests,tests}.rs`로 옮겨 실행 오케스트레이터 가독성을 높였습니다.
- `src/engine/chat/room_context.rs`의 테스트는 `src/engine/chat/room_context/tests.rs`로 분리했습니다.
- `src/engine/video.rs` / `src/api/video.rs` 테스트는 각각 `src/engine/video/tests.rs` / `src/api/video/tests.rs`로 분리했고, 새 LTX 실행기는 `video/ltx.rs`입니다.
- `src/engine/memory.rs`의 기억 검색·에이전트 격리·회귀 테스트 4개 하위 모듈은 `src/engine/memory/{tests,memory_algorithm_tests,per_agent_memory_learning_tests,regression_memory_visibility_tests}.rs`로 옮겼습니다.
- `src/engine/{algorithms,improve,learning,jobs}.rs`, `src/api/providers.rs`, `src/db.rs`의 large inline test 모듈도 각 파일명 아래 `tests.rs`로 옮겨 프로덕션 구현과 검증 책임을 분리했습니다.
- Rust `#[cfg(test)]` 하위 모듈만 이동하였으며 production 호출 그래프·테스트 실행은 변경하지 않았습니다.

## 4. 다음 핵심 개선 경계 (이번 변경에 아직 구현되지 않은 부분)

1. **동적 모델 티어링**: 사용자 지정 모델 가격표·품질 하한·세션별 스위치 비용과 캐시 손실을 반영하는 실제 모델 라우트 선택이 더 필요합니다. 현재 그룹 `model_pool`은 참가자별 안정적 provider binding을 우선합니다.
2. **시맨틱 캐시**: 민감 정보·개별 대화/시간 의존 상태를 포함하지 않는 검증된 불변 조회에 한정해 versioned TTL과 무효화, 증거 hash를 붙일 때만 안전합니다. 자유 대화 응답을 유사 문장 하나로 건너뛰는 경로는 구현하지 않았습니다.
3. **컨텍스트 압축**: `room_context::compile`은 BM25+비용 선택과 필수 원문 보존을 사용하며, 로컬 모델 요약을 **필수**로 요구하지 않습니다. 동적 요약이 cache prefix를 깨는 비용을 함께 평가해야 합니다.
4. **동영상 품질 보증**: 현재 생성물 MP4 유효성 검사는 있지만 인물 일관성, 컷간 움직임, 음향 동기, 실제 프레임 품질 자동 판정과 수동 승인/재생성 UX는 별도 개발 과제입니다.
5. **운영 완성도**: `chat/runtime/turn.rs`, `rooms.rs`, `db.rs`의 대형 책임과 제공자별 오류 모델, UI 별도 상태 관리 및 유료 작업 과금 중복 방지 영수증 검증은 추가 분해 대상입니다.
6. **RSI 승리 증명**: 소스 수준 동결 평가·후손 계승 경로와 실제 4세대 유료 모델 완주는 별도입니다. 학습/LoRA/fine-tuning을 RSI 경로에 연결하지 않습니다.

## 참고 (2026 공개/업데이트)

- [Runway multi-shot 2026-06](https://docs.dev.runwayml.com/recipes/multi-shot-video/) — 버전 고정된 API 계약.
- [OpenAI API prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching) — exact prefix, tool schema, provider hit telemetry.
- [Don't Break the Cache (2026-01-09)](https://arxiv.org/abs/2601.06007) — 장기 에이전트 캐시 전략 측정.
- [TOPAS (2026-08-26)](https://arxiv.org/abs/2608.25523) — multi-agent prefix-state scheduling과 공정성.
- [MAS-PromptBench 2026-10-22 갱신](https://github.com/juyangbai/MAS-PromptBench) — 멀티에이전트의 역할/통신 포맷/팀 크기 평가.

**평가 시의 원칙**: 선언된 모듈보다 실제 UI/API→Job→엔진→provider/도구→영수증→상태→후속 동작을 우선 추적하고, 명시된 미완료를 성취로 세지 않습니다. 새 코드의 정적 경계 검토만 수행했습니다.
