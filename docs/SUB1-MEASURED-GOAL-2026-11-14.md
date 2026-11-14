# 서브1 — 2026-11-14 실측 기반 Goal 성과 검증

이번 재채점에서 구현 연결과 실제 운영 결과를 구분한 기준을 적용한다. 기존 모의 모델 E2E, Rust 단위 테스트, 문서, 메뉴는 **실모델 작업 성공률이 아니다**. 완수율을 주장하려면 실제 모델이 수정한 외부 파일을 독립 채점해야 한다.

## 현재 실제 호출 경로

- 최초 목표 등록: `POST /api/goals` → `api/goals.rs::create_goal` → `Store::transaction([goal,job])`. 최초 등록도 목표↔작업 ID를 1개의 SQLite commit으로 보존. 과거에는 목표 등록, Job 생성, Job ID 연결 3개 개별 DB 쓰기였다.
- 결과 발생: `engine/goals.rs` → 실제 provider `run_quiet_outcome` → `execution_operation` 영속 증거 → Rust `acceptance_gaps` → 독립 모델 리뷰 → `goal_metrics::final_report`. "completed"라고만 적힌 AI 메시지는 완료 실측으로 인정하지 않는다.
- 관측 API: `GET /api/goals/metrics` → `goal_metrics::aggregate`. 현재 저장된 실제 Goal의 터미널 분모(실패·needs_review·취소 포함), 독립된 영수증에 의한 검증 완료 수, 사용량이 있는 세션의 LLM 호출/토큰, 캐시 보고된 호출/읽기 토큰을 집계한다. 결과를 모르면 null. 제공자 실청구 원화는 청구서가 없으므로 항상 null.
- **로컬 실모델 실행**: `.github/workflows/sub1-measured-goal-eval.yml` → `scripts/sub1-measured-goal-eval.mjs` → 실제 DOMINION API의 생성/Job/Goal 조회. GitHub Actions 자체 CPU에서 Ollama + Qwen3 4B Instruct를 실행하므로 유료 텍스트 API 키가 필요 없다. 공개 저장소 CI 분과 다운로드/전력 원가는 이 실험의 원화 청구 계측에 포함하지 않는다.

## 독립 정답 검사 방식

1. 별도 임시 `BENCH_ROOT`에 data/workspace를 만들고 benchmark sentinel과 `DOMINION_BENCHMARK=1`의 identity를 1:1로 일치시킨다. 일반 운영 서버와 경로가 다르면 실행을 거부한다.
2. 고정 작업 2개에 대해 실제 모델이 파일·셸 아닌 소스/워크스페이스 도구로 수행한다. **다중 파일 로케일 수정**과 **실행 가능한 JavaScript 버그 수정**이다. 고정 입력 파일과 변경된 출력의 SHA를 저장한다.
3. 모델에게 전달한 명시적 성공 조건 외에, 모델 작업공간 밖의 실행기에서 JSON 불변값·ESM 모듈 export·예외 입력 등 숨겨진 검사를 실행한다. 스스로 보고한 완료는 검사하지 않는다.
4. 각 사례는 `oraclePass`와 `engineVerified`를 별개로 기록한다. 서버가 성공으로 판단했어도 외부 오라클이 실패하면 **성공으로 보고하지 않는다**. 제한시간 초과·모델 오류도 분모에 포함한다.
5. 파일 해시, 실제 Goal/Job ID, terminal status, LLM 사용량/estimatedCalls, 검증되지 않은 청구 비용, Git commit SHA, Ollama 모델 digest, 환경 정보를 `live-goals.json`에 남긴다. 작업 실패에도 레코드가 남는다.

### 정확한 범위

- 이 테스트는 **실제 모델 + 실제 DOMINION 실행 엔진**의 소형 2개 다중 파일 작업이다. 2개 결과를 SWE-bench, 장기 프로젝트 완수율, 상용 모델 품질, 일반 에이전트 성공률로 확장 해석하지 않는다.
- 모델의 체감 지능, 실제 토큰 절감률, provider 결제 총원가, RSI의 신규 모델 주도 개선, 영화 시청 품질은 이 테스트에서 측정하지 않는다.
- 재현 가능한 여러 샘플·타 브랜치 동일 모델 비교, 실프로젝트의 독립 수정·비공개 테스트는 별도 평가의 다음 단계다.
- 참고: 2026년 [SWE-Bench Pro V2](https://labs.scale.com/leaderboard/swe_bench_pro_public_v2?tab=full)는 clean image에서 patch를 재평가하고, 2026년 [SWE-Bench Pro Verified](https://arxiv.org/abs/2609.08149)는 유출된 gold patch와 정답 오염에 의한 과대평가를 경고한다. 이 방식을 그대로 복제했다고 주장하지 않는다.

## 영상의 실행 직전 방어

- 유료 작업 등록: `video::enqueue`에서 LTX-2.5 요금 스냅샷과 사용자가 승인한 지문·상한 기록.
- **추가 검증**: `video::run` 시작 시 `authorize_paid_execution`가 동결 견적과 시점별 재산정 결과를 비교. 승인 누락/모델·초수 변경/부분 재생성 원본 결과 변경은 원격 유료 API를 보내기 전에 `needs_review`로 중단한다.
- 실제 제공자 청구액을 미리 통제할 공급자 인보이스 API가 없으므로, 이것은 **공시 목록가 기반 요청 승인**이지 실제 청구액의 상한 보증이 아니다.
