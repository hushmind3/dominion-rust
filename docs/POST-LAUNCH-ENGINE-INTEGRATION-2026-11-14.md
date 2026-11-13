# DOMINION sub3 — 출시 후 엔진·하네스·알고리즘 통합 개선 (2026-11-14)

> Scope: dominion-sub3. Source inspection and edits only; this change does not claim completed CI, a measured cost reduction, a real-model quality win, or demonstrated full-system RSI.

## 실제로 연결한 경로

### 1. 고화질 영상의 턴키 생성 경로

- 화면: web/src/pages/Video.tsx — native / comfyui / omni 선택, Gemini API 키 등록, Omni 모델과 360p/720p/1080p/4k 선택
- 설정: src/api/video.rs::save_config → AppState::save_secret("video:api") → Vault 암호화 → GET /api/video/status의 준비 상태
- 실행: POST /api/video/jobs → src/engine/video.rs::enqueue → jobs/video → video.rs::run → video/omni.rs::render
- 제공자: Gemini API Gemini Omni Flash 1.2 predictLongRunning → 작업 ID 영속 체크포인트 → operation 상태 폴링 → 신뢰할 수 있는 Google 파일 URL의 MP4 다운로드 → runtime/video/{job_id}/output.mp4 → GET /api/video/jobs/:id/content
- 비용/복구: 중복 과금 위험이 있는 모호한 제출은 needs_review로 종료하고 자동 재요청하지 않음. 다운로드는 256MiB 제한, API 키는 사용자 설정 저장소에 비밀로 보관하며 외부 리다이렉트에 전달하지 않음
- 현재 계약: 약 8초 단일 클립, 네이티브 오디오 지원, 9:16/16:9. 더 긴 다중 샷과 인물 일관성은 기존 ComfyUI cinematic 또는 향후 클라우드 편집기로 확장해야 함. 4K 지원 여부는 선택한 모델·계정의 제공자 제한에 종속적

### 2. 그룹대화의 선택적 토론

- UI: web/src/pages/Rooms.tsx에서 opt-in sparseDebate
- API: src/api/rooms.rs에서 설정 검증·저장, frozen roomSnapshot에도 포함
- 실행: engine/rooms.rs::run_locked → rooms/economy.rs::should_finish → checkpoint → finished
- 소수 참여자의 답변이 문자 단위로 매우 유사하고, 실도구 호출·의견 충돌·전원 발언 지시·직접 지목·명시 발언 상한이 없을 때만 추가 호출을 중지
- 이 동작은 semantic consensus 판정이나 품질 향상 증명은 아니다. 기존 adaptiveSpeakers, 고정 provider/model 라우팅, 전체 대화기록, 토큰 상한을 보존

### 3. 토큰 비용 관측

- src/engine/chat/continuity.rs 기존 기능: 참가자별 안정적 prefix, 내용 변화 감지, provider 힌트 협상
- src/engine/chat/runtime/tool_catalog.rs 기존 기능: 요청별 도구 스키마 선택
- src/engine/chat/provider.rs 변경: 4개 HTTP·WebSocket 스트리밍 경로의 cache-read 사용량을 gateway에 직접 전달
- src/engine/gateway.rs 변경: model.usage 감사 이벤트의 cachedPromptTokens, uncachedPromptTokens, cacheUsageReported
- 값이 없는 제공자의 캐시 적중은 null로 보존. totalTokens에서 캐시 읽기 토큰을 빼지 않는다. 캐시는 단가를 바꾸는 것이지 요청의 문맥 크기를 제거하는 것이 아니다. 절감률은 단가와 실제 hit tokens에 따라 계산해야 한다.

## 2026년 자료와 구현 원칙

