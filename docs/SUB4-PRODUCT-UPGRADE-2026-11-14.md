# 서브4 제품 전체 통합 업그레이드 — 2026-11-14

> 범위: 출시된 DOMINION의 영상·그룹 대화·모델·채팅·에이전트·RSI를 **동일한 실행 엔진, 작업 큐, 감사·증거·기억, UX 경로**로 발전시킨다. 새로운 기능의 등록만이 아니라 실제 진입점 → 실행 → 증거 → 다음 행동을 확인한다.
>
> 이 문서의 구현 현황은 **소스 연결 검토**이며, 이 변경에서 테스트/빌드/CI/실제 유료 모델·영상 호출은 실행하지 않았다. 성능·화질·비용 절감 비율을 측정 완료로 주장하지 않는다.

## 1. 실제 변경 — 영상 턴키 일관성

| 책임 | 소스/연결 | 현 상태 |
| --- | --- | --- |
| 사용자 생성 요청 | `web/src/pages/Video.tsx` → `POST /api/video/jobs` → `src/api/video.rs` → `video::enqueue` → `jobs::enqueue` | 이미 내구성 있는 작업 큐 |
| Kling 3.0 네이티브 멀티샷 | `src/engine/video/shot_plan.rs::{shots,request}` → `video/runway.rs::run` | **이번 변경**: 3~15초를 1~3개 카메라 샷으로 나누고 전체 길이 보존, 반복 인물·배경 앵커, `providerSettings.multiPrompt` 적용 |
| Kling 4.0 연속 생성 | `src/engine/video/kling4.rs::{clip_durations,direction,request,attach_prior_frame}` → `video/runway.rs::run` | **이번 변경**: 3~30초 1080p 네이티브 오디오, 최대 180초를 필요한 30초 클립으로 분리. 4.0에는 3.0 전용 `multiPrompt`/`audio`를 보내지 않고, 이미지-투-비디오의 `auto_1080p` 계약 사용 |
| 장편 클립 간 연속성 | `src/engine/video/frame_anchor.rs::first_frame_for_next` → `runway.rs::run` | **이번 변경**: 이전 클립 다운로드가 완료된 뒤 FFmpeg로 마지막 프레임을 JPEG로 추출하고 다음 `image_to_video`의 `promptImage`로 제공. 계정 키는 기존 vault 사용 |
| 비용·상태 | `runway.rs::unsafe_to_resubmit` → 원격 task ID 체크포인트 → 다운로드 → FFmpeg 합성 → `video::public_job` | **이번 변경**: 이전 클립이 유료 생성 후 다운로드된 경우에도 전체 계획 자동 재제출 차단(`needs_review`). 신규 `shotCount`/`frameAnchoredClips` 결과 필드 및 실제 저장된 모델 기준 유료 클립 수 표시 |
| 사용자 선택 | `Video.tsx`의 **Kling 3.0/4.0 모델·클립 간 연속성** 옵션 → `/api/video/config` → `runwayModel`·`runwayContinuityFrames` | **이번 변경**: 설정 저장·입력 스냅샷 고정, 모델별 최대 길이에 따른 유료 클립 개수 표시. 장면 전환이 필요하면 연속성 옵션 해제 가능 |

Kling 3.0 Standard/Pro/4K의 샷별 길이 합 3~15초, 최대 6개 샷, `image_to_video` 입력은 2026-10-26 Runway Dev 공식 문서에 근거한다. Kling 4.0의 30초 길이, `kling_4` 모델 ID, `auto_1080p` 이미지-투-비디오 비율은 2026-10-23 출시 및 10-26 공식 문서에 근거한다. 이 구현은 비용을 늘리는 별도 샷 요청 대신 동일 요청 안의 네이티브 멀티샷을 활용한다. **시각·인물 일관성은 확률적인 모델 결과**이며 영상 전체 화질·오디오 정합성이 자동 보장되는 것은 아니다. 15초를 넘는 영상은 여러 개의 유료 task와 합성으로 완성되므로 요청 총 길이에 비례한 비용이 발생한다.

