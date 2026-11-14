# Sub4 2026-11-15: 실제 프레임 영상 QA · 내구 이벤트 복구 · TypeScript SDK

## 근거와 우선순위

이 문서는 기능 자체가 아니라 **실제 운영 경로와 재현 가능한 검증**을 기록한다.
2026년 공개 소스 조사:
- `https://github.com/Well-Made/ComfyUI-Wan-SVI2Pro-FLF`:
  Wan 2.2의 first/last-frame 및 장면 연결 워크플로(2026-02). 참고 구현이며
  본 변경에서 해당 ComfyUI 커스텀 노드를 자동으로 설치하지 않는다.
- `https://github.com/pjazdzyk/sns-comfy-ui-workflows`:
  2026년 공개 ComfyUI 모델별 영상·인물·후처리 실증 워크플로.
  사용자 로컬 GPU가 충분하지 않으면 이를 필수 실행 경로로 사용하지 않는다.
- `https://github.com/langchain-ai/agent-protocol/blob/main/streaming/README.md`:
  2026년 에이전트 스트리밍의 sequence-based checkpoint/reconnect 사례.
  DOMINION에는 자체 Rust SQLite 이벤트 로그와 자체 이벤트 형식을 유지한다.
- `https://github.com/garrytan/g-harness-evals`:
  작업별 비용·완수율을 함께 보는 공개 harness 비교. 외부 수치를
  DOMINION의 달성 점수로 전용하지 않는다.

## 1. 실제 프레임 결함 검사 (엔진 + UI)

```
VideoPage (checkbox: visualQualityGate)
  POST /api/video/jobs
  video::enqueue -> job.input.visualQualityGate (immutable once queued)
  jobs::run -> cinematic::render
    storyboard / shot-specific frozen prompt + seed
    possible reuse -> quality::inspect_shot (only if passes current gate)
    missing/failed -> comfy::render (existing non-idempotent API semantics)
    -> quality::inspect_clip (ffprobe duration/shape/streams)
    -> visual_scan::inspect (FFmpeg decode full video)
       blackdetect, freezedetect -> black/freeze fractions, elapsedMs
    -> retry only this shot once when badShotRetries=1
    -> shotResults[].qualityReceipt.contentDefectScan
  ffconcat -> final FFmpeg 1080p assembly -> ffprobe final receipt
```

실제 결함 판정은 보수적이다.
- 거의 검은 화면이 **클립 길이 75% 이상, 최소 3초** 이어진 경우
- 고정 프레임이 **클립 길이 92% 이상, 최소 5초** 이어진 경우
- FFmpeg 프레임 디코딩 오류
- 정상적인 짧은 페이드·정적인 구도를 가능한 한 배제하지 않도록 문턱 설정

기존 `badShotRetries`가 0이면 결함을 발견해도 비용이 발생하는
재제출을 허용하지 않고 실패로 처리한다. 기존 유료 Runway의
승인·견적·불명확한 원격 제출 재시도 차단은 이 변경으로 수정하지 않는다.
`visualQualityGate`는 ComfyUI cinematic 경로에서만 적용된다.

검증: `visual_scan.rs` 이벤트 파싱 유닛 테스트, 실제 FFmpeg로 생성한
6초 흑색/움직임 합성 영상 테스트. FFmpeg가 설치되지 않은 환경에서
합성 영상 테스트는 건너뛴다. 영상 미학·연속 인물 정체성은
FFmpeg 탐지로 입증되지 않는다. GPU 전력·시간당 요금은 미측정이다.

## 2. 500개 초과 이벤트 복구와 병렬 순서

기존 `api/system.rs::events`는 `Bus::replay(after,500)` 호출 1회로
대기 기록을 받았다. 예를 들어 900건이 SQL에 살아 있어도 뒤 400건을
스트리밍 연결 중 알리지 못할 수 있다.