- Gemini Omni Flash 1.2 Interactions: https://ai.google.dev/gemini-api/docs/omni ; https://ai.google.dev/api/interactions-api ; https://ai.google.dev/gemini-api/docs/background-execution (2026-11-14 검토)
- **2026-10-22 종료된 Veo Preview API는 제거.** https://discuss.ai.google.dev/t/veo-models-missing-from-models-list-on-a-paid-tier-1-project/199067 (Google 지원 담당 2026-11-13 확인)
- Google Gemini video overview, Gemini Omni Flash 및 Veo 워크플로: https://ai.google.dev/gemini-api/docs/video (2026-06-30 갱신)
- Anthropic stable tool schemas + cache_control: https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-use-with-prompt-caching (2026년 현행 문서)
- OpenAI prefix caching, 비용 및 provider 측정: https://developers.openai.com/api/docs/guides/prompt-caching (2026년 현행 문서)
- LASE (2026-10), selective multi-agent debate: https://arxiv.org/abs/2610.16343
- GEPA (ICLR 2026), 경험 기반 프롬프트 최적화: https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html

위 논문은 설계 참고자료다. 외부 논문 수치를 DOMINION의 실측 성능·절감률로 전용하지 않는다.

## 전체 엔진 / 하네스 / 알고리즘 통합 후속 목표

1. **토큰 비용 제어기**: 제공자별 cache_read/cache_write 요금과 SLA, 작업 난도, 실패 재시도 이력 기반으로 라우팅하되 품질 회귀를 독립 평가. 기존 rsiAdaptiveApiRouting 및 room effort 라우팅을 일반 채팅과 assistant workflows에 일관된 정책 API로 정렬
2. **모델 불필요한 결정론적 캐시**: 반복 읽기 도구는 tenant/session/permissions/resource-revision을 키에 포함하여 TTL+무효화. 동적 웹검색·개인화 답변·변경 작업의 결과를 무조건 재사용하지 않음
3. **장기 대화 압축**: 기존 room continuity/compact의 체크포인트와 원문 provenance를 통일하고, 압축 비용·검색 정확도·캐시 prefix 파손을 함께 측정
4. **영상 멀티샷 완성**: 8초 생성물을 고품질 샷 계획·레퍼런스·프레임 확장·음성·자막·오디오 정합성으로 확장. 비용 상한·중단/재개·후반 합성까지 1개 내구성 있는 작업으로 연결
5. **분해 및 UX**: engine/chat/provider.rs, chat/runtime/turn.rs, models.rs, rooms.rs 등의 다중 책임을 적절한 어댑터·정책·실행 계층으로 분해. 비밀/승인/품질 검증은 독립 경계에 유지. 카드/세션/비용 실측 표시를 동일 UX 계약으로 구성
6. **전체 RSI 계승**: 기존 API/jobs→편집→worktree→동결 평가→채택→후손 재빌드 및 메타 재진화 경로를 유지. 모델 가중치/LoRA/파인튜닝은 RSI 경로에 연결하지 않음

### 4. 선택형 모델 등급 라우팅

- `web/src/pages/Rooms.tsx → src/api/rooms.rs → src/engine/rooms/economy.rs::select_tier → src/engine/rooms.rs::run_locked`
- 사용자가 고른 모델별 `costTier: economy | frontier | balanced`를 보존·검증하고, `modelTiering` 옵션이 켜진 회의실에서만 단순 요청은 저비용, 복잡한 요청은 고성능 등급으로 라우팅합니다. 등급이 없으면 기존 모델 회전을 유지합니다.
- 실제 과금 단가는 provider에서 측정해야 하며 모델 이름이나 규모로 요금을 추정하지 않습니다. 관리자가 직접 지정한 `providerId`/`model`이 우선합니다.

## 검증 상태

2026-11-14의 변경은 소스 읽기와 GitHub 소스 수정으로만 검토했다. 컴파일·빌드·테스트·벤치마크·실 API 영상 생성·유료 모델 실행을 수행하지 않았다. 엔진 하네스/알고리즘 통합 완성도 및 80/90점 여부는 독립 구조 심사와 운영 증거가 필요하다.
