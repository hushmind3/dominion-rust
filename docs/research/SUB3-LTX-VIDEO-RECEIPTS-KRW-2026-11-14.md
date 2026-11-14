# 서브3 · LTX 2.5 턴키 영상·장면 재생성·원화 견적 (2026-11-14)

## 심판 56점 재검토에 따른 실제 실행 경로

기존 `native` 자막 카드, ComfyUI 멀티샷, Gemini Omni 영상 엔진을 보존하면서 새 `ltx` V2 비동기 제공자와 클립별 영수증·부분 재생성 경로를 연결한다.

1. `web/src/pages/Video.tsx` → 엔진/모델/해상도/길이/장면 선택 → `POST /api/video/quote`에서 **원화 참고 견적** 조회. 금액·장면 수 확인/동의 없이는 생성 버튼이 비활성화된다.
2. `src/api/video.rs::create_job` → `engine/video.rs::enqueue`가 모델, 2~6장면 동결 스펙, 예정 유료 호출 수, `approvedPaidQuote`, `maxEstimatedKrw`를 검증하고 영속 job을 등록한다. API를 직접 호출해도 승인/상한 검사를 우회할 수 없다.
3. `engine/video.rs::run`은 실제 유료 제출 직전에 동일 모델·기간·요금표에 따라 재견적한다. 요금표 버전/금액이 승인과 다르면 외부 호출 없이 검토 대기로 멈춘다.
4. `engine/video/ltx.rs::render`는 `https://api.ltx.io/v2/text-to-video`에 제출하고 원격 ID를 영속 체크포인트에 남긴 다음 상태를 폴링한다. 결과를 다시 제출하지 않고, HTTPS 허용 호스트에서 MP4를 내려받는다. `video:ltx-api` 암호화 키는 Gemini의 `video:api`와 독립이다.
5. `engine/video/film.rs::run`은 길이에 따른 클립 분할, 동일 인물/의상/배경 프롬프트 앵커, 클립별 SHA-256 + `receipt.json` + FFprobe 기술 검수, FFmpeg 합치기를 수행한다. 오류나 불확실한 유료 외부 결과는 자동 재구매하지 않고 검토 대기로 보낸다.
6. `GET /api/video/jobs/:id/scenes` → `FilmRetake.tsx`는 원본 클립의 실제 SHA, 새로 결제될 클립 수 및 참고 원화 견적을 표시한다. `POST /api/video/jobs/:id/retake`는 `expectedSha256`, 명시 동의, 유료 호출 횟수와 원화 상한을 서버에서 다시 검사해 새 고정 ID의 job을 만든다. 정상 클립은 검증 영수증과 함께 재사용한다. 브라우저의 중복 클릭/재요청은 같은 결정적 job ID에 대응한다.
7. 완성본은 `/api/video/jobs/:id/content`에서 재생/다운로드하고 기존 YouTube 업로드 흐름을 재사용한다.

## 2026-11-14 공개 요금표·환율 스냅샷

공식 [LTX-2.5 API 요금표](https://docs.ltx.io/pricing/)의 text-to-video(음성 포함) 단가를 사용한다. Fast 720p/1080p/1440p/4K는 각각 **초당 약 120원·174원·254원·401원**, Pro는 **초당 약 160원·227원·334원·521원**이다. 계산은 공급자 원화 청구액이 아니라 2026-11-13 참고 환율 **1달러=약 1,335원**을 적용한 기획용 환산값이며, 실제 카드 환율·부가세·수수료·공급자 과금/환불은 미포함한다. 환율 참고: [Wise 2026-11-13](https://wise.com/us/currency-converter/usd-to-krw-rate/history/13-11-2026).

- **견적 ≠ 청구 영수증**: `costQuote.estimatedKrw`는 유료 호출의 예상 상한 참고값이다. `actualBilledKrw`는 공급자 결제 정보를 조회하지 않았으므로 `null`이다.
- **초과 제출 차단의 한계**: 동일 소스의 요금표가 변경·배포되면 서버 사전 검사로 중단한다. 제3자가 공지 없이 요금을 바꾸거나 FX/카드 환율이 변동하는 경우 강제 결제 상한을 기술적으로 보장하지 않는다. 따라서 사용자-facing 문구는 '예상 견적 승인'이지 '최종 청구 보증'이 아니다.
- **기존 엔진 보존**: `native`, `comfyui`, `omni`는 기존 엔진/키/검수 경로가 그대로 존재한다.
- **가중치 학습 없음**: 이 영상 파이프라인은 정적 제공자 API, 휴리스틱 샷 계획, SHA/FFprobe/FFmpeg로 구성된다.

## 검증·남은 증거

- `engine/video/ltx.rs`의 V2 모델/길이/형식/다운로드 URL 검증 테스트 및 `engine/video/film/tests.rs`의 클립 분해·영수증 재사용·SHA 재생성 계약 테스트.
- `engine/video/pricing.rs`의 모델·화질별 참고 요금·동결 환율·원화 합계 및 유효하지 않은 장면 분할 테스트.
- `scripts/sub3-connection-hub.test.mjs`의 화면→API→결제 승인/클립 재생성 경로 정적 회귀 테스트.
- **실모델 영상 품질, 공급자 실청구액, 실제 API 원격 작업 재개, FFmpeg의 모든 코덱 간 전환**은 외부 유료 생성 없이는 입증하지 않았다. 최신 GitHub CI가 컴파일/단위 테스트를 확인해야 하며, 실영상 평가와 청구 내역은 별도로 측정한다.

참고: [LTX 공식 V2 마이그레이션](https://docs.ltx.io/migrate-v1-to-v2/), [2026-11-14 LTX 2.5 모델](https://docs.ltx.io/models/ltx-2-5).