관련 검사 소스: `tests/unit/video_shot_plan_tests.rs` (3.0 샷 길이 합·요청 형식·앵커), `tests/unit/video_kling4_contract_tests.rs` (4.0 30초 분할·요청·이미지 비율·프롬프트), `tests/unit/video_paid_resubmission_tests.rs` (유료 처리 부분 완료 후 중복 과금 차단). 실행하지 않음.

## 2. 실제 변경 — 그룹 대화, 토큰과 모델 가격 계측

| 경로 | 담당 | 상태 |
| --- | --- | --- |
| 참여자·발언 제한·모델 선택 | `src/engine/rooms/speakers.rs::{speaker_window,plan_speakers,model_pool,tiered_model_pool}` → `src/engine/rooms.rs::run_locked` | **이번 변경**: 발언자·모델 배정을 독립 모듈로 추출, `economy`/`frontier` 역할은 사용자가 명시적으로 지정했을 때만 난도별 적용. 명시적 모델/전원 발언 우선·모델 경로 유지. 그룹 런타임 파일은 979줄 수준 |
| 적응형 비용·품질 알고리즘 | `Rooms.tsx` → `api/rooms.rs::validated_config/submit_internal` → `algorithms.rs::{choose_for_agent,record_owner_feedback}` → `rooms/economy.rs` → `rooms/speakers.rs` → `rooms.rs::run_locked` | **이번 변경**: 새 방은 `adaptive` 기본, 기존 방은 `full` 유지. 실제 버전 있는 balanced/lean/quality 정책을 선택해 작업 스냅샷에 고정하고 발언 인원·라운드에 적용. 완료 작업에 대한 사용자 평가만 해당 정책 revision의 다음 선택 통계에 반영, 원본 증거 평가와 분리 |
| 방별 컨텍스트 | `src/engine/chat/room_context.rs::compile` | 기존: 동결된 주인 원문, 발언자 귀속, BM25+비용 기준 발췌, 압축시 필수 증거 보존 |
| 프롬프트 재사용 | `src/engine/chat/continuity.rs::{prepare,project,commit_projection}` | 기존: 참가자별 안정된 접두사·동적 프레임, 증거 변경 시 무효화 |
| 제공자 실제 캐시 사용 | `chat/provider.rs::cached_prompt_tokens` → `gateway::record_usage_tokens_with_cache` → `db.rs::session_usage` | **이번 변경**: 캐시 적중·비교 가능한 총 입력 토큰·측정 회수 별도 SQL 집계. 미보고/추정 토큰을 캐시 실패로 간주하지 않음 |
| 대화 UI | `src/api/rooms.rs`의 feed `usage` → `web/src/pages/Rooms.tsx` | **이번 변경**: 제공자 실제 보고가 있을 때만 방 헤더에 캐시 적중 비율 표시 |
| 소형/대형 모델 자동 선택 | `src/engine/router.rs::{classify,estimated_krw_per_million,select_provider}` | **이번 변경**: 기존 능력·키 보유·난이도 우선 정책을 유지하면서, 원화 입력/출력 가격이 모두 알려진 쌍에서는 가격 비교 |
| 가격·품질 설정 | `web/src/pages/Models.tsx` → `POST /api/providers` → `src/api/providers.rs::save_provider` | **이번 변경**: ₩/100만 입력·출력 토큰 가격과 0~100 품질 등급(사용자 평가)을 검증·저장. 자동 선택용이며 사용자가 고정한 모델은 건드리지 않음 |

**중요한 경계**: 원화 가격과 0~100 품질 점수는 제공자가 자동 제공한 확정된 수치가 아니라 **사용자가 입력한 선언값**이며 최신 가격 반영 및 품질 평가는 사용자 측정에 의존한다. 캐시 적중률은 청구 비용 절감률이 아니다. 캐시 쓰기·읽기 단가, 제공자별 할인과 실제 청구액은 별도이며, 데이터가 없으면 추정 할인율을 표시하지 않는다. 새 가격 정보가 없는 모델은 **무료 모델로 간주하지 않는다**.

