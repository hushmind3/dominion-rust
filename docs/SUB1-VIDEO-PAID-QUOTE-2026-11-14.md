# Sub1 2026-11-14 — LTX-2.5 영상 실제 과금 사전통제

## 심사 지적과 변경

기존 장편 영상은 최대 6개 샷, MP4/SHA-256 영수증, 기술검수, 중단시 미확정 유료 호출 보류, 선택적 재생성까지 연결됐다. 하지만 사용자 동의는 *API 호출 수*에 머물렀고 LTX 모델·해상도·총 과금초수에 따른 금액 상한이 없었다. 영상 품질·실측 결제금액이 증명됐다는 주장은 하지 않는다.

### 코드에서 실제로 실행되는 경로

1. 프론트 `web/src/pages/Video.tsx`: LTX 모델·해상도·길이·오디오·Film 설정을 고른 후 **견적 확인**으로 `POST /api/video/quote` 실행, 서버의 참고 원화 견적 및 과금 초수·호출수 확인, 별도 금액 승인 체크박스.
2. `src/api/video.rs::quote_job` → `src/engine/video.rs::paid_quote` → `video/ltx.rs::spec` 및 `video/film.rs::split_duration/max_clip` (유효한 샷/동작만) → `video/pricing.rs::quote` (공식 단가 스냅샷, 정수 달러 센트, 참고 환율). 이 조회에서 유료 API 호출·작업 등록은 발생하지 않는다.
3. `src/engine/video.rs::enqueue`는 사용자 값 대신 **서버가 견적을 재계산**, `approvedPaidQuote=true`, `maxUsdCents >= totalUsdCents`, 설정·요금표에 묶인 `quoteDigest` 일치를 모두 확인한 후 영속 작업을 만든다. 승인 금액, 견적 및 스냅샷은 `job.input`에 기록된다.
4. 부분 재생성 UI `web/src/pages/video/FilmRetake.tsx`는 `/api/video/jobs/:id/scenes`에서 선택 샷별 새로운 견적을 받는다. `video/film.rs::retake_quote`는 **해당 샷 + 미생성 샷**만 신규 과금초수에 포함하고 이미 SHA 검증된 다른 샷은 재구매 비용에 포함하지 않는다. `video.rs::enqueue_retake`는 금액 및 명세 동의를 다시 검사한다.
5. LTX 실제 생성기는 기존 `ltx::run` 및 `film::run`을 그대로 쓰며 비멱등적 제출을 자동 재시도하지 않는 증거 보호가 계속 적용된다. 출력과 영수증을 실제 영상 품질 보증으로 오인하지 않는다.

## 2026-11-14 공개 가격 스냅샷

출처: [LTX API Pricing](https://docs.ltx.io/pricing) — `v2/text-to-video`, LTX-2.5.

| 모델 | 720p | 1080p | 1440p | 4K |
|---|---:|---:|---:|---:|
| LTX-2.5 Fast, USD/초 | $0.09 | $0.13 | $0.19 | $0.30 |
| LTX-2.5 Pro, USD/초 | $0.12 | $0.17 | $0.25 | $0.39 |

견적 로직은 소수 부동소수점 반올림을 사용하지 않는다. USD cents/second × 신규 과금 초수 → 총 센트 → 참고 원화 금액 정수 올림. 환율은 **2026-11-13 참조값 USD 1 = KRW 1,336**으로 코드에 명시되어 있으며 실시간 환율이 아니다. 예: Pro 1080p 영상 22초, 10+6+6초 3개 장면이면 미화 $3.74, 참조 환산 **4,997원**이다. 이는 세금, 환율 변화, API 요금표 변경, 승인된 외부 제공자 청구액, 모델 계획 API 호출을 포함하지 않는 *예상 목록가*다. 제공자가 공시 요금을 변경하면 가격표를 갱신해야 하며, 현재 코드는 런타임에 실시간 요금표를 직접 조회하지 않는다.

Runway 기존 경로는 가격 계약이 별개여서 LTX 계산을 전용하지 않았다. Runway는 다음 별도 견적·승인 통제 개발 대상이다. ComfyUI/Native는 이 호스티드 LTX 견적의 대상이 아니다.

## 회귀 테스트

- `video/pricing.rs`: 2.5 Fast/Pro 요금·KRW 올림·샷 순서/오디오까지 서명에 반영, 과소 승인/잘못된 지문 거부.
- `api/video/tests.rs`: API 키가 있어도 명시적 승인·정확한 지문·충분한 상한이 없으면 유료 LTX 작업이 `queued`에 들어갈 수 없음.
- `video/film/tests.rs`: 기존 3장면 [10,6,6]에서 10초 선택 샷과 미생성 6초 샷만 재결제 견적 계산, SHA 검증된 6초 샷은 무료 재사용, 승인 상한 미달 거부.
- 모의/정적 단위 테스트와 CI 결과를 실제 제공자 과금액·시청 품질·GPU 원가에 대한 실측으로 표기하지 않는다.

## 다음 연결

실제 청구 금액은 공급자 billing API 영수증 또는 사용자 업로드 원장을 통해 대조해야 한다. 외부 호출 결과가 미확정인 경우 기존 `needs_review` 차단을 유지한다. 품질 측정은 FFprobe 기술 결과 외에 영상 프레임/음성 연속성·사람 평가가 필요하다.