새 구조:
1. `src/events.rs::Bus::emit`이 mutex 안에서 seq 할당 → SQLite append →
   in-process broadcast를 **동일 순서**로 수행한다.
2. SSE 핸들러가 SQL 조회보다 먼저 broadcast를 구독한다.
3. `Bus::replay_available`가 500개 단위로 **보존된 모든 페이지**를 읽는다.
4. 재생 데이터와 실시간 데이터는 `seq` 기준으로 중복 제외한다.
5. `Lagged`에서도 전체 재생을 수행하고, 보존 창이 이미 지난 경우
   `{type:"replay_gap",requiresStateRefresh:true}`를 출력한다.
6. `/api/events` 재생 이력은 유한하다(기존 SQLite 보존 정책 유지).
   영구 이력으로 광고하지 않는다.

테스트: `events.rs` 900개 초과 재생, 보존기간 경과 후 실제 첫 이벤트,
병렬 8개 writer × 30개 이벤트에서 DB 시퀀스와 수신 순서 비교.

## 3. TypeScript SDK의 읽기 전용 스트림 복구

`sdk/typescript/src/index.ts::DominionSDK.watchEventsResilient`
- 최종 **관측** seq만 기억하며 `GET /api/events?after=seq`에 재연결
- 중복 seq 제거, 인증 실패 시 즉시 중단, 일시적 실패는 bounded exponential delay
- `AbortSignal` 종료 지원, `replay_gap`을 애플리케이션에 투명 전달
- `POST /api/chat`, `goals` 작성, 승인 등 **side-effecting 요청 재시도 금지**

테스트: `sdk/typescript/tests/sdk.test.mjs`에서 재연결 커서,
overlap 중복, 보존 초과 신호, abort, retry 제한, 401 중단.

## 4. 룸 대화 비용: 기준점 고정 및 재개 전 선검사

기존 룸 실행기는 세션 토큰을 **모델 발언이 끝난 뒤** 검사했고,
재실행 시 새 사용량 기준점을 취했다. 현재는 최초 작업의
`job.input.usageBaselineTokens`와 전체 `usageBaseline`를 영속화하고,
안전하게 이어진 작업에서도 매 모델 발언 **직전** 같은 한도를 검사한다.
`roomUsage`는 최종 세션 사용량과 동결된 기준점의 차이를 반환한다.
입력 토큰·출력 토큰·보고된 캐시 사용량·캐시 관측 호출 수를 포함하고,
이전 버전 작업처럼 원시 기준 데이터가 없으면 null로 나타낸다.
`web/src/pages/Rooms.tsx`는 전체 이력 누적과 이번 대화의 관측 사용량을
구분한다. 실제 원화 청구액과 공급자 미보고 캐시 적중은 추정하지 않는다.
테스트는 `tests/unit/engine_rooms_economy_tests.rs`에 있다.

## 5. 목표 대기 SDK의 중복 실행 방지

`DominionGoal.waitForTerminal()`은 이벤트 관찰만을 자동화한다.
최초와 이후 목표 관련 이벤트마다 `GET /api/goals/:id`의 정본을
조회해 완료 상태를 판정한다. 오래된 SSE 기록의 완료 메시지가
현재 실행 중인 목표를 완료로 잘못 처리하지 않으며,
보존 기간 초과 `replay_gap`에도 현재 상태를 재조회한다.
`AbortSignal`은 SDK 관찰만 끝내고 실제 목표 실행은 별도 `cancel()`
권한 요청이 있어야 중단한다. 단위 테스트는 TypeScript SDK 자체
테스트 파일에 있다.

## 입증 경계

기존 코드의 실행 경로와 본 변경의 단위/FFmpeg 합성 테스트를 따로 기록한다.
CI 성공은 실제 LLM/영상 생성 완수율이나 사용자 GPU·API 청구액을 입증하지 않는다.
500-task SWE-bench Verified 공식 판정, 영화 시청 평가와 운영 청구 자료는 별도.