검사 소스: `tests/unit/db_room_cache_usage_tests.rs`, `src/engine/router.rs`의 명시가격·품질 자동 선택 테스트, `tests/unit/engine_rooms_economy_tests.rs`의 예산 상한·지목 우선 검증 및 추출된 `tests/unit/engine_rooms_{pool_tests,tests}.rs`의 모델 tier 검증. 실행하지 않음.

## 3. 통합 아키텍처 및 유지보수 지도

```text
React (Rooms / Models / Video / Chat / SelfImprovement)
  ├── /api/rooms + /api/chat → engine::rooms → engine::chat
  │                      ├── rooms/economy (정책 버전별 발언 수·라운드 실행)
  │                      ├── rooms/speakers (발언자·저비용/심층 모델 선택)
  │                      ├── algorithms (revision·상태·사용자 피드백 통계)
  │                      ├── room_context (선택·증거·토큰 예산)
  │                      ├── continuity (고정 접두사·모델별 대화 상태)
  │                      ├── provider (도구·cache telemetry·stream)
  │                      └── router + gateway (능력·가격·제한·감사)
  ├── /api/video/jobs → engine::video → engine::jobs
  │                            ├── shot_plan (Kling 3.0 멀티샷·시간 계약)
  │                            ├── kling4 (Kling 4.0 30초·비디오 계약)
  │                            ├── runway (외부 제출·폴링·재개 경계)
  │                            ├── frame_anchor (FFmpeg 추출·이미지 연속성)
  │                            └── FFmpeg 합성 → 영속 출력
  └── /api/rsi/* → engine::rsi_evolve → rsi/{planner,execution,evaluation,lineage,meta,learning}
                                     └── frozen paired oracle → 채택 → 재컴파일 후손
SQLite: jobs, sessions, messages, audit, secrets / vault
```

기존 RSI 자기개선 폐회로의 **고정 가중치·독립 평가·소스 계승** 규칙을 바꾸지 않았다. 영상/그룹 개선은 기존 엔진 경계를 통해 실제 호출되고, RSI의 성취 판정은 원본 후보·동결 평가·후손 실행 증거에만 의존한다. 새로운 영상 코드가 존재한다고 RSI 1위 또는 A→B→A 실증을 주장하지 않는다.

## 4. 이어서 확장할 핵심 기능과 검수 증거

1. **영상 QA·복구 게이트**: 생성 후 실제 프레임·FPS·오디오 스트림·검은 화면/무음 구간 자동 검사, 원본 task의 재조회/재다운로드, 의도된 장면 전환·대사 씬별 편집, 비용 상한 및 재생성 승인. 현재는 중복 과금 재제출 차단과 완료된 MP4 컨테이너 중심이며, 자동 재개보다 안전한 `needs_review`에서 수동 검토하도록 둔다.
2. **원가 관측**: provider usage + cached input + 요금표 revision + 실제 청구액 대조, 엔진/모델/룸별 비용 및 지연·품질 차트를 동결 비교. 현재는 토큰·캐시 계측과 선언 가격 기반 자동 선택까지.
3. **행동 안전형 시맨틱 캐시**: 반복되는 무부작용·시점 불변·권한 동일 질의에만 캐시를 검토. 뉴스·주가·도구·개인정보·환경 상태 변경 질의는 저장값 재사용하면 오류나 누출이 생기므로 무조건적인 LLM 패스는 금지.
4. **에이전트 역할 통합**: task/episode-root, 발언자 attribution, 실제 도구 결과, delegated child job, room state, 결과 검증과 비용 기록을 단일 실행 계보로 집계; 가짜 사용자 발언을 생성하지 않음.
5. **대형 파일 분리**: 이번 변경에서 `rooms.rs`의 발언·모델 선택과 2개 단위 테스트 모듈을 분리해 런타임 파일을 979줄로 낮췄다. 이어서 `tools.rs`, `chat/provider.rs`, `api/providers.rs`, `web/src/pages/Models.tsx`의 책임을 기능별 디렉터리로 점진 이관. 아직 일부 대형 파일이 구조 점수의 병목.
6. **RSI 본증명**: 실제 유료 모델·CI는 별도 운영 시점에 연결하고 frozen oracle을 통과한 원본 A→B→A 영수증 및 비용/성능 회귀 지표가 나올 때만 개선을 인정.

