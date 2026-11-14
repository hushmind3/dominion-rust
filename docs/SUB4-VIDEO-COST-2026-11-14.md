# 서브4 영상 유료 호출 견적·지출 노출 증거 (2026-11-14)

## 심판 76점 이후 직접 개선한 병목

실영상 시청 품질, 실제 생성비 및 재시도 비용 측정 부재를 분리하여 처리한다. FFprobe 기술 검수 통과를 시청 품질로 포장하지 않는다.

**새 경로**

1. `VideoPage`에서 영상 길이/모델 설정에 따라 `POST /api/video/quote`를 호출한다.
2. `api/video.rs::price_quote` → `engine/video.rs::paid_quote` → `engine/video/runway.rs::quote`에서 서버가 요금표·해상도·오디오·길이와 클립 수로 견적을 계산한다.
3. 프론트는 **원화 추정 견적**, 모델, 유료 클립 수, 세금 별도 안내, 별도 체크박스를 제시한다. 승인 전 생성 버튼은 비활성화한다.
4. `POST /api/video/jobs` → `video::enqueue`는 `approvedPaidQuote=true`와 `maxCreditsMicro`가 서버 견적 이상인지 검사한다. 기존 클라이언트가 승인 없이 유료 요청을 보내면 실패한다.
5. 실제 `runway::run`에서 **유료 API 호출 직전** 날짜 기준 요금을 재계산한다. 대기 중 11월 30일 프로모션이 끝나 금액이 커졌다면 유료 제출 없이 `needs_review`로 진행을 멈춘다.
6. 원격 요청 직전 영속 `videoState`에 잠재 과금 노출, 시도 횟수, 클립 번호를 기록한다. 네트워크 타임아웃·불명확한 결과는 원래 있던 **자동 재제출 차단** 정책을 유지한다.
7. `public_job`과 `VideoPage`는 작업 상태가 실패하더라도 원래 견적과 **제출 시도 기준 노출액**을 구분하여 보여준다.

## 2026-11-13 공급자 가격 스냅샷

모든 원가 계산은 Runway **Dev** 크레딧 기준. 개발자 API 크레딧과 일반 Kling 앱 구독 크레딧은 다르다.

| 설정 | Dev credits / 출력 1초 | 내부 정밀도 |
| --- | ---: | --- |
| Kling 3.0 Standard, 무음 | 9 | 9,000,000 microcredit |
| Kling 3.0 Standard, 사운드 | 13 | 13,000,000 |
| Kling 3.0 Pro, 무음 | 12 | 12,000,000 |
| Kling 3.0 Pro, 사운드 | 17 | 17,000,000 |
| Kling 3.0 4K | 43 | 43,000,000 |
| Kling 4.0 **1080p** (2026-11-30까지 20% 할인) | 18.8 | 18,800,000 |
| Kling 4.0 **1080p** (2026-12-01부터 정가) | 23.5 | 23,500,000 |

현재 엔진의 Kling 4.0 요청은 text-to-video에서 `1920:1080`, 이미지로 이어지는 다음 클립은 `auto_1080p`로 작동하므로 720p 할인가 12.56 크레딧/초를 적용하면 **위험한 과소 견적**이 된다. 프레임 참조 이미지 자체에는 입력 비디오 추가 요율이 없다.

- Runway Dev Kling 3.0: https://dev.runwayml.com/learn/kling-3-0-api
- Runway Dev Kling 4.0: https://dev.runwayml.com/learn/kling-4-0-api
- Runway Dev API price guide: https://docs.dev.runwayml.com/guides/pricing/
- **원화 표시는** 크레딧당 미화 0.01달러, 2026-11-13 참고 환율 1달러≈1,336원으로 변환하고 `referenceFxKrwPerUsd`, `fxAsOf`를 응답에 명시한다. 실제 결제 환율, 세금, 카드 수수료와 제공자 가격 개정은 제외한다.

## 정확히 입증하는 것과 못 하는 것

- **입증**: 어떤 모델·설정을 유료 API에 제출하려는지 계산한 **예상 크레딧**, 명시 승인 여부, 원격 HTTP 요청 전에 체크포인트에 남긴 예상 노출 금액, 유료 중복 재시도 차단 코드.
- **아직 입증하지 않는 것**: 계좌에서 차감된 **실제** 크레딧, 제공자 실패 후 환불, 부가세·수수료, 실제 영상 시청·미학 품질, 모델 요금이 공급자에서 사후에 변경된 경우의 차액. 이들은 공급자의 사용량/인보이스 및 실제 영상 평가가 필요하다.
- 크레딧 승인 한도는 *현재 공개 가격 스냅샷에 따른 제출 차단 기준*이며 제3자 공급자가 실제 청구하는 금액을 보증하는 결제 한도는 아니다. 계획용 텍스트 모델 호출 비용은 별도 모델 사용량 기록을 참조해야 한다.
- ComfyUI GPU·로컬 전력비는 API 크레딧과 단위가 다르므로 이 견적에 합산하지 않는다.

## 회귀 확인

- `engine/video/runway.rs::pricing_quote_tests`: Standard/Pro/4K/Kling4 사운드별 가격, 프로모션 종료, 길이 검증.
- `api/video.rs::tests::runway_paid_jobs_need_exact_owner_approval_and_fresh_quote`: 승인 누락·상한 미달 거부, 성공 작업의 견적 동결, 더 비싼 모델로 바꾼 뒤 오래된 한도 거부.
- `tests/unit/video_paid_resubmission_tests.rs`: 이전에 기록한 원격 유료 작업 재전송 차단.

**검증 상태**: 코드·테스트 커밋 반영. Rust·프론트 타입/빌드 및 실제 API 외부 작업 결과는 CI나 별도 실제 테스트 결과로만 판정한다.