## 5. 근거 — 2026 확인

- Runway Dev Kling 3.0 API, 2026-10-22 게시 / 10-26 갱신: https://dev.runwayml.com/learn/kling-3-0-api
- Runway Dev Kling 4.0 API, 2026-10-26 게시: https://dev.runwayml.com/learn/kling-4-0-api
- Runway Dev API 변경 이력, 2026-10-13 Kling 멀티샷 출시: https://docs.dev.runwayml.com/api-details/api_changelog/
- OpenAI API 프롬프트 캐싱 설명: https://developers.openai.com/api/docs/guides/prompt-caching
- 2026 ACL LLM-as-Scheduler 실험: https://aclanthology.org/2026.acl-long.581/

위 링크들의 측정 수치는 각 연구/공식 제공자 환경의 결과이다. DOMINION에서 같은 할인율이나 영상 품질 수치가 입증되었다는 뜻이 아니다.


## 6. 다른 2026년 제품·연구에서 확인한 다음 구조 선택지

- GitHub Copilot의 2026년 컨텍스트/라우팅 개선은 캐시 가능한 고정 프롬프트와 도구 검색·지연 로드를 병행한다. [GitHub Engineering 설명](https://github.blog/ai-and-ml/github-copilot/getting-more-from-each-token-how-copilot-improves-context-handling-and-model-routing/)
- DeepSeek Harness의 2026-09-20 설계 기록은 도구 활성 집합의 변경과 동일한 도구 선언을 분리해 히스토리의 캐시 가능한 접두사를 보존한다. [DeepSeek 동적 도구 업데이트](https://github.com/deepseek-ai/deepseek-harness/blob/master/.agents/notes/implemented/architecture/2026-09-20-dynamic-tool-updates.md)
- 2026 EMS 연구는 다수결 결과가 더 이상 뒤집히지 않을 때 투표를 중단하는 방식을 검증했다. 이 결과는 **답변 취합이 다수결인 토론**에 한정하며 자유 토론의 임의 조기 종료를 정당화하지 않는다. [EMS 논문](https://arxiv.org/abs/2604.02863)
- 2026-11 ParetoSaddler는 고정 가중치 에이전트 하네스의 품질·비용 Pareto 선택을 연구했다. 실서비스 라우팅에는 사용자 정의 품질 점수뿐 아니라 **실제 작업 성공/실패, 입력 캐시, 비용, 작업 난도**를 동일 실험으로 기록해야 한다. [ParetoSaddler](https://arxiv.org/abs/2611.02079)

다음 통합 목표는 **방별 사용량→비용·성공률→개선 후보→동결 평가→선택된 에이전트·라우팅 정책 revision→후속 작업 재평가**를 하나의 감사 가능한 고리로 연결하는 것이다. 현재 코드의 가격/품질 입력 및 캐시 집계는 그 기반이며, 자기개선에 의한 비용·성능 상승이 입증되었다고 표시하지 않는다.

## 7. 실제 개선법 계승에 연결된 그룹대화 정책 (2026-11-14)

```text
신규 대화방 설정 adaptive (기존 방 full)
→ POST /api/rooms + api/rooms.rs::validated_config (역할·모델 tier 검증)
→ POST /api/rooms/:id/messages → algorithms::choose_for_agent("multiagent.economy")
→ source revision 및 파라미터를 불변 job.input.roomSnapshot.economyKernel에 저장
→ rooms/speakers.rs::plan_speakers → economy::target_speakers
→ jobs::run(room) → economy::effective_rounds → tiered_model_pool
→ 실제 모델 호출, 저장 메시지, provider cache usage 및 job 결과
→ POST /api/rooms/:id/jobs/:job_id/feedback
→ room_feedback 멱등 레코드와 owner_preference 감사 이벤트
→ algorithms::record_owner_feedback(revision) → algorithm_stat
→ 다음 자동 발언 정책 선택
```

- 사용자가 **전체 토론·전원·직접 지목·직접 모델 지정**을 하면 적응형 최적화보다 우선한다. 모델 tier는 모델명을 추측하지 않고 사용자가 직접 `economy`/`frontier`로 표시한 항목만 사용한다.
- 이미 실행된 job의 `economyKernel`은 후속 모델/알고리즘 설정이 변해도 다시 해석하지 않는다.
- 평가 버튼은 사용자 **선호**를 뜻한다. 독립 검증이나 RSI 성공 판정의 oracle로 계산하지 않는다.
- 성능과 실제 절감률은 모델 호출 전후 관측을 비교해야 한다. 테스트·실제 청구는 이 변경에서 실행하지 않았다.

## 8. 운영 체크포인트와 RSI 개선 가능 소스 경계 (2026-11-14)

- `JobContext::checkpoint`는 **중첩 JSON을 얕게 덮어쓰는** 저장 API다. `video/runway.rs::merge_video_state`와 `checkpoint_video`는 유료 원격 작업 중 변경하지 않은 `scenes`, `shotPlans`, `taskIds`, `completedClips`를 보존하면서 `inFlight`·`remoteTaskId`의 명시 변경을 적용한다. 해당 단계는 `tests/unit/video_checkpoint_merge_tests.rs`에 독립 단위 계약으로 작성했다(미실행). 이 수정은 SQLite의 전역 patch 동작을 변경하지 않는다.
- `src/engine/rsi_system.rs::COMPONENTS`에 실제 사용 중인 `rooms/economy.rs`, `rooms/speakers.rs`, `router.rs`, `rooms.rs`, `chat/room_context.rs`를 알고리즘·하네스 소스로, `video/{runway,shot_plan,kling4,frame_anchor}.rs`를 도구 능력 소스로 반영했다. 실행 소스 검색에도 `video`와 `rooms` 책임 분류를 추가했다.
- 이는 실제 모델 후손이 **제품 기능과 개선 방법을 함께 탐색할 수 있게 소스 지도를 확장한 변경**이다. 동결 평가를 통과한 실험 채택, 후손 빌드, 실제 영상 품질 증가의 완료 증거를 생성하거나 점수를 가산하지 않았다.

## 9. 자율 목표 실행·증거·장기 복구 (에이전트 25점 기준)

기존 `goals.rs` 계획→실행→검토→재계획 폐회로에 `goals/report.rs::build`의 최종 기계적 검증과 파일 SHA-256 보고를 더했다. 별도 `Goals.tsx` UI를 사이드바와 API에 연결해 목표 입력, 실행 상태, 계획 버전, 실도구 결과, 검증 산출물, 중단, 대기 중 계획 변경, 외부 결과 수동 확인 후 재개를 제공한다. `verification.rs`는 `delegate_task` 및 `task_result` 완료 하위 도구 성공만 상위 목표에 귀속하고, `tools.rs` 위임 작업 수/활성 수 제한으로 급격한 모델 호출 확장을 줄인다. 자세한 소스 경로·검증 경계·후속 개선은 [자율 에이전트 통합 지도](SUB4-AUTONOMOUS-AGENT-2026-11-14.md)에 기록했다. **코드 소스 확인 및 커밋만 완료, 실행·CI·유료 모델 평가 미실행.**

### 자율 목표 실행 토큰 예산 및 소스 계약 (2026-11-14)

`api/goals.rs`는 자율 작업별 `tokenBudget`(0=무제한, 기본 150,000, 최대 2,000,000)을 저장한다. `goals.rs::run`은 다음 executor 턴 시작 직전에 `db::session_usage`의 실제 누적 토큰을 조회하며, 초과 시 `needs_review`로 마무리한다. `goals/report.rs`의 `finalReport.sessionUsage`와 `Goals.tsx`에 관측된 입력/출력/캐시 토큰과 호출 횟수를 표시한다. Planner·Reviewer의 별도 추론·제공자 청구는 이 예산에 자동 포함되지 않아 청구 총액 상한으로 선전하지 않는다. `tests/unit/goals_api_lifecycle_tests.rs`에서 목표-작업 연결, 대기중 계획 수정, 취소 및 명시 확인 후 재개를 검사하도록 작성했다(미실행).
