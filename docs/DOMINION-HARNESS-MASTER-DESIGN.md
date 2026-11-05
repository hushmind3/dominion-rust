# DOMINION — 범용 자율 AI 하네스 통합 마스터 설계서

**버전:** 2.0 · **갱신일:** 2026-11-06 (Asia/Seoul) · **구현 인계 대상:** DeepSeek / Rust 개발자

**상태:** 현행 소스 분석과 목표 설계. 이 문서의 신규 구조는 구현 완료를 의미하지 않는다. 이번 작업은 애플리케이션 코드를 수정하지 않았다.

## 문서 사용법과 분석 기준

이 문서가 DOMINION의 최상위 설계 기준이다. 기존 `DOMINION-DEEPSEEK-HANDOFF-2026-11-05.md`와 S0~S12 문서는 세부 연구와 과거 구현 기록으로만 참조한다. 충돌 시 현재 소스의 사실관계와 이 문서의 목표 계약을 각각 구분하여 적용한다.

| 항목 | 확인값 |
| --- | --- |
| 저장소 | `hushmind3/dominion-rust` |
| 분석 작업 디렉터리 | `/Users/gim-yeonsu/dominion-rust` |
| 브랜치 | `codex/native-memory-harness` |
| HEAD | `9eff603251d3f40892c7e34f9c5f4f5598bf5405` |
| 기준 소스 | 위 HEAD와 분석 시점의 미커밋 소스; 원격 브랜치와 HEAD 일치 확인 |
| tracked source diff SHA-256 | `7c7c07128097780d0bb0924dbe9528674a4be9e4c5c8a6f509a6035dc34c2f8d` |
| 소스 인벤토리 | `src`, `web/src`, `src-tauri/src`, `tests`, `scripts`: 148개, 44,060줄 |
| 이번 확인 | 파일 인벤토리, 핵심 호출 경로·함수 본문·저장 및 평가 계약 정적 분석, 기존 실험 보고서 대조 |
| 이번 실행하지 않은 검증 | cargo/npm 테스트, 실제 모델 성능 재측정, 계정 발송·업로드, 운영 DB·비밀키 열람, 설치·다중 OS 실기 검증 |

분석 시작 시 미커밋 파일은 `src/api/benchmark.rs`, `src/bin/dominion-benchmark.rs`, `src/engine/scheduler.rs`; 미추적 경로는 `tmp/`였다. 이는 사용자 작업으로 보존한다. 이 문서의 runner/scheduler 분석에는 해당 작업 트리 내용이 포함된다. `tmp/` 전체와 운영 데이터를 조사했다는 뜻은 아니다. 과거 설계의 테스트 통과 수치를 이번 검증 결과로 재사용하지 않는다.

분석은 전체 영역의 파일·진입점 인벤토리와 핵심 실행 경로 본문을 중심으로 했다. 모든 파일·분기를 동일 깊이로 검증한 것은 아니다. **연결 확인은 정적 호출 관계, 실제 결과는 출처가 명시된 기존 실험 기록, 나머지는 목표 설계**로 구분한다. 최신 문서와 소스가 다르면 소스를 구현 사실의 기준으로 삼고 실험은 해당 실행 revision에만 귀속한다. 이 문서의 코드·SQL·API 예시는 목표 계약이며 현재 배포된 구현이라는 뜻이 아니다.

**판정:** `연결`은 실제 호출 경로 확인, `부분`은 보장 범위 제한, `미연결`은 생산 실행의 소비 경로 미확인, `중복`은 책임 분산, `결함`은 정적 계약 위반, `부재`는 조사 범위에서 구현을 찾지 못함이다. 연결만으로 실계정·실모델 성공을 단정하지 않는다.

## 목차

1. 프로젝트 비전과 최상위 설계 철학
2. 범용 AI 하네스의 정의와 역할
3. 기존 저장소 전체 분석 결과
4. 현행 아키텍처의 문제점과 기술 부채
5. 목표 시스템 전체 아키텍처
6. 모델 활용 및 동적 능력 지원 설계
7. 범용 에이전틱 실행엔진 설계
8. 하네스 내부 알고리즘 체계
9. 장기기억·경험·학습 아키텍처
10. 자율개선 및 메타개선 알고리즘
11. 멀티에이전트·멀티모델 협업 설계
12. 도구·MCP·플러그인·외부 연결 설계
13. 실행 권한·상태·검증·장애 복구
14. Rust 모듈·trait·데이터·API 계약
15. 실제 작업의 종단 간 실행 시나리오
16. 기존 구현 재사용·통합·교체 방안
17. 새로운 단계별 개발 로드맵
18. 성능·성공률·자율개선 검증 방법
19. 기술적 위험·한계·대안
20. 최종 아키텍처 검토와 구현 인수 조건

---

## 1. 프로젝트 비전과 최상위 설계 철학

DOMINION은 **모델의 추론을 실제 환경의 행동으로 연결하고, 수행 경험으로 실행 정책을 검증하며 개선하는 Rust 기반 범용 자율 AI 하네스**다. 대화, 개발, 조사, 문서·미디어 생성, 파일 관리, 서비스 운영, 장기 목표, 에이전트 협업을 같은 실행 기반으로 처리한다.

최상위 단위는 DAG나 bandit이 아니라 **Task → Run → Observation → Evaluation → Policy Revision**이다. DAG는 의존성을 표현하는 선택 가능한 계획 형식이며, contextual bandit은 적격한 정책 후보를 고르는 한 방법이다. 모델의 크기를 실행 계약에 넣지 않는다. 모델마다 다른 능력은 기능 계약과 실측 프로파일로 표현한다.

핵심 원칙은 다음과 같다.

1. **모델 우선 판단:** 의미 이해, 개방형 계획, 가설, 새 도구의 조합, 결과 설명은 모델이 담당한다. 모든 요청을 사전 정의된 단계에 끼워 넣지 않는다.
2. **엔진의 정확성:** 권한, 예산, 상태 전이, 동시성, 증거 출처, 저장, 재개, 외부 부수효과의 불확실성은 Rust가 책임진다.
3. **선택 가능한 지원:** 구조화 출력 교정, 짧은 계획, 검색, 절차 보조, 다른 모델 검토를 실측 필요에 따라 추가한다. 강한 모델에는 불필요한 재계획·교사 호출·작은 컨텍스트 제한을 부과하지 않는다.
4. **실제 결과로 갱신:** 모델의 완료 선언, 정상 HTTP 응답, 파일 생성, 절차 준수, 사용자 목적 달성은 서로 다른 신호다.
5. **가역적 정책 발전:** 새 정책은 불변 revision으로 만들고 평가·canary·배포·회귀 관찰을 거친다. 실행 중 정책을 몰래 바꾸지 않는다.
6. **동일한 행동 경계:** 모델, 절차 실행기, 다른 에이전트, 스케줄러가 제안한 행동은 같은 권한·예산·Operation 경로를 통과한다.
7. **로컬 우선:** 단일 Rust 프로세스와 SQLite로 시작한다. 모델·임베딩·연결을 로컬에 둘 수 있으며, 클라우드 전송은 명시한 데이터 경계 안에서만 허용한다.

범용성은 모든 문제의 성공 보장이 아니다. 새 작업을 표현·탐색·시도하고 관찰에 따라 수정할 수 있으며, 능력이나 자원이 부족하면 그 이유와 미완료 상태를 정확히 보존하는 성질이다.

## 2. 범용 AI 하네스의 정의와 역할

| 개념 | 소유자 / 의미 | 혼동하면 안 되는 것 |
| --- | --- | --- |
| Session | 사용자와 에이전트의 대화·출처·접근 범위 | 하나의 작업 또는 실행과 동일하지 않음 |
| Task | 사용자 의도, 완료 계약, 권한, 전체 예산을 가진 작업 | 모델이 만든 계획과 구분 |
| Run | 동결한 정책·모델 설정으로 Task를 수행하는 실행 | 복구·재시도 때문에 Task당 여러 개 가능 |
| Step / Operation | 판단 경계 / 환경에 대한 개별 시도 | 한 tool call ID가 영구적 식별자는 아님 |
| PlanRevision | 목표를 달성하기 위한 수정 가능한 계획 | 원래 사용자 목적을 바꾸는 권한은 없음 |
| Evidence | 원문·도구 receipt·파일 해시 등 관측 자료 | 모델 해석 또는 주장과 구분 |
| Evaluation | 특정 oracle revision이 증거를 판정한 기록 | 실행 상태와 별개, 재평가 가능 |
| PolicyRevision | 프롬프트·검색·계획·라우팅·복구·선택 등의 불변 구현/설정 | 사용자 지시·권한 규칙을 대체하지 않음 |
| Job | 작업을 깨우는 영속 스케줄링 단위 | worker의 Rust Future 자체를 저장하는 것은 아님 |

모델은 `ActionProposal`을 내고 엔진은 이를 검증하여 `AuthorizedOperation`으로 바꾼다. 모델이 엔진을 자유롭게 활용하되 모델 출력 자체를 권한으로 사용하지 않는다. 알고리즘은 판단 후보와 점수·불확실성·측정 기록을 제공하며 실행 권한을 직접 갖지 않는다.

자율성의 범위는 Task 계약에 저장한다. `interactive`는 필요한 입력을 기다릴 수 있고, `background`는 허가된 범위에서 UI 없이 진행하며, `scheduled`는 시간·이벤트에 의해 재개한다. 이 모드는 모델 지능 등급이 아니라 작업 수명과 상호작용 방식이다.

## 3. 기존 저장소 전체 분석 결과

### 3.1 현재 실행 경로

```text
bootstrap::serve
  → SQLite/migrations → jobs/wake/finalization 복구
  → experiments 예약 + policy 후보 제안
  → jobs/maintenance/scheduler + Discord/Telegram + Axum

HTTP 채팅 / Room / Goal / delegated Job / 외부 채널
  → 각 ingress의 session/task 구성
  → chat::run_turn 계열 → runtime::run_turn_with_policy
      → session lock + LeaseGuard + cancellation generation
      → Task/Attempt + context/strategy snapshot
      → planning/recovery policy + model/tool loop
      → dispatch_tool_call (모델 호출과 assisted procedure 공통)
      → finish_attempt / finalize_execution / Store::finalize_once
      → observation / experiments 예약 / policy 후보 제안

chat/provider + llm helper + compact
  → gateway의 route/resolve/admission/send/usage

improve / policy 후보 → experiment queue
  → opt-in scheduler → dominion-benchmark child process
  → experiments API 결과 → 평가/승격 판단
```

모든 ingress가 동일한 root 권한·예산·수명 계약으로 완전히 통합된 것은 아니다. 특히 helper 호출의 비용 귀속, 부가 서비스의 외부 효과, detached child, wake 시 권한 복원이 남아 있다. **일반 대화가 공통 실행의 중심이며 `/goal`은 선택 가능한 장기 작업 인터페이스다.**

### 3.2 영역별 구현과 경계

| 영역 / 근거 | 현재 확인한 경로 | 판정과 남은 범위 |
| --- | --- | --- |
| 서버·bootstrap | `bootstrap.rs::serve`, migrations, recover, maintenance 시작 | 연결. loopback 로컬 중심; 서버 다중 사용자·기기 권한은 목표 |
| 일반 채팅 | `chat/runtime.rs::run_turn_with_policy`, `run_turn_body` | 연결. streaming/native·compat tools, acceptance, assisted 실행. 기본 8 rounds/40 tools/600초/250k token 등은 동적 예산으로 발전 필요 |
| UI 연결 해제 | runtime이 SSE disconnect를 실행 취소 조건에서 제거 | 이미 개선됨. UI 닫힘과 명시 취소를 다시 결합하지 말 것 |
| Goal·장기 작업 | `goals.rs`, `jobs.rs`, `workflows.rs` | 연결/부분. plan/review/repair/replan 및 작업 경로 유지; 모든 요청에 Goal 절차 강제 금지 |
| 실행 복구 | `run_state.rs`, `lease.rs`, `wake.rs` | 연결/결함. heartbeat·중단 복구 있으나 fencing·권한 보존·일반 durable waits 미완성 |
| 모델 | `gateway.rs`, `models.rs`, `chat/provider.rs`, `llm.rs`, `compact.rs` | 연결. 공통 gateway와 local GGUF/llama/LM Studio 경로 있음. 모델명 숫자 기반 능력 추정·root 비용 누락은 교체 대상 |
| 도구·승인 | `tools.rs`, `chat/runtime.rs::dispatch_tool_call` | 연결/부분. 노출과 실행 허용 분리, 공통 model/assisted dispatch. registry·모든 서비스 부수효과 통합은 미완성 |
| MCP | `mcp.rs` stdio/HTTP initialize | 연결/부분. 코드의 protocol 2024-11-05 및 이름 기반 identity를 버전 협상·안정 ID로 보강 |
| 기억·학습 | `memory.rs`, `learning.rs`, `reflect.rs`, `experience.rs` | 연결/부분. content revision·추출·lexical 검색·경험 주입 존재. reflection의 global 저장과 source 권위 경계, 의미 검색 확장 필요 |
| 전략·평가 | `improve.rs`, adaptation 타입, runtime selection snapshot | 연결. deterministic bundle 확률 1·semantic hash·delta merge 개선됨. 혼합 평가를 일괄 deterministic으로 기록하는 문제 잔존 |
| 절차/DAG | `procedure.rs` validation/ready/evaluate, runtime assisted | 연결. node/edge 및 instance receipt 평가 존재. DAG 부재로 실행 불가한 구조는 아님 |
| 정책 자율개선 | `policy.rs::propose_candidates`, `policy_patch.rs`, runtime 소비 | 연결/부분. planning/recovery 두 슬롯 및 제한된 파라미터 후보. 범용 새 알고리즘 생성·메타 최적화와 동일하지 않음 |
| 실험 | `api/experiments.rs`, `scheduler.rs`, benchmark 바이너리 | 연결/결함. 실제 로컬 모델 paired 기록 존재. opt-in 자동 runner, 통계·승격 원자성·증거 검증 보강 필요 |
| 협업·페르소나 | `agents.rs`, `rooms.rs`, `tools.rs` delegate, jobs | 연결/부분. 역할·그룹 대화·위임·권한 boolean 상속 존재. root 예산·채널 scope·linked cancellation 부족. 9명은 preset이지 엔진 상한 아님 |
| 외부 채널 | `discord.rs`, `telegram.rs`, bridge/inbox/outbox 경로 | 연결/부분. 실제 계정·방 생성·메시지 전달은 이번에 확인하지 않음. channel binding과 공유 정책 명시 필요 |
| 개발·영상 서비스 | dev/video 및 API, 별도 프로세스·외부 요청 | 연결/중복. 공통 Operation으로 귀속 필요; 실제 렌더·YouTube 업로드 성공은 미확인 |
| 계정·보관 | OAuth/vault, `vault_rotation.rs`, `backup_keys.rs`, `remote_backup.rs`, `maintenance.rs` | 연결/부분. 암호화·회전·백업 경로를 재사용. HTTP backup을 완성된 S3·다중 기기 운영으로 과장하지 않음 |
| SQLite·이벤트 | `db.rs` create_attempt/finalize_once/delta merge/leases, `events.rs` durable log+replay | 연결/결함. 기존 원자화 개선 보존. 상태와 이벤트의 통합 transaction, DB 순서 할당·fence 남음 |
| 프론트·데스크톱 | React/Vite/Tailwind, `web/src/api.ts`, `src-tauri`, Rust sidecar | 연결/부분. Node는 프론트 개발 도구. 현재 package scripts 정리됨; 이전 Electron 잔재 지적 반복 금지. API any·수동 event 계약 보강 |
| 테스트·운영 | tests, benchmark, CI/release workflow | 자산 존재. Linux release tests/build 경로 있음. 이번 재실행 없음; fixture·모델 성과·실계정 운영은 별개 |
| 범용 browser/computer | shell·MCP로 일부 작업 가능 | 독립 native Browser/Computer adapter의 안정 계약은 조사 범위에서 부재. shell이 있다는 이유로 완성 판정 금지 |

**미연결/미구현의 구분:** planning/recovery 정책 소비, gateway, lease, wake는 연결되어 있다. 아직 없는 것은 전 슬롯 공통 PolicyRegistry·일반 알고리즘 생성기·독립 meta-evaluation·계층 예산과 durable dependency coordinator다. 모델이 만든 개선 제안 문장만으로 이 기능이 연결됐다고 보지 않는다.

### 3.3 기존 실제 실험이 말해 주는 범위

출처: `docs/H7-SMOKE-RESULT-2026-11-05.md`의 누적 기록. 아래는 **기존 보고서 대조이며 이번 재측정이나 raw artifact 전체 재검증이 아니다.**

| 기록 | 관찰 | 해석 |
| --- | --- | --- |
| 초기 4B seeded 절차 smoke | baseline 0/3, assisted 3/3 | 작은 fixture에서 실행 보조 효과. 범용 학습/전이 입증 아님 |
| update_config 12 pairs | baseline 0/12, treatment 12/12, 보고된 usable 11·preliminary_support | 해당 fixture 결과. 반복 횟수를 독립 task family 수로 보지 않음 |
| 11월 6일 no-plan baseline 비교 | baseline 10/10, treatment 8/10+timeout 2회; p50 약 128.3초/168.8초 | 전체 할당 기준 성공률 차이 **−20%p**. 표본이 작아 일반 회귀를 확정하지 않지만 개선 증거도 아님 |
| 같은 no-plan 실험의 현재 runner 판정 | 완료된 pair 8개만 사용하여 delta 0·inconclusive; 후보 미활성 | 미승격은 유지해야 함. timeout을 버리면 운영 실패가 비교에서 숨겨지므로 통계 계약 수정 필요 |

계획 품질 heuristic이 높아져도 성공률이나 지연이 악화될 수 있다. 이 결과 때문에 계획을 더 강제하거나 유리한 fixture만 찾는 방향으로 설계를 바꾸지 않는다. `none` 정책, 단순 reactive 실행, 비용 절감도 동등한 후보다.

### 3.4 저장소 전반의 책임 배치

`src/api`는 ingress/DTO, `src/engine`은 현재 실행 및 기능 서비스, `src/db.rs`는 영속성, `src/events.rs`는 알림, `src/bin`은 실험 도구, `web/src`는 화면, `src-tauri`는 desktop lifecycle이다. 현재 runtime은 약 2,500줄로 여러 책임을 품는다. 분해 기준은 줄 수가 아니라 호출·권한·transaction 소유권이다. 기존 façade를 유지하면서 책임을 추출하고 두 엔진을 동시에 돌리지 않는다.

## 4. 현행 아키텍처의 문제점과 기술 부채

아래는 최신 작업 트리의 정적 근거다. 이번에 장애를 재현하거나 테스트를 실행했다는 뜻은 아니다. 이전 설계 D01~D18은 그대로 계승하지 않으며 이미 고쳐진 항목은 §3의 현재 사실로 대체한다.

| ID / 우선순위 | 근거와 문제 | 목표 수정 / 반드시 필요한 회귀 조건 |
| --- | --- | --- |
| C01 / P0 | `lease.rs` 갱신 실패가 heartbeat만 종료. worker dispatch/commit에 fencing 없음 | lease epoch를 Operation·checkpoint commit에서 CAS; 탈락 worker는 취소. lease 탈취 후 이전 worker의 write 0 |
| C02 / P0 | `wake.rs` 재개 input에 toolsEnabled/toolsExecutionEnabled=true, side-effect 판정이 일부 tool 이름 목록 | 최초 grant/policy/resource scope 보존·현재 철회 재검증; registry effect class 사용. 실행 금지 Task가 재개로 권한 얻지 않음 |
| C03 / P0 | `api/experiments.rs` 결과 적용과 최종 claimToken CAS 분리, 제출 delta/units/verdict에 의존 | frozen manifest+raw trials에서 서버 재계산; claim/fence·promotion intent를 원자 commit. stale runner의 승격 0 |
| C04 / P0 | benchmark `pair_effect` 완료 pair만 사용, 반복 pair를 independentUnits로 표기, CI halfwidth를 먼저 min(1) 처리 | ITT·cluster 단위·사전 통계 계획; CI 최종 endpoint만 support와 교차. timeout 회귀가 보고됨 |
| C05 / P1 | `events.rs` AtomicU64 할당→DB append→broadcast; append 오류 및 concurrent 순서 불일치 가능 | DB commit 안 sequence·state·outbox; 실패 시 성공 방송 금지. 재시작/동시 완료에도 cursor 누락 없음 |
| C06 / P1 | `run_state.rs::apply` get→put, 불법 전이 시 이전 값 반환; completed를 succeeded로 매핑 | expectedVersion/fence CAS와 typed transition error. lifecycle 종료와 TaskOutcome 성공/미검증 분리 |
| C07 / P1 | finalizer가 regime=ProductionObserved, label=DeterministicOracle 일괄 기록; 일부 오류 fallback은 terminal 단독 저장 | 환경·check별 평가 provenance, finalize 실패를 retry 가능한 상태로 보존. 저장 실패가 성공/학습으로 흡수되지 않음 |
| C08 / P1 | gateway helper usage root=None, delegate/background/procedure의 계층 예산 불완전 | root reservation ledger, provider/helper/child 비용 합산, 취소 연결·명시 detach. 동시 호출로 상한 초과 불가 |
| C09 / P1 | policy apply가 모든 슬롯에 plan-quality 개선을 요구, 전역 active 교체가 여러 put | slot별 primary metric·비열등성/비용 gate; scope별 deployment CAS. 계획 없는 효율 정책·recovery 정책도 공정 평가 |
| C10 / P1 | plan_quality 두 판정 모두 문장/단계/단어 heuristic | 두 개를 독립 정답 판정기로 취급하지 않음. artifact oracle·blind semantic rubric과 분리 |
| C11 / P1 | reflection의 global memory/experience 기록, 일반 root source 고정, 위임/channel scope 부족 | 원문 principal·source message·partition·grant를 Task부터 보존. 추론과 사실 분리, 검색 전 ACL |
| C12 / P1 | gateway 모델명 숫자 rank, local URL substring, 고정 baseURL semaphore | 명시 capability/실측 profile, URL 파싱과 data-egress policy, 실제 resource ID·interactive quota |
| C13 / P1 | scheduler child output 대기·메모리 축적, wake enqueue와 marker 별도 저장, jobs::start_now 우회 | bounded output·총 timeout·process cancellation; job/wake unique key transaction; 단일 scheduler 입장 경로 |
| C14 / P2 | 광범위 JSON/any, runtime·서비스 책임 분산, 저장 Option 오류 축약 | 핵심 Rust 타입/Result·생성 TS 계약, domain/store port, 관측 가능한 오류. source 호환 façade 유지 |

P0는 데이터·실험 신뢰 경계의 우선순위이며 모든 기능을 정지하라는 뜻이 아니다. 현재 작동하는 채팅·절차·기억·룸·MCP를 보존하면서 수정한다. 고정 8-round 같은 기본값도 무조건 제거하기보다 작업별 예산 상속과 연장/중단 사유를 갖게 한다. 속도 개선은 첫 응답, 모델 대기, 도구, context, 후처리 시간을 분리 측정한 뒤 판단한다.

## 5. 목표 시스템 전체 아키텍처

### 5.1 통합 구조

```mermaid
flowchart TB
  U[React / Desktop / CLI / Channels / Schedules] --> I[Ingress + Identity + Task Contract]
  I --> K[RunCoordinator · Durable State Machine]
  K <--> C[Context Composer + Memory Service]
  K --> P[Planning and Decision Policies]
  P --> M[ModelGateway + Capability Registry + Router]
  M --> L[Local and Cloud Models]
  P --> A[Action Proposals]
  A --> G[ActionGateway · Authority · Budget · Journal]
  G --> T[Builtin / MCP / Browser / FS / API / Process]
  K <--> S[Scheduler + Agent Supervisor]
  S --> K
  T --> O[Observations + Evidence + Artifacts]
  O --> K
  O --> V[Verification and Evaluation]
  V --> K
  V --> X[Experience + Failure Analysis + Candidate Generation]
  X --> E[Isolated Experiments + Holdout + Canary]
  E --> R[Versioned Policy Registry + Deployment]
  R --> P
  R --> C
  R --> S
  R --> V
  E --> X
  K <--> D[(SQLite · Event Journal · Checkpoints · Outbox)]
  O --> D
  R --> D
```

실행 경로와 개선 경로는 같은 Task/Run/Evidence/Policy 계약을 사용한다. 개선 작업도 하네스의 Job으로 수행하지만 foreground 예산과 권한을 공유하지 않는 별도 quota를 갖는다. 개선 지연이나 실패가 사용자 실행의 완료를 지연시키지 않도록 outbox에 관측 이벤트를 원자적으로 남긴다.

### 5.2 계층과 의존 규칙

| 계층 | 책임 | 허용 의존 |
| --- | --- | --- |
| domain | ID, 계약, 상태, evidence, errors, 이벤트 | serde 등 기본 타입; AppState/HTTP/DB 금지 |
| runtime | 상태 전이, proposal 처리, checkpoint, lifecycle | domain + port traits |
| model/context/tools/memory | 모델·컨텍스트·행동·지식 서비스 | domain + repository/adapter ports |
| policies | 계획·검색·선택·복구·스케줄링 알고리즘 | immutable inputs와 제한된 model port; DB 직접 수정 금지 |
| adaptation | 후보 생성, 실험, 정책 revision 및 배포 | 평가된 관측, registry, sandbox runner |
| storage/adapters | SQLite·provider HTTP·MCP·process·channel | domain 구현; runtime 판단을 역호출하지 않음 |
| api/ui | 인증·요청 DTO·이벤트 projection·표현 | application facade; 직접 tool/process 실행 금지 |

런타임이 adaptation 내부를 직접 호출하는 순환 의존은 없앤다. 런타임은 registry의 동결 PolicyBundle을 읽고 Observation을 기록한다. adaptation은 그 기록을 소비해 새로운 bundle을 발행한다. 등록된 policy implementation이 compiled Rust인 경우 composition root가 연결한다.

### 5.3 고정할 최소 불변조건

- I1: 권한이 확인되지 않은 외부 부수효과는 dispatch하지 않는다.
- I2: 실행 시작 기록 저장에 실패하면 외부 행동을 시작하지 않는다.
- I3: 확인되지 않은 결과는 Unknown이며 성공이나 자동 재실행으로 치환하지 않는다.
- I4: Task 성공 조건을 후보 정책이 임의 완화하지 못한다.
- I5: Run의 contract·policy·model config·context snapshot revision을 추적할 수 있다.
- I6: root budget과 grant는 자식·재시도·모델 전환에도 보존된다.
- I7: durable evaluation과 outbox는 한 transaction으로 한 번만 반영한다.
- I8: 생성한 가설, 운영 관찰, 통제 실험의 근거 등급을 섞지 않는다.
- I9: UI 연결·이벤트 손실·서버 재시작이 작업 목적이나 권한을 바꾸지 않는다.
- I10: 승인/접근통제/평가 기준 변경을 일반 온라인 정책 학습에 맡기지 않는다.

이 조건을 엔진에 고정하되 계획 깊이·추론 방식·모델 수·DAG 사용 여부는 고정하지 않는다.

## 6. 모델 활용 및 동적 능력 지원 설계

### 6.1 ModelGateway와 능력 계약

provider가 OpenAI 호환 API인지 여부와 모델의 실제 능력을 분리한다. `ProviderAdapter`는 전송·인증·stream decoding을, `ModelProfile`은 기능과 측정값을 담당한다. 초기에는 기존 Chat Completions adapter를 이식하고, 다른 native API는 별도 adapter로 추가한다. 모든 provider를 최저 공통 API로 강제하지 않는다.

| 필드 | 의미 / 취급 |
| --- | --- |
| identity | providerId, modelId, model/version digest(가능할 때), endpoint revision |
| capabilities | text/image/audio/video input/output, native tools, structured JSON, parallel calls, streaming, cache, continuation |
| support | 각 capability의 Supported / Unsupported / Unknown + declared/probed/measured 출처 |
| limits | context, max output, request bytes, tool count/schema 제한, provider concurrency |
| reasoning | provider가 지원하는 옵션 schema와 범위. 공개 가능한 reasoning summary와 opaque state 구분 |
| extensions | adapter namespace의 typed 옵션과 schema version; 알 수 없는 옵션은 경고/거절 |
| performance | task family별 성공률·JSON 유효율·tool 오류율·p50/p95 지연·비용·표본 수·측정일 |
| data boundary | 로컬/허용 region/외부 전송 금지/사용 가능한 secret account |

Capability 조회는 등록 정보 → 공식 API 메타데이터 → 부수효과 없는 probe → 실제 실행 통계 순서로 보완한다. Unknown을 지원으로 가정하지 않는다. 사용자가 지정한 모델의 기능 부족을 알게 되면 허가된 fallback, 기능 보조, 입력 요청 중 하나를 선택하고 이유를 남긴다.

숨겨진 chain-of-thought를 하네스의 필수 인터페이스로 요구하지 않는다. 결정·도구 제안·공개 rationale·검증 evidence만으로 제어한다. provider 고유 continuation 데이터는 원래 adapter에서 opaque하게 보관하며 다른 모델로 넘길 때 이식 가능한 원문·도구 결과·작업 상태만 사용한다.

### 6.2 선택 및 전환

선택은 먼저 `capability ∩ permission ∩ privacy ∩ available budget ∩ availability`로 적격 집합을 만든 뒤 한다. 초기 정책은 사용자 지정 모델 우선 + 명시적 대체 목록 + 실측 비용/성공률 tie-break다. 데이터가 충분할 때만 contextual router를 도입한다.

`route(task_features, role, required_capabilities, constraints)`는 `{model, adapter, config_revision, alternatives, reasons, decision_id}`를 반환한다. 역할은 executor/planner/reviewer/embedding/reflector 등이며 모델 인스턴스와 독립적이다. 하나의 모델이 여러 역할을 맡아도 된다. 실험에서는 역할별 모델 변경을 분리해 “동일 모델 하네스 개선”과 “더 강한 모델로 교체한 효과”를 따로 측정한다.

전환은 모델 호출 경계에서만 수행한다. 이미 실행한 tool operation은 다시 실행하지 않는다. 변경 전후 모델·입력 hash·usage·실패 유형을 저장한다. 불확실한 서버측 tool 실행을 제공하는 provider는 operation receipt/reconcile adapter 없이는 투명 재시도하지 않는다. 모델 답변 스트림이 끊겼을 때 partial answer를 최종 성공으로 저장하지 않는다.

### 6.3 지원 수준은 독립 옵션의 조합

| 관찰된 필요 | 지원 정책 | 지원 해제 / 상향 조건 |
| --- | --- | --- |
| 도구 schema를 안정적으로 따름 | native tools, 동적 도구 검색, 가능한 병렬 제안 | protocol 실패 시 해당 adapter capability 재평가 |
| native tools 미지원 | 명시적 compat protocol과 schema validation | probe/실측으로 native 안정성이 확인되면 전환 |
| 인자 오류 반복 | 구조화 오류 관측, 최소 교정 예시, 제한된 repair | 최근 유효율이 회복되면 추가 교정 프롬프트 제거 |
| 컨텍스트 부담 | relevant tool subset, 토큰 기반 압축·검색 | 더 큰 context 모델/더 높은 budget이면 확대 |
| 계획 불안정 | 짧은 subgoal, 중간 verifier, 선택적 planner | 단독 실행의 성공·비용이 더 좋으면 보조 해제 |
| 검증된 절차의 기계적 단계 | 적합성이 확인된 procedure assistance | 낯선 조건·불완전 binding·drift면 model planning으로 복귀 |
| 의미 판단 불확실 | 독립 reviewer 또는 사용자 입력 | 비용/위험 기준과 평가 신뢰도를 기록 |

4B라고 모두 compat/DAG, 70B라고 항상 무보조로 정하지 않는다. 강한 모델이 긴 탐색을 할 수 있도록 Task budget을 설정할 수 있고, 단순 인사에는 planner·reviewer를 추가하지 않는다. 지원은 능력을 보충하지만 올바른 추론을 보장하지 않는다.

### 6.4 컨텍스트와 비용

`ContextComposer`는 mandatory instruction, Task 계약, 최근 원문, 관련 memory, 현재 plan, 최신 observation, tool schema, artifact 참조를 토큰 예산 안에서 구성한다. 필수 조건과 미해결 승인·operation 결과를 요약 때문에 잃지 않는다. 도구 call/result 쌍은 protocol 단위로 유지한다. 고정 40/160 메시지 절단은 이 정책으로 대체한다.

모든 모델 호출은 root ledger에 예약하고 실제 usage로 정산한다. planning/review/compaction/reflection/fallback도 예외가 아니다. usage 미제공이면 `estimated=true`와 토큰 추정기를 사용하며 0으로 기록하지 않는다. 가격표 revision·통화·실측/추정 여부를 저장하고 가격을 모르면 금전 지표는 Unknown으로 둔다. 토큰·시간 예산은 계속 적용된다.

공통 gateway의 자원 semaphore는 runtime instance별로 둔다. 현재 baseURL 단위 semaphore를 실제 모델 서버/GPU resource identity에 연결하고 streaming·non-streaming 호출의 공통 admission을 유지한다. 네트워크 호출 중 DB lock이나 worker 소유권을 붙잡지 않는다.

## 7. 범용 에이전틱 실행엔진 설계

### 7.1 계약 생성과 계획

Ingress는 실제 principal, source event, session, attachments, 사용자의 원문을 검증해 Task를 만든다. 명시한 성공 조건과 권한은 immutable contract revision에 저장한다. 의도 해석 모델이 제안한 조건은 `inferred`로 구분한다. 경로·도구 사용 여부만으로 복잡한 문서 품질이나 서비스 운영 성공을 판정하지 않는다.

계획은 `Reactive`, `Outline`, `DependencyGraph`, `ProcedureInstance` 중 선택하며 서로 전환할 수 있다. 저장된 DAG가 없으면 Reactive 실행에서 관측하고 새 계획을 만든다. Outline의 자연어 설명은 모델 입력이고, 기계가 실행할 단계만 schema 검증된 ActionProposal로 바꾼다.

PlanRevision은 parent revision, 생성 모델/정책, subgoal, dependency, assumptions, success signals, change reason을 갖는다. 완료한 operation을 history에서 지우지 않는다. 재계획은 미실행 단계를 대체하거나 보상 행동을 새로 제안한다. 사용자 목적·권한 확장은 새 contract revision과 필요한 사용자 결정을 거쳐야 한다.

### 7.2 상태 전이

```mermaid
stateDiagram-v2
  [*] --> Queued
  Queued --> Running: lease + snapshot
  Running --> WaitingApproval: action requires grant
  Running --> WaitingInput: missing essential information
  Running --> WaitingDependency: child or timer
  WaitingApproval --> Queued: durable decision
  WaitingInput --> Queued: new input revision
  WaitingDependency --> Queued: event or child result
  Running --> Verifying: candidate result
  Verifying --> Running: repair or replan
  Verifying --> Succeeded: required checks pass
  Verifying --> AnsweredUnverified: answer without objective verification
  Running --> NeedsReview: effect outcome unknown
  Verifying --> NeedsReview: conflicting evidence
  Running --> SuspendedBudget: resumable budget boundary
  SuspendedBudget --> Queued: authorized budget extension
  Queued --> Cancelled
  Running --> Cancelling
  Cancelling --> Cancelled: workers stopped or fenced
  Running --> Failed: terminal failure
```

상태 transition은 `expected_version`, `lease_epoch`, `cancel_epoch`를 비교하는 transaction으로 저장한다. terminal Run은 다시 Running으로 바꾸지 않는다. Task 재개는 새 Run을 생성해 이전 checkpoint와 evidence를 참조한다. 대기·예산 중단은 terminal 실패와 구분한다.

`TaskOutcome`과 `RunStatus`는 분리한다. Run이 끊겨도 이미 검증한 결과물은 남을 수 있다. converse 응답은 `AnsweredUnverified`로 끝나도 정상 UX이며, 자동 학습의 객관적 성공 표본으로 세지 않는다. Cancelled는 작업의 의미적 실패나 모델 품질 실패로 집계하지 않는다.

### 7.3 공통 실행 루프

```text
resume(task, checkpoint):
  claim fenced lease; load contract and pinned policy bundle
  reconcile unfinished operations before proposing new side effects
  while authorized resources remain:
    read pending owner input and cancellation epoch
    assemble context + bounded authorized memory
    select next action policy (reactive / outline / graph / procedure)
    obtain proposal from model or compiled algorithm
    validate proposal; persist decision and new checkpoint
    match proposal:
      Tool: ActionGateway.dispatch -> durable observation -> next decision
      Delegate: Supervisor.spawn -> parent WaitingDependency or continue
      PlanPatch: validate dependencies and goal preservation -> new revision
      Wait/Input: durable subscription -> release worker -> return
      Finish: VerificationService.evaluate -> success / repair / unknown
  atomically finalize + append evaluation/outbox + release reservations
```

엔진은 Rust Future를 그대로 저장하지 않는다. 재개 가능한 경계의 입력·모델 결과·operation 결과를 저장하고 다음 step을 생성한다. checkpoint가 가리키는 완료 operation을 재실행하지 않는다. 동일 prompt 재호출은 새 model attempt이며 같은 reasoning을 재현한다고 주장하지 않는다.

### 7.4 병렬성, 장기 실행, 실패 수정

독립 읽기·독립 artifact 작업은 병렬 허용한다. 같은 경로/계정 resource를 쓰는 변경은 resource lock과 expected version으로 직렬화하거나 isolated workspace에서 합친다. DAG edge는 선행성만 표현하며 무관한 노드의 `opSeq`가 실제 완료 순서를 뜻하지 않는다. happens-before는 node completion event와 dependency receipt로 검증한다.

장기 Task는 다음 wake 조건과 checkpoint, 남은 root budget, 승인 만료, policy pin을 저장한다. 작업의 장기 수명과 한 dispatch의 timeout을 분리한다. 재시작 후 외부 연결/도구 버전/permission이 바뀌면 적격성을 재검사한다. 서비스가 꺼져 있는 동안 실행되었다고 간주하지 않는다.

오류는 invalid argument / transient transport / rate limit / permission denied / missing prerequisite / artifact mismatch / semantic uncertain / outcome unknown / exhausted로 분류한다. 모델은 원인 가설과 대체 방법을 제안하고 엔진은 retry 가능성·횟수·backoff·중복 방지를 판정한다. 같은 실패 signature가 반복되면 재시도 대신 다른 방법·모델·입력 요청으로 이동한다.

## 8. 하네스 내부 알고리즘 체계

### 8.1 슬롯과 알고리즘의 위치

모든 알고리즘은 `slot + implementation_id + version + config_revision + input/output schema`로 등록한다. 초기 기본값은 단순하고 측정 가능한 정책이다. 교체는 registry의 versioned bundle 변경으로 수행하며 온라인 code injection은 사용하지 않는다.

| 슬롯 / 문제 | 초기 방법과 적용 조건 | 입력 → 출력 | 모델과 Rust의 분담 | 장단점·연결·효과 측정 |
| --- | --- | --- | --- | --- |
| intent/contract | 모델 의미 해석 + 명시 조건 추출 | 원문/출처 → 제안 intent/checks | 모델 제안, 엔진 authority·schema 검증 | 범용; 오해 가능. 목적 보존률·수정 요청률 |
| planning | ReAct형 관찰 반복, 필요 시 outline | task/context/observations → proposal/plan patch | 모델 계획, 엔진 예산·상태 | 낯선 문제 대응; 호출 비용. verified success/cost |
| dependency | topological ready queue, 조건부 branch | validated graph/receipts → ready nodes | 모델이 graph 제안, Rust cycle/binding 검사 | 병렬 효율; 동적 상황엔 replan 필요. makespan·deadlock |
| procedure reuse | applicability filter + typed binding | task features/procedure → executable intents | 모델 적합성 보완, Rust typed binding | 반복 작업 비용 절감; 과적용 위험. transfer success·false trigger |
| tool discovery | 권한 filter + lexical retrieval; 선택적 semantic rerank | intent/catalog → eligible tool set | 모델 최종 선택, 엔진 등록·schema | 작은 context에 유리; 누락 위험. recall@k·선택 성공률 |
| model routing | constraints filter + explicit priority, 이후 contextual estimator | role/features/capabilities → model/config | 모델 필요 설명, Rust 허용 집합 | 단순·재현성; 데이터 필요. same-budget success·latency |
| context packing | mandatory-first + relevance/token knapsack 근사 | ranked blocks/token budget → context snapshot | 모델 요약, Rust provenance/size | context 효율; 요약 손실. required fact retention·task success |
| memory retrieval | lexical+embedding hybrid, RRF, optional reranker | scope/query/index watermark → citations | 모델 relevance 보완, Rust ACL 선필터 | paraphrase 향상·index 비용. nDCG/Recall·누출 0 |
| memory consolidation | exact dedupe + evidence/key conflict graph | memory revisions → merge/supersede proposal | 모델 일반화, Rust lineage·원문 보존 | 중복 감소; 과일반화 위험. contradiction/false merge |
| scheduling | quota + weighted fair queue + aging + resource tokens | runnable jobs/resources → leases | 모델 분해 제안, Rust admission | starvation 억제; overhead. wait p95·GPU utilization |
| multi-agent assignment | 능력·권한·부하 기준 매칭 | subtasks/agent profiles → delegation plan | 모델 역할 설계, 엔진 제약 | 병렬 이득; 통신 비용. gain per extra model token |
| recovery | typed error rules + bounded backoff + model replan | failure/effects/budget → retry/reconcile/repair | Rust side-effect 판단, 모델 대안 | 안전한 재시도; 과보수 가능. recovery success·duplicate effects |
| verification | deterministic oracle + calibrated semantic judge | contract/artifact/receipts → check verdicts | Rust 물리 검사, 모델 의미 평가 | 신뢰 향상; judge 편향. false pass/negative·agreement |
| runtime policy selection | none baseline + deterministic ranking, 이후 ε-greedy/contextual bandit | eligible bundles/context → chosen bundle+propensity | 통계 선택, 모델은 후보 제안 | 저비용 적응; 장기 credit 한계. randomized lift·regret |
| candidate generation | failure clustering + LLM reflection + bounded mutation | trace clusters/current policy → candidate patch | 모델 가설, 엔진 변경 범위 검사 | 새로운 방법 생성; hallucination. yield of valid improvements |
| experiment allocation | paired blocked trials, 고정 n; 이후 사전 정의 sequential design | candidates/suite/budget → trial allocation | Rust 배정, 모델 결과 해석 보조 | 인과 비교; 비쌈. effect CI·evaluation cost |
| drift/rollback | task/model별 window + regression thresholds | canary metrics → keep/stop/rollback | 엔진 규칙, 모델 diagnosis | 빠른 회귀 대응; 잡음. detection delay·false alarms |
| meta-improvement | optimizer revision 간 nested held-out 비교 | optimizer history/meta suite → optimizer candidate | 모델 새 탐색법, 엔진 독립 gate | 장기 효율 개선; 과적합·비용. net gain per eval budget |

ReAct는 reasoning과 action의 상호작용을 연구한 방법이다. 여기서는 외부 관찰에 따라 다음 행동을 바꾸는 패턴을 활용하며 특정 프롬프트나 공개 reasoning format을 강제하지 않는다. [ReAct 원문](https://arxiv.org/abs/2210.03629)

### 8.2 후보 알고리즘 비교와 선택 기준

| 방법 | 적합한 상황 | 채택하지 않는 기본 사용법 |
| --- | --- | --- |
| 고정 규칙/priority | 초기 데이터가 적고 제약이 명확 | 새 작업 의미까지 문자열 규칙으로 판단 |
| contextual bandit | 한 decision slot에서 반복 선택·관측 reward·탐색 가능 | 전체 하네스 지능/새 알고리즘 생성/장기 신용할당의 대체 |
| Bayesian 추정 | 적은 데이터의 불확실성·prior 표현 | posterior가 곧 인과 증거라고 주장 |
| beam/tree search | 되돌릴 수 있는 계획·샌드박스 코드 후보 | 실제 외부 서비스를 탐색 branch마다 변경 |
| 모델 기반 reflection/search | trace의 실패 원인을 언어로 수정 가능한 prompt/절차 | 모델의 자기평가만으로 채택 |
| offline RL / sequence policy | 충분한 궤적·정의된 reward·신뢰 가능한 simulator가 확보됨 | 초기 sparse·편향 로그에서 즉시 production 정책 학습 |
| meta-optimization | 안정된 evaluation 계약과 여러 task family가 있음 | optimizer가 자기 시험지·권한·채택 threshold를 동시에 변경 |

교체의 우선 기준은 품질·자원 제약을 만족하는 가장 단순한 방법이다. 더 복잡한 방법이 추가 비용을 상쇄하는지 ablation으로 비교한다. 알고리즘 호출 실패는 reason을 기록하고 검증된 baseline으로 되돌린다. silent fallback은 금지한다.

## 9. 장기기억·경험·학습 아키텍처

### 9.1 네 종류의 저장물을 분리

1. **Conversation state:** 원문·화자·첨부·도구 메시지·요약 checkpoint. 세션의 진행 맥락이다.
2. **Knowledge memory:** 사용자 선호·검증된 사실·결정. 근거, 유효기간, 소유권을 갖는다.
3. **Episode/experience:** 특정 Task의 시도, 환경, 실패·성공, evidence, 평가. 다른 상황에 그대로 맞는 규칙이 아니다.
4. **Hypothesis/policy candidate:** 경험에서 일반화한 추정과 새로운 방법. 평가되기 전에는 사실·강제 지시가 아니다.

`learn`은 근거 기반 추출기, `reflect`는 실패 가설/후보 생성기로 정리한다. 동일 source를 저장했거나 전략을 프롬프트에 넣었다는 사실은 학습 효과가 아니다. 후속 행동 변화와 비교 실험이 있어야 개선으로 표시한다.

### 9.2 기억 계약

```text
MemoryRevision {
  memory_id, revision, kind: Fact | Preference | Decision | Hypothesis,
  body, normalized_key?, subject, claim,
  source_refs[], evidence_grade, confidence?, confidence_method?,
  owner_principal, partition, allowed_audiences[], sharing_grant_id?,
  valid_from, valid_until?, observed_at, last_confirmed_at,
  supersedes[], conflicts_with[], status,
  embedding_model_revision?, content_digest
}
Episode {
  task_id, run_id, contract_revision, environment_fingerprint,
  policy_bundle, models_used[], operation_refs[], outcome,
  evaluation_refs[], failure_classes[], costs, scope
}
```

정확한 사용자 인용은 “사용자가 그렇게 말했다”의 증거이며 그 내용의 외부 사실성까지 증명하지 않는다. 파일 내용·HTTP receipt 역시 관측 시점과 자원 버전에 한정된다. 모델의 가설 confidence는 교정되지 않았다면 calibrated probability로 표시하지 않는다.

### 9.3 검색과 갱신

접근 가능한 partition/visibility/expiry/status를 **검색 전에** 적용한다. lexical baseline을 유지하고 선택 가능한 로컬 embedding adapter를 추가한다. 초기 벡터 저장은 SQLite에 model revision·차원·float bytes를 저장하고 작은 authorized subset에서 cosine scan을 사용한다. 규모가 커지면 동일 인터페이스 뒤에 검증된 인덱스를 둔다. embedding 서버 부재 시 lexical로 degraded 실행하되 원인을 기록한다.

제안하는 기본 hybrid는 `RRF(d)=Σ_j 1/(k+rank_j(d))`이고 k와 후보 수는 suite에서 조정한다. dense 결과와 lexical 결과는 각자의 rank로 합치고 필요할 때만 reranker를 호출한다. 필수 사용자 선호는 relevance 점수와 별도 mandatory lane으로 넣되 출처와 적용 scope를 유지한다. exact identifier·코드 심볼·숫자 질의의 lexical 성능을 회귀 검사한다.

같은 key의 conflicting claim은 최신 timestamp만으로 정답을 정하지 않는다. 같은 권한을 가진 주인의 명시적 정정은 supersede할 수 있지만 서로 다른 주체·환경·시간의 사실은 병존할 수 있다. 일반화 후보는 원문 episode를 참조하고, 반례·예외 조건·검증 대상 task family를 함께 저장한다. 삭제 대신 archived/superseded를 기본으로 하되 사용자가 삭제를 요구한 데이터는 파생 embedding·요약·cache까지 추적하여 제거한다.

### 9.4 공유와 context 격리

개인, agent, project, room, external account/channel의 경계를 명시한다. 공유는 `SharingGrant(source_scope,target_audience,content_class,expiry,issuer)`로 허용한다. `scope=global` 문자열은 grant를 대체할 수 없다. 공동 작업에 필요한 artifact만 전달하고 모델에 보이는 사실이 곧 장기 저장 허가는 아니다.

기존 global 기억은 자동 삭제하지 않는다. 출처/동의가 확인되는 항목은 grant에 매핑하고 나머지는 원래 partition에서만 보이도록 migration quarantine에 둔다. 기존 명시적 사용자 공유 설정은 보존하되 해석 결과를 migration report에 남긴다.

임베딩 재생성은 content digest와 embedding version으로 멱등 처리한다. index watermark보다 최신인 기억은 lexical overlay로 검색한다. snapshot에는 실제 채택한 memory revision과 검색 policy/version, ranking score, 제외 이유를 기록해 후속 평가가 재현 가능하게 한다.

## 10. 자율개선 및 메타개선 알고리즘

### 10.1 통합 폐쇄 루프

```text
Run 수행 → evidence/usage 관찰 → 고정 oracle 평가
  → failure cluster / success pattern → 모델의 개선 가설
  → typed candidate revision → static compatibility checks
  → isolated paired experiments → held-out effect comparison
  → eligible policy bundle → bounded canary → active deployment
  → drift/regression 감시 → 유지 / rollback / 다음 가설
```

이 루프는 실행과 같은 domain/store/scheduler를 사용한다. 실험은 별도 namespace·workspace·credential policy·예산을 사용한다. 사용자 실행을 시험 환경으로 몰래 바꾸지 않는다. 이미 승인된 자동 개선 범위 안의 변경은 매번 사용자의 재지시 없이 진행한다.

### 10.2 개선 대상과 적용 범위

| 범주 | 예시 | 생성·검증·적용 경로 |
| --- | --- | --- |
| runtime configuration | context token 배분, tool retrieval k, 허용 retry/backoff, 모델 role 선택 | 허용 범위 내 후보 → offline tests → canary → 다음 Run 적용 |
| prompt/policy data | planner/reviewer prompt, bounded procedure, applicability, rubric 사용 방식 | 불변 revision + schema/권한 검사 + task suite; 평가 정답 자체 변경과 구분 |
| compiled algorithm selection | lexical→hybrid, greedy→ε-greedy, scheduler policy version | 이미 배포된 implementation registry에서 선택; compatibility hash 확인 |
| 새 알고리즘 구현 | 새로운 ranking/search/recovery/selector/optimizer Rust 코드 | isolated source branch + patch artifact + build/test/benchmark + 배포 단계; 실행 중 프로세스 직접 수정 금지 |
| evaluation 시스템 | semantic judge calibration, 새 task family oracle | 별도 evaluator release; 기존 후보와 독립 검증·기존 결과 재평가 |
| 모델 가중치 | fine-tuning/adapter/model artifact | 이후 TrainingAdapter로 확장. dataset rights·holdout·모델 registry·serving 검증 필요 |

권한/사용자 목적/secret 접근/평가 정답/실험 provenance/예산 상한은 온라인 학습 후보가 변경하지 못한다. 소스·실행파일 변경은 정책 데이터 승격과 다른 release 계약을 사용한다. 본 문서는 그 생성과 검증 절차를 설계하며 자동 바이너리 교체가 이미 존재한다고 주장하지 않는다.

### 10.3 Candidate와 Promotion 상태

```text
Draft → ValidatedSchema → SandboxTesting → Eligible → Canary → Active
            ↘ Rejected        ↘ Inconclusive       ↘ RolledBack
Active → Superseded 또는 RolledBack
```

배포 상태와 근거 등급은 다른 필드다. `Observed`, `DeterministicVerified`, `ControlledExperiment`, `HeldOutTransfer`를 분리하고 labels/provenance를 함께 보존한다. “5번 사용됨”은 표본 수일 뿐 ControlledExperiment가 아니다. 기존 레코드는 schema/provenance별로 이관한다. 단순 사용 횟수 기반 adopted/validated만 legacy_observed로 분류하고, 실제 paired 실험이 연결된 E2는 manifest·oracle·trial 근거를 보존한 채 해당 범위의 controlled evidence로 재평가한다. 문자열 등급만 보고 모두 강등하거나 모두 실험 증거로 승격하지 않는다.

후보는 target slot, parent revision, exact patch, motivation, failure/evidence refs, applicability, expected effect, risk bounds, allowed capability delta, compatibility, proposer model, generation cost, generation dataset watermark를 갖는다. 권한을 늘리는 patch는 일반 자동 개선 후보에서 거절한다.

### 10.4 contextual bandit의 정확한 역할

Bandit은 **이미 생성되고 적격성 검사를 통과한 실행 정책 중 무엇을 사용할지** 결정한다. 새로운 알고리즘의 코드나 실패 원인 설명, 실험 oracle, 배포 및 rollback을 스스로 제공하지 않는다. 따라서 전체 자율개선을 bandit 하나로 구현할 수 없다.

초기에는 단일 slot의 bundle을 arm으로 삼아 credit assignment를 단순화한다. 여러 전략을 같이 넣으면 bundle 전체가 treatment다. 개별 효과를 알고 싶으면 한 변수 ablation 또는 사전 정의 factorial 설계를 별도로 한다. 어떤 전략도 안 쓰는 `none` arm을 반드시 포함한다.

초기 학습 선택기의 구현 제안은 명시적 ε-greedy다. 적격 arm K개, greedy tie를 분배한 확률 g(a|x)에 대해 `p(a|x)=(1-ε)g(a|x)+ε/K`를 기록한다. random seed뿐 아니라 후보 집합·filter 이유·정확한 확률·선택 결과를 저장한다. ε=0이면 선택하지 않은 arm의 로그 기반 효과를 추정하지 않는다. Thompson sampling을 나중에 도입할 경우 선택 propensity가 단순 posterior sample 값이 아님을 주의하고, 정확하거나 검증된 근사 확률을 기록할 수 있을 때만 OPE에 쓴다.

기본 성공 추정은 task family/model stratum별 Beta-Bernoulli 같은 단순 모델로 시작할 수 있다. prior와 decay/window는 revision에 포함한다. 비용을 함께 비교하되 `Unknown`, 취소, 무검증 답변을 성공 0/1에 억지로 넣지 않는다. outcome label이 늦으면 pending observation으로 두고 갱신한다.

`U = success - λ_cost·normalized_cost - λ_time·normalized_latency`는 제약을 통과한 후보의 순위 보조에만 사용한다. 권한 위반·증거 위조·critical regression은 성공률로 상쇄할 수 없는 별도 gate다. quality·cost·latency는 원시 값도 함께 보고하여 가중치 조작으로 개선이 만들어지지 않게 한다.

### 10.5 실험과 인과 판단

동일 task fixture와 environment snapshot으로 baseline/treatment를 짝지어 실행하고, arm 순서는 무작위 또는 균형 배정한다. 모델 version·parameters·tool versions·grants·artifact 초기 상태를 고정한다. seed 미지원 provider는 seed 고정이 가능하다고 주장하지 않고 반복 표본으로 변동성을 측정한다.

성공률 차이는 pair의 `d_i=y_i,T-y_i,C`, `Δ=mean(d_i)`로 구한다. 같은 template의 반복은 독립 task로 취급하지 않으며 template/family 단위 cluster bootstrap 또는 사전 정한 paired 분석을 사용한다. 작은 표본에서는 폭넓은 불확실성과 Inconclusive를 허용한다. 한 후보를 수십 번 고른 뒤 같은 holdout에서 유리한 결과만 보고하지 않는다.

채택 규칙은 실험 전에 `minimum_effect`, 허용 회귀 폭, 비용/지연 상한, 표본 계획, critical cases, 통계 방법을 고정한다. 예: quality 개선 후보는 효과 하한이 사전 정의 δ보다 크고 비용 상한 내일 때만 승격. 비용 절감 후보는 품질 비열등성 경계를 만족하면서 비용이 개선되어야 한다. 숫자는 업무에 맞게 정하며 이 문서의 예시를 보편적 합격선으로 쓰지 않는다.

로그 기반 IPS/DR은 정확한 propensity, support overlap, 사전 정의 reward, 편향·누락 평가가 있을 때 **후보 우선순위**에 보조 사용한다. 현재 legacy Decision 기록에는 적용하지 않는다. DR이 항상 인과 효과를 보장하는 것은 아니며 propensity/reward model 가정·분산·effective sample size를 보고한다. [Doubly Robust Policy Evaluation and Learning](https://arxiv.org/abs/1103.4601)

### 10.6 후보 생성과 메타개선

Reflexion의 언어 피드백·episode 기억은 후보 생성의 참고가 된다. DOMINION에서는 반성 문장을 검증된 지식으로 즉시 승격하지 않고 실제 outcome에 연결된 Hypothesis로 저장한다. [Reflexion 원문](https://arxiv.org/abs/2303.11366)

GEPA의 trace 기반 반성·prompt mutation·Pareto 후보 관리 방식은 prompt/policy 탐색의 참고다. DOMINION 적용 효과는 별도 실험 대상이며 논문 수치를 프로젝트 성능으로 옮기지 않는다. [GEPA 원문](https://arxiv.org/abs/2507.19457)

메타개선은 optimizer `O_v`가 proposer prompt, mutation operator, failure clustering, 실험 예산 배정, 후보 선택 방식을 가진다는 데서 시작한다. 다른 optimizer `O_v+1`이 같은 **생성·평가 총예산**으로 얻는 held-out 개선의 크기와 비용을 비교한다. 내부 train/validation은 policy를 찾는 데, 외부 meta-test는 optimizer를 비교하는 데 사용한다.

```text
for optimizer in [incumbent, candidate_optimizer]:
  for family in meta_train_families:
    search within identical compute/eval budget
    freeze best eligible policy; do not inspect meta-test labels
  evaluate frozen policies on disjoint meta-test families
compare net held-out gain, generation cost, regressions, stability
promote optimizer only through the independent deployment gate
```

optimizer가 evaluator·holdout·권한 gate를 직접 바꿀 수 없다. 평가 알고리즘 자체도 개선 대상이지만 별도의 adjudicated gold set과 독립된 변경 경로에서 검증한다. 동시 변경은 원인 식별이 불가능하므로 분리한다. 메타 단계는 한 층의 optimizer 비교부터 시작하고 무한 재귀 자기수정은 요구하지 않는다.

### 10.7 배포와 회귀

새 Run 시작 시 활성 bundle을 pin한다. 진행 중 Run은 기존 version을 유지한다. 위험한 결함으로 revision을 폐기하면 해당 revision의 새 dispatch를 차단하고 checkpoint에서 안전한 정책으로 재계획하거나 NeedsReview로 전이한다. 권한 철회는 정책 pin보다 우선한다.

Canary는 사전에 허용된 low-risk task family·비율·기간/표본 상한으로 제한한다. production에서 baseline과 treatment의 외부 변경을 두 번 실행하지 않는다. shadow는 제안/읽기 전용 평가 또는 샌드박스에서만 한다. rollback은 registry active pointer를 이전 revision으로 CAS 변경하고, 이미 일어난 외부 변경의 복원은 별도 compensation Task로 취급한다.

### 10.8 실행 가능한 알고리즘 개선 계약

`AlgorithmRevision`은 `{id, slot, implementationId, implementationDigest, schemaVersion, parameterSchema, parameters, applicability, inputContract, outputContract, parentRevision, evidenceRefs}`다. 실행 가능한 구현 ID는 엔진의 허용 registry에서만 해석한다. 후보 JSON의 문자열을 shell/Rust로 실행하지 않는다. 새 Rust 구현은 별도 source patch/build artifact 경로를 거치며 배포 전에는 활성 구현 목록에 들어가지 않는다.

`OptimizerRevision`은 `{generatorRevision, mutationOperators, clusteringRevision, selectorRevision, allocationRule, maxCandidates, totalBudget, stopRule, trainingWatermark}`를 pin한다. 생성기·선택기·실험기·배포기는 서로 다른 책임이고 같은 optimizer가 자신의 평가 정답이나 승인 조건을 바꾸지 못한다.

내부 탐색의 구체적인 한 회차는 다음과 같다.

1. finalized observation을 event receipt로 한 번만 소비한다. 환경 오류/모델 오류/도구 오류/권한 거절/목적 불일치를 분리하고 template·model·scope별로 묶는다.
2. generator가 실패 근거를 읽어 **원인 가설, 개입 슬롯, 예상 개선/부작용, applicability, 반증 조건**을 제안한다. 단순 retry 횟수 증가나 계획 강요만으로 후보 집합을 제한하지 않는다.
3. typed validator가 권한 확대·무한 반복·알 수 없는 implementation·호환되지 않는 schema를 거절한다. incumbent와 `none`을 대조군에 남긴다.
4. 격리 smoke로 실행 불가 후보를 제거한다. 남은 후보는 사전 정한 예산 안에서 training/validation으로 탐색하고 final policy를 freeze한다. successive halving을 쓰면 exploratory score로만 취급한다.
5. frozen 후보를 untouched holdout에서 비교한다. 후보 수·중간 관찰·중단 규칙을 manifest에 고정한다. holdout 재사용 시 새 독립 검증 또는 사전 오류율 지출 정책이 필요하다.
6. deployment 서비스가 evidence digest·claim fence·compatibility·비열등성/효과·비용을 확인하고 원자적으로 canary를 등록한다. 사용자별/작업별 scope가 다른 후보를 전역 적용하지 않는다.
7. 운영 결과는 새 observation이며 원래 실험 trial에 소급해서 섞지 않는다. 회귀는 rollback 후 별도 가설을 만든다.

예산 배정은 처음에는 균등 또는 실패 빈도×복구 가치의 명시적 점수를 쓴다. Bayesian optimization은 저차원 연속 파라미터·충분한 평가 예산이 있을 때, evolutionary search는 prompt/절차의 구조적 변이가 필요할 때 후보 생성 방식으로 비교한다. offline RL은 state/action/reward 및 행동 support가 확보된 뒤의 선택지이며 현행 로그에 바로 적용하지 않는다.

메타 목적은 `G(O)=E_family[held-out quality(policy found by O)−quality(incumbent)]`와 총 탐색 비용·지연·critical 회귀를 함께 비교하는 것이다. 같은 탐색 예산에서 optimizer별 독립 seed와 family 묶음을 반복한다. raw improvement가 양수라도 비용·비열등성 gate를 위반하면 불채택이다. 메타 표본 단위는 내부 trial 수가 아니라 **독립 search run / held-out family**다. 탐색기가 평가기를 고쳐 자신의 점수를 올리는 변경은 금지한다.

불확실하면 `Inconclusive`로 남기고 예산이 끝나면 incumbent를 유지한다. “항상 성장한다”는 약속 대신 **개선 후보를 자율적으로 만들고, 실제 이득이 확인될 때만 채택하며, 손해를 되돌리는 능력**을 구현한다.

## 11. 멀티에이전트·멀티모델 협업 설계

### 11.1 AgentSpec과 Supervisor

에이전트는 이름이 다른 프롬프트에 그치지 않고 역할·모델 요구·도구 허용·기억 범위·작업 계약을 가진 실행 actor다. 영구 persona와 일시적인 worker instance를 분리한다. 동적 생성은 모델이 `AgentSpec`을 제안하고 Supervisor가 기존 principal의 권한 안에서 instance를 만든다. 생성 요청은 새 사용자 권한을 만들지 않는다.

```text
AgentSpec {
  role, instructions_revision, model_requirements,
  requested_tools, requested_memory_scopes, output_schema,
  lifetime: Linked | DetachedWithGrant
}
DelegationContract {
  parent_task_id, child_task_id, delegated_by,
  source_contract_revision, subgoal, acceptance_checks,
  inherited_partition, attenuated_grant, budget_reservation,
  artifact_inputs, output_schema, deadline, cancellation_mode
}
AgentEnvelope {
  message_id, from_agent, to_agent_or_topic, task_id,
  correlation_id, causation_id, logical_sequence,
  message_kind: Request | Progress | Evidence | Result | Critique,
  payload_ref, visibility, created_at
}
```

동료 메시지는 owner instruction으로 기록하지 않는다. 모델은 상대 의견을 검토할 수 있지만, 검증 없는 동료의 “완료”를 부모 Task 성공 증거로 쓰지 않는다. 결과에는 구조화 요약·artifact digest·operation/evaluation refs·미완료 항목이 있어야 한다.

### 11.2 자원과 권한 상속

자식 권한은 `parent grant ∩ role allowance ∩ task grant ∩ current account capability`다. toolsEnabled 같은 boolean은 표시/모델 노출 옵션으로만 사용하고 실제 권한은 이 grant로 결정한다. 자식이 부모보다 더 큰 데이터 scope를 얻을 수 없다. task_result/status도 principal 및 delegation edge ACL을 검사한다.

root ledger는 자식 예산을 예약한다. 부모가 가진 전체 10달러 중 자식에 3달러를 할당했다면 부모가 동시에 10달러를 쓰지 못한다. 자식 종료 시 미사용분을 반환하고 실제 사용은 root에 한 번만 합산한다. 위임 깊이·동시성·agent 수는 구성 가능한 budget/admission 제약이며 고정 9인 구조로 만들지 않는다.

부모가 결과를 기다릴 때는 WaitingDependency로 저장하고 worker permit을 반환한다. 자식은 동일 scheduler에서 lease를 얻는다. 현재 `start_now`의 무제한 bypass는 제거하되, 이는 자식 실행 기능 삭제가 아니라 deadlock 해소 방법의 교체다. 기본 Linked 자식은 부모 취소를 상속한다. 독립 장기 작업은 명시적 DetachedWithGrant 계약이 있을 때만 계속된다.

### 11.3 협업과 결과 통합

- 토론은 RoomSession의 공용 원문과 actor별 발언을 유지한다. 현재 owner·peer 구분과 첫 발언 공정성은 재사용한다.
- 작업 병렬화는 각 child task의 독립 workspace/artifact를 사용한다. 공유 파일 변경은 version check 후 integrator가 합친다.
- 재시도는 child attempt만 교체한다. 성공한 형제 task를 다시 실행하지 않는다.
- 부모 검증기는 자식의 산출물과 상호 의존 조건을 검사한다. 합성 단계 자체도 budget과 evaluation을 갖는다.
- 협업 이득은 단일 agent·동일 총비용 baseline 대비 성공률과 시간으로 측정한다. agent 수 증가를 성능 개선으로 간주하지 않는다.

## 12. 도구·MCP·플러그인·외부 연결 설계

### 12.1 도구 레지스트리와 공통 실행

ToolDescriptor는 stable tool ID, namespace, version, input/output schema, resource resolver, effects, approval hint, idempotency, cancellation/reconcile 지원, timeout, maximum output bytes, required account scopes를 가진다. 표시 이름은 바뀔 수 있으므로 라우팅 키로 사용하지 않는다.

`builtin:fs/write@1`, `mcp:<server_uuid>:<tool_name>@<schema_digest>`처럼 출처와 revision을 식별한다. 모델에 노출하는 짧은 이름은 Run snapshot에 매핑한다. schema 변경 후 오래된 호출은 재검증한다. 이름만 같다는 이유로 다른 서버에 실행하지 않는다.

```text
proposal (origin=model|procedure|agent|scheduler|service)
  → schema validation
  → tool/resource resolution + data classification
  → authority decision + budget reservation
  → operation prepared journal
  → approval if required (suspend, resume, revalidate)
  → dispatching marker
  → adapter invocation
  → result/receipt/artifact persistence
  → observation + task wake event
```

모델과 하네스의 도구 사용은 동일 gateway다. dev/video/channel의 모든 HTTP를 generic model tool로 공개할 필요는 없지만 **외부 변경**은 service-origin ActionProposal과 scoped service grant로 같은 journal을 거친다. 내부 순수 변환·파일 render 계산은 실행 비용만 기록하고 불필요한 사용자 승인 창을 만들지 않는다.

### 12.2 권한 계약

Effect는 `Read`, `WorkspaceWrite`, `ProcessExec`, `NetworkRead`, `RemoteWrite`, `Publish`, `CredentialUse`로 분류하고 resource를 붙인다. Tool annotation은 참고값이며 권한 부여의 근거가 아니다. HTTP GET도 민감 데이터 전송을 할 수 있으므로 method만으로 전체 위험을 판정하지 않는다.

ApprovalRequest는 principal, task/run/operation ID, exact canonical args hash, resolved resource set, tool version, policy revision, expiry, max uses를 포함한다. 승인 뒤 args·도구 버전·권한이 변하면 다시 평가한다. 실제 사용자 의도를 이미 포함하는 standing grant가 유효하면 매 호출 재승인을 요구하지 않는다. 구 `autoApproveAll`은 그대로 무한 전역 grant로 확장하지 않고 기존 사용자 설정의 범위·의미를 명시적으로 이관한다.

### 12.3 MCP

현재 client의 2024-11-05 선언을 근거로 모든 MCP 기능을 지원한다고 하지 않는다. 구현자는 지원할 protocol versions를 명시하고 해당 버전의 공식 schema와 conformance fixtures를 pin한다. 본 설계는 확인한 **2025-11-25 명세를 참조 기준 중 하나**로 사용하며 이것이 작성일의 유일한 최신 버전이라고 주장하지 않는다. [MCP 명세](https://modelcontextprotocol.io/specification/2025-11-25)

버전별 adapter가 connection lifecycle, capability negotiation/discovery, tools/list pagination, resources/prompts, notifications, cancellation, progress, structured output, reconnect를 처리한다. 2025 계열의 initialize/session 계약과 다른 세대의 lifecycle을 혼합하지 않는다. 동의하지 않은 sampling/elicitation 요청은 명시적 unsupported 또는 별도 승인 경로로 처리한다. 서버가 요청한 sampling도 ModelGateway 예산과 data policy를 통과해야 한다.

HTTP 계정 인증은 audience/resource에 묶인 token으로 처리하고 token passthrough를 금지한다. stdio credential은 제한된 env/secret reference로 전달한다. 모든 MCP 자격증명을 공통 API 키 하나로 축약하지 않는다. [MCP Authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)

프로세스 풀과 요청 multiplexing은 server capability 및 thread safety에 따라 사용한다. 모델별 도구 검색은 전체 catalog에서 권한을 적용한 뒤 수행하며 선택 subset에 없던 도구를 찾을 `discover_tools`도 제공한다. “관련 도구 10개”를 영구 능력 상한으로 두지 않는다.

### 12.4 플러그인·스킬·브라우저·채널

PluginManifest는 package ID/version, compatible engine ABI/protocol, tools/capabilities, requested permissions, secret references, install digest, enabled scopes를 갖는다. 초기 플러그인은 manifest+MCP/별도 process adapter 조합을 권장한다. 안정된 Rust ABI가 없는 임의 dylib hot load는 기본 확장 방식으로 채택하지 않는다. WASM은 나중에 제한된 host API가 필요할 때 비교한다.

SkillRevision은 지침·artifact template·선택적 procedure·provenance를 담는 데이터 패키지다. 활성화는 실행 권한 부여가 아니며 외부 텍스트가 secret이나 system policy를 변경하지 못한다. 안정된 native browser/computer-control adapter는 이번 조사 범위에서 찾지 못했다. 향후 ToolAdapter로 추가하고 observation(화면/DOM), action target, before/after evidence, cancellability를 같은 operation 계약에 맞춘다.

채널은 `InboundEnvelope → Task submission`, `Response artifact → Outbox delivery`만 담당한다. `connectionId + remoteEventId` unique로 중복 수신을 막는다. outbox는 binding revision과 cancel epoch를 확인하며 불확실한 전송은 자동 중복 발송하지 않는다. Discord/Telegram의 현재 브리지 원장·offset·echo 방지는 이 adapter의 기반이다. 나머지 manifest는 availability=false로 유지하고 실제 adapter·검증이 있어야 활성화한다.

## 13. 실행 권한·상태·검증·장애 복구

### 13.1 Operation 상태와 crash window

```text
Proposed → Prepared → AwaitingApproval → Ready → Dispatching
Dispatching → Succeeded | Failed | OutcomeUnknown
Prepared/Ready → Cancelled | Denied | Expired
OutcomeUnknown → Reconciling → Succeeded | Failed | NeedsReview
```

| crash 지점 | 저장된 사실 | 재개 행동 |
| --- | --- | --- |
| Prepared 이전 | 외부 실행 안 함 | proposal을 다시 검증 가능 |
| Prepared/승인 대기 | 실행 의도만 있음 | 승인/만료·권한 확인 후 이어감 |
| Dispatching 저장 후, 호출 직전 | 실제 호출 여부를 DB만으로 모름 | idempotency 지원이면 같은 key, 아니면 reconcile/NeedsReview |
| 외부 성공 후 receipt 저장 전 | 효과는 생겼을 수 있음 | readback/remote idempotency로 확정; 임의 재전송 금지 |
| receipt 저장 후 observation 전 | 결과가 있음 | DB transaction으로 observation/wake 재생성 |
| evaluation commit 후 consumer 전 | outbox가 있음 | idempotent consumer가 재처리 |

SQLite transaction과 외부 API 호출을 하나의 원자 transaction으로 만들 수 없다. **외부 exactly-once를 일반적으로 보장한다고 주장하지 않는다.** 멱등 키를 실제 remote API가 지원하는 경우와 로컬 dedupe만 있는 경우를 구분한다. 파일 쓰기는 staging/temp+fsync+rename 및 expected digest로 재조정할 수 있지만 모든 shell command는 그렇게 할 수 없다.

### 13.2 취소와 lease

Run·Task·root cancel epoch를 durable하게 증가시키고 cooperative token으로 빠르게 알린다. gateway는 승인 후와 dispatch 직전에 epoch를 다시 확인한다. shell은 process group 종료 및 wait, MCP는 지원하는 cancellation을 전송하고 bounded cleanup을 수행한다. 외부 취소가 보장되지 않는 요청은 OutcomeUnknown으로 남긴다.

Scheduler lease에는 worker_id, expires_at, fencing_epoch가 있다. heartbeat 실패로 lease를 잃은 worker는 새 operation을 시작하지 못한다. DB commit도 fencing epoch를 검사한다. 외부 서비스가 fencing을 지원하지 않으면 이미 출발한 요청을 막을 수 없으므로 lease 만료만으로 동일 operation을 재실행하지 않는다.

Task가 WaitingApproval/Dependency/Timer일 때 worker permit을 해제한다. 승인 자체는 process-local oneshot의 생존과 무관하게 API transaction으로 저장된다. 이벤트 bus는 깨우기 최적화이고 polling/recovery로 다시 찾을 수 있어야 한다.

### 13.3 Verification 계약

각 CheckSpec은 id, origin(owner/inferred/policy), mandatory 여부, artifact/resource target, oracle type+version, expected condition, baseline digest, evidence freshness를 가진다. 결과는 Pass/Fail/Unknown/NotApplicable이며 판정 이유와 evidence refs를 저장한다.

- 파일 oracle: path authorization, symlink/TOCTOU 방지, digest/version, 존재·내용·변경 확인.
- tool oracle: 성공 receipt와 **어떤 결과를 보장하는지** 제한. tool 성공을 업무 성공으로 확대하지 않음.
- procedure oracle: instance/revision/node별 실제 receipt, resolved argument digest, dependency 완료 이후 dispatch 여부, postconditions 확인.
- semantic oracle: rubric revision, judge model/config, source/citation 검사, blind evaluation, calibration 기록. 이 결과를 deterministic로 표시하지 않음.
- human oracle: principal과 exact evaluated artifact revision을 보존. 후속 artifact 변경에 이전 판정을 재사용하지 않음.

필수 Check에 Fail이 있으면 성공 불가, Unknown이면 요구한 신뢰도에 따라 NeedsReview/추가 검증, NotApplicable은 applicability 자체가 검증되어야 성공 조건에서 제외할 수 있다. adaptive strategy 미준수는 독립적인 사용자 목적이 달성됐으면 Task 실패와 분리한다. owner의 mandatory 절차 위반은 실패다.

종합 결과만 저장하지 않고 check별 결과를 남긴다. mixed compliance summary는 `Violated > Unknown > Followed > NotTriggered > NotApplicable`처럼 설명 가능한 규칙을 사용하되, mandatory와 adaptive 결과를 따로 집계한다. 하나의 followed가 다른 위반을 가리지 못하게 한다.

### 13.4 운영 권한과 다중 디바이스

단일 로컬 principal을 첫 배포 기본값으로 유지한다. 원격 접근은 별도 모드로 인증, device pairing/revocation, TLS 또는 인증된 tunnel, 읽기·쓰기·이벤트·artifact별 ACL을 제공한 뒤 연다. 현재 loopback bind를 0.0.0.0으로 바꾸는 것만으로 지원하지 않는다.

다중 디바이스는 처음에 **한 authoritative engine에 여러 UI가 접속**하는 모델이다. SQLite 파일을 여러 기기에서 동기화해 동시에 실행하지 않는다. 동시 명령은 idempotency key와 expected_version으로 충돌 처리한다. read-only observer와 executor를 분리하고 이벤트 구독도 scope filter를 적용한다.

Desktop-owned 프로세스는 앱 종료와 함께 끝날 수 있다. 지속 실행을 원하는 mode는 별도 service lifecycle로 관리하며 UI에 active/suspended 상태를 정확히 표시한다. shutdown은 신규 lease 중지 → 실행 drain/cancel → checkpoint flush → process 종료 순서로 처리한다.

## 14. Rust 모듈·trait·데이터·API 계약

### 14.1 목표 모듈 구조

아래는 **최종 목표 경계**다. 한 번에 폴더를 전부 만들거나 복제 엔진을 추가하지 않는다. 현재 모듈에서 책임을 추출하며 compatibility facade를 유지한다.

```text
src/
  domain/           ids, task, run, operation, evidence, authority, budget, event, error
  application/      submit, resume, cancel, approvals, query projections
  engine/
    runtime/        coordinator, reducer, checkpoint, finalizer, recovery
    models/         gateway, profiles, router, resource admission
    context/        composer, compaction, provenance
    tools/          registry, gateway, authorization, resource resolution
    memory/         repository service, retrieval, consolidation, sharing
    policies/       planner, procedure, recovery, selector, scheduler policies
    adaptation/     candidates, experiments, evaluator, registry, promotion, meta
    agents/         supervisor, delegation, messaging, integration
    jobs/           scheduler, leases, subscriptions, quotas
  adapters/
    providers/      openai_compat, provider-specific protocols, local runtime
    tools/          fs, process, http, github, browser, mcp
    channels/       discord, telegram, bridge
    services/       video, dev studio, skill packages
  storage/          sqlite repositories, migrations, artifact blobs, outbox
  api/              existing facade + v2 task/run/event/policy APIs
  bootstrap.rs      composition root; registry/adapters wiring
```

### 14.2 핵심 타입과 trait

다음 Rust는 **구현 계약을 보여주는 인터페이스 초안**이다. 참조하는 domain 타입과 구현체를 생략했으므로 복사만으로 컴파일되는 새 엔진 코드가 아니다. 구현 단계에서 DTO/schema와 함께 컴파일·conformance test를 작성한다. dyn dispatch가 필요한 async trait은 boxed future를 사용한다.

```rust
use std::{pin::Pin, sync::Arc};
use futures_util::{future::BoxFuture, Stream};

pub type ModelStream = Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send>>;

pub struct RunContext {
    pub task_id: TaskId,
    pub run_id: RunId,
    pub principal: PrincipalId,
    pub contract: Arc<TaskContract>,
    pub policy: Arc<PolicyBundle>,
    pub grant: GrantSnapshot,
    pub budget: BudgetAccountId,
    pub lease: LeaseFence,
    pub cancel: CancelHandle,
}

pub enum ActionProposal {
    CallTool(ToolIntent),
    Delegate(DelegationProposal),
    RevisePlan(PlanPatch),
    Ask(InputRequest),
    Wait(WakeCondition),
    Finish(ResultProposal),
}

pub trait ProviderAdapter: Send + Sync {
    fn describe(&self) -> AdapterDescriptor;
    fn start<'a>(&'a self, request: ModelRequest, ctx: ModelCallContext)
        -> BoxFuture<'a, Result<ModelStream, ModelError>>;
}

// Algorithm cannot invoke a raw HTTP client or persist arbitrary domain records.
pub trait DecisionPolicy: Send + Sync {
    fn descriptor(&self) -> PolicyDescriptor;
    fn propose<'a>(&'a self, input: DecisionInput, models: &'a dyn ModelPort)
        -> BoxFuture<'a, Result<DecisionOutput, PolicyError>>;
}

pub trait ToolAdapter: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    fn invoke<'a>(&'a self, call: AuthorizedOperation, cancel: CancelHandle)
        -> BoxFuture<'a, Result<ToolReceipt, ToolError>>;
    fn reconcile<'a>(&'a self, pending: PendingOperation)
        -> BoxFuture<'a, Result<ReconcileResult, ToolError>>;
}

pub trait VerificationPort: Send + Sync {
    fn evaluate<'a>(&'a self, request: EvaluationRequest)
        -> BoxFuture<'a, Result<Evaluation, EvaluationError>>;
}

pub trait MemoryPort: Send + Sync {
    fn retrieve<'a>(&'a self, query: AuthorizedMemoryQuery)
        -> BoxFuture<'a, Result<MemorySnapshot, MemoryError>>;
}

pub trait RunRepository: Send + Sync {
    fn transition<'a>(&'a self, command: TransitionCommand)
        -> BoxFuture<'a, Result<TransitionReceipt, StoreError>>;
    fn finalize<'a>(&'a self, command: FinalizeCommand)
        -> BoxFuture<'a, Result<FinalizeResult, StoreError>>;
}

pub trait CandidateGenerator: Send + Sync {
    fn propose<'a>(&'a self, batch: FailureBatch, budget: SearchBudget)
        -> BoxFuture<'a, Result<Vec<PolicyCandidate>, PolicyError>>;
}

pub trait ExperimentRunner: Send + Sync {
    fn evaluate<'a>(&'a self, manifest: FrozenExperimentManifest)
        -> BoxFuture<'a, Result<ExperimentReport, ExperimentError>>;
}
```

`AuthorizedOperation`의 생성자/필드는 gateway 모듈 밖에서 임의 구성할 수 없게 한다. Adapter가 repository나 global AppState를 통째로 받지 않게 하고 필요한 resource/credential handle만 부여한다. RunContext의 grant snapshot은 현재 철회 여부를 우회하지 못한다.

`CancelHandle`은 현재 epoch 검사와 Tokio 신호를 감싸는 내부 타입으로 시작할 수 있다. 필요하면 `tokio-util::CancellationToken`을 새 의존성으로 검토한다. JSON Schema validator, tracing, vector index도 필요한 단계에서 dependency ADR을 남긴다. 현행 Cargo.toml에 이 라이브러리가 이미 있다고 가정하지 않는다.

### 14.3 구체적인 데이터 계약

| 타입 | 필수 필드 | 검증 / 불변조건 |
| --- | --- | --- |
| TaskContract v2 | schemaVersion, taskId, revision, principal, sourceRefs, intent, acceptance[], permissionGrant, budget, parentTaskId?, family? | sourceRefs 실제 존재·읽기 권한, mandatory 조건 출처, 전체 content digest |
| PolicyBundle | revision, slots:{slot→implementation/config revision}, compatibility, activatedBy, effectiveFrom | 등록된 구현만, dependencies 만족, immutable |
| SelectionSnapshot | contractRevision, policyBundle, model configs, tool schemas, memory refs/watermark, context artifact hash, decisions[], environment | 실제 실행 직전 동결, secrets와 비공개 reasoning 제외 |
| DecisionRecord | id, run/step, slot, inputHash, candidates/filterReasons, selectedBundle, actualPropensity?, rngInfo, policyRevision | deterministic 선택과 randomized 선택 구분; 없는 확률을 만들어 넣지 않음 |
| Operation | id, runId, stepId, toolRevision, origin, argsDigest, resourceSet, grantId, approvalId?, idempotencyKey, status, attempt, timestamps, receiptRef? | execute 전 prepared 저장; model call ID와 별도 identity |
| ProcedureNodeKey | procedureInstanceId, procedureRevision, nodeId | node id가 다른 절차와 충돌하지 않음 |
| Evaluation | id, task/run, evaluatorRevision, contractRevision, checkResults[], outcome, evidenceRefs, labelProvenance, payloadDigest | 판정 변경은 새 evaluation revision; 원자료 덮어쓰기 금지 |
| Observation | id, decisionId, runId, evaluationId, regime, costs, attributionUnit, observedAt | regime별 집계 분리; delayed label 지원 |
| PolicyCandidate | revision, parent, slot, patch, scope, provenance, hypothesis, constraints, compatibility | 신규 runtime code는 release artifact 참조 |
| Deployment | id, candidateRevision, baselineRevision, scope/strata, canaryPlan, gateReport, state, expectedVersion | promotion 원자 CAS; 과거 active pointer 기록 |

Task 예시(설계용, 신규 API):

```json
{
  "schemaVersion": 2,
  "sessionId": "session-user-1",
  "input": "자료를 조사해서 보고서를 작성하고 검증해 줘",
  "mode": "background",
  "acceptance": [
    {"kind": "artifact_exists", "path": "report.md", "required": true},
    {"kind": "semantic_rubric", "rubricId": "cited-report-v1", "required": true}
  ],
  "budget": {"maxModelTokens": 120000, "maxToolCalls": 100, "maxWallSeconds": 3600},
  "grantRef": "user-project-research-grant",
  "modelPreference": {"providerId": "local", "modelId": "selected-model", "fallbackPolicy": "configured"}
}
```

principal, run outcome, actual cost, evidence grade는 클라이언트 입력으로 받지 않는다. 가격 예산이 없더라도 token/time/tool 예산으로 제한할 수 있다. 사용자 입력의 성공 조건을 더 약한 inferred 조건으로 덮어쓰지 않는다.

### 14.4 SQLite 저장과 원자성

설정·legacy UI records는 유지하고 실행의 핵심 key와 상태부터 typed table로 옮긴다. `Store`를 당장 전체 ORM으로 교체할 필요는 없다. 한 writer actor 또는 bounded blocking executor에 rusqlite 쓰기를 모아 async runtime thread의 긴 blocking을 줄인다. 네트워크/LLM 대기 중 transaction을 유지하지 않는다.

다음은 목표 스키마의 키·index 계약이다. 기존 records/event_log/lease와 중복된 권위 테이블을 한 번에 만들지 않는다. 같은 불변조건을 현재 테이블의 unique index·transaction·typed access로 만족하면 우선 재사용하며, 스키마 이동 시 한쪽만 write authority로 둔다. payload는 schemaVersion을 가진 canonical JSON이다. 실제 migration에서는 enum CHECK, FK, retention/backup과 기존 레코드 변환을 포함해야 한다.

```sql
CREATE TABLE tasks (
  id TEXT PRIMARY KEY, principal_id TEXT NOT NULL, contract_revision TEXT NOT NULL,
  state TEXT NOT NULL, version INTEGER NOT NULL, root_id TEXT NOT NULL,
  parent_id TEXT, payload TEXT NOT NULL
);
CREATE TABLE runs (
  id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id),
  state TEXT NOT NULL, version INTEGER NOT NULL, policy_revision TEXT NOT NULL,
  cancel_epoch INTEGER NOT NULL, checkpoint_seq INTEGER NOT NULL, payload TEXT NOT NULL
);
CREATE TABLE operations (
  id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id),
  state TEXT NOT NULL, idempotency_key TEXT NOT NULL,
  lease_epoch INTEGER NOT NULL, payload TEXT NOT NULL,
  UNIQUE(run_id, idempotency_key)
);
CREATE TABLE run_events (
  global_seq INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL,
  run_seq INTEGER NOT NULL, event_type TEXT NOT NULL, payload TEXT NOT NULL,
  UNIQUE(run_id, run_seq)
);
CREATE TABLE evaluations (
  id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id),
  evaluation_revision TEXT NOT NULL, payload_hash TEXT NOT NULL, payload TEXT NOT NULL,
  UNIQUE(run_id, evaluation_revision)
);
CREATE TABLE outbox (
  id TEXT PRIMARY KEY, topic TEXT NOT NULL, payload TEXT NOT NULL,
  available_at TEXT NOT NULL, delivered_at TEXT
);
CREATE TABLE consumer_receipts (
  consumer TEXT NOT NULL, event_id TEXT NOT NULL, projection_version TEXT NOT NULL,
  PRIMARY KEY(consumer, event_id, projection_version)
);
CREATE TABLE policy_revisions (
  revision TEXT PRIMARY KEY, slot TEXT NOT NULL, parent_revision TEXT, payload TEXT NOT NULL
);
CREATE TABLE deployments (
  id TEXT PRIMARY KEY, scope_key TEXT NOT NULL, slot TEXT NOT NULL,
  revision TEXT NOT NULL REFERENCES policy_revisions(revision),
  state TEXT NOT NULL, version INTEGER NOT NULL, payload TEXT NOT NULL
);
CREATE INDEX operations_by_run_state ON operations(run_id, state);
CREATE INDEX tasks_by_state ON tasks(state, root_id);
CREATE INDEX outbox_due ON outbox(delivered_at, available_at);
```

추가 저장 계약: `job_leases(job_id PK,worker,epoch,expires_at)`, `wake_subscriptions(id PK,task_id,event_key,deadline)`, `approvals(id PK,operation_id,args_hash,state,version,payload)`, `budget_accounts(id PK,parent_id,version,limits,spent,reserved)`, `budget_reservations(id PK,account_id,operation_id,state,payload)`, `decision_records(id PK,run_id,slot,payload)`, `observations(id PK,decision_id,evaluation_id,regime,payload)`, `memory_revisions(id,revision PK,payload)`, `artifacts(id PK,digest,scope,storage_ref)`, `experiment_manifests(id PK,digest,payload)`, `trial_results(experiment_id,trial_id PK,payload_hash,payload)`가 필요하다. 모델·provider·grant·agent catalog는 초기에는 versioned records로 유지할 수 있다.

`finalize` transaction은 expected Run version/lease 확인 → stable evaluation insert → terminal state update → observation/outbox insert를 수행한다. 동일 `(run,evaluation_revision)`와 동일 semantic payload이면 AlreadyApplied, 다른 payload면 Conflict로 반환한다. `finishedAt` 같은 재시도마다 달라지는 값은 identity hash에서 제외하거나 최초 값을 재사용한다.

정책 통계는 full record read-modify-write를 transaction 밖에서 수행하지 않는다. consumer receipt의 unique key를 먼저 확인하고 **동일 transaction 안에서** observation의 delta를 적용한다. projector를 새 버전으로 재생하면 이전 revision 통계를 덮어쓰지 않고 병렬 projection으로 비교한다. 모든 저장 오류를 typed Result로 반환하고 기록 실패를 성공으로 흡수하지 않는다.

### 14.5 API와 이벤트 계약

| 신규 API | 동작 / 필수 계약 |
| --- | --- |
| `POST /api/v2/tasks` | Idempotency-Key + body digest; task와 초기 job/event 원자 생성; 202 taskId/runId |
| `GET /api/v2/tasks/:id` | principal ACL, version, outcome, waits, current run, budget |
| `POST /api/v2/tasks/:id/cancel` | expectedVersion, cancelMode; epoch 증가; 이미 완료면 no-op 결과 |
| `POST /api/v2/tasks/:id/resume` | checkpoint/contract/budget 변경 확인, 새 Run; 불확실한 효과 무시 불가 |
| `POST /api/v2/approvals/:id/resolve` | expectedVersion + allow/deny + argsHash; durable CAS; replay 멱등 |
| `GET /api/v2/runs/:id/events?after=` | sequence 순서, 권한 filter, snapshot fallback |
| `GET /api/v2/runs/:id/operations` | receipt/effect/unknown을 별도 표시, secret redaction |
| `GET /api/v2/tools` / `models` | 권한 적용 catalog와 capabilities, availability 이유 |
| `GET /api/v2/policies/:revision` | exact immutable policy와 평가 근거 |
| `POST /api/v2/experiments` | frozen manifest 생성; 격리·quota gate; runtime production과 분리 |
| `POST /api/v2/deployments` | 평가 gate report와 expected active revision 필요 |

ID key가 같고 body가 다르면 409, 유효하지 않은 계약 422, 권한 부족 403, budget/resource queue 정책에 따라 202 대기 또는 429, 지원하지 않는 capability는 설명 가능한 structured error다. 오류 schema는 `{code,message,retryable,effectCertainty,operationId?,details}`다. 문자열에 “중단”이 포함되는지로 취소 상태를 판정하는 현재 코드는 typed error로 전환한다.

이벤트 envelope는 `{schemaVersion,eventId,globalSeq,runSeq,taskId,runId,causationId,occurredAt,type,payload}`다. 상태/권한/결과 이벤트는 DB commit 후 발행한다. token delta는 임시 스트림으로 허용하되 최종 message/artifact가 권위 자료다. bounded queue가 넘치면 delta를 합치거나 버리고 `resync_required`를 보내며 durable event는 cursor로 복구한다.

기존 `/api/chat`, `/api/jobs`, `/api/goals`, Room API는 facade로 유지한다. `chat::run_turn` compatibility adapter가 새 coordinator를 호출하도록 단계적으로 이동한다. 현행처럼 화면/SSE 연결 해제를 Task 취소로 취급하지 않는다. 명시적인 ephemeral 실행만 별도 계약으로 취소할 수 있으며 durable Task는 연결과 무관하다. API version 전환 전후의 SSE `done/error/assistant_saved` 계약을 regression test로 고정한다.

### 14.6 마이그레이션과 배포

1. DB·artifact·schema version·소스 fingerprint를 백업하고 읽기 전용 검증을 한다.
2. additive table과 typed parser를 추가한다. unknown legacy enum은 quarantined로 보존한다.
3. legacy→domain 변환과 read projection을 수행하며 원본 records를 삭제하지 않는다.
4. 한 Run은 legacy 또는 새 coordinator **하나만** 소유한다. routing version을 영속화한다.
5. 같은 transaction의 domain write와 legacy UI projection으로 한시 호환한다. 두 엔진의 이중 side-effect 실행은 금지한다.
6. 기존 running job은 drain하거나 명시적으로 NeedsReview 처리 후 새 Run으로 재개한다. 불완전 legacy evidence를 새 성공으로 승격하지 않는다.
7. 회귀/성능 검증 후 read path를 옮긴다. 구 데이터는 retention 정책에 따라 보존하고 rollback은 호환 바이너리·DB backup 쌍으로 검증한다.

schema migration은 번호·checksum·시작/완료·복구 상태를 기록한다. 새 schema가 옛 바이너리에 호환되지 않으면 자동 binary rollback을 중지하고 백업 복원을 안내하는 운영 계약이 필요하다.

## 15. 실제 작업의 종단 간 실행 시나리오

### 15.1 낯선 복합 작업 → 실패 → 개선 → 전이

예시 요청: “새로운 공개 API에서 자료를 모아 표와 한국어 보고서를 만들고, 출처와 계산을 검증해 프로젝트 폴더에 저장해 줘.” 이 사례는 특정 파일을 쓰는 DAG가 이미 저장되어 있지 않다고 가정한다. 외부 게시·메일 발송은 요청에 없으므로 Task grant에 포함하지 않는다.

| 단계 | 모델 판단 | Rust의 책임 / 내부 흐름 | 알고리즘 / 남는 증거 |
| --- | --- | --- | --- |
| 1. 수신 | 사용자가 요구한 산출물·범위 해석 | Ingress가 principal/source를 검증, Contract v1과 전체 예산 저장 | 의도 추출, 명시/추론 조건 구분, TaskSubmitted |
| 2. 초기 계획 | API 문서 확인→수집→정규화→집계→보고서→검증 outline 생성 | Coordinator가 PlanRevision p1 저장; 목적과 권한 확장 여부 검사 | 모델 계획 정책, plan artifact |
| 3. 실행 환경 | 필요한 HTTP·파일·계산 도구 선택 | Registry 권한 filter, gateway schema 검사, isolated work dir 제공 | tool retrieval, context packing, DecisionRecords |
| 4. API 탐색 | 문서와 첫 응답을 보고 pagination·schema 파악 | ModelGateway 호출과 HTTP operation 예약·저장·실행 | Reactive loop, raw response digest·source URL |
| 5. 실패 발생 | 429/누락 필드/중복 행을 보고 가설 생성 | transport/error classifier가 transient와 data mismatch를 구분; 완료 선언 차단 | failure signature, response code, check Fail |
| 6. 복구 | pagination cursor 보존, backoff 후 재개, dedupe 기준 수정 제안 | RecoveryPolicy가 허용 retry·시간 예산 확인; 기존 수집 receipt 재사용 | bounded backoff + replan p2, 새 operation IDs |
| 7. 협업 | 자료 검토와 보고서 초안을 병렬로 맡길지 판단 | Supervisor가 두 subtask에 grant/예산 분할; 부모 wait 시 permit 반환 | assignment policy, child evidence refs |
| 8. 결과 통합 | 표의 설명·예외·출처를 종합 | integrator가 artifact digest와 버전 충돌 확인; 산출물 저장 | artifact versioning, parent result proposal |
| 9. 검증 | 의미 rubric과 근거 충분성을 reviewer가 검토 | 행 수/합계/파일/citation oracle 실행; 필수 Fail은 repair | deterministic+semantic Evaluation, provenance 분리 |
| 10. 완료 기록 | 사용자에게 결과와 한계 설명 | finalization transaction으로 outcome·cost·evidence·outbox 저장 | verified result와 response artifact |
| 11. 실패 분석 | 실패 trace를 묶어 “API pagination 관측 후 계획 확정” 가설 생성 | adaptation consumer가 Hypothesis와 Candidate c1 저장 | failure clustering + reflection, 원문 episode 링크 |
| 12. 통제 실험 | 후보 prompt/절차를 제안하되 시험 정답을 보지 않음 | 샌드박스 API fixture baseline/c1 paired trials, 모델/환경 고정 | trial assignment, blind checks, effect CI |
| 13. 채택 | 모델은 보고서 해석을 보조 | PromotionGate가 품질·비용·critical criteria 확인, canary 후 active | Deployment d1, candidate/baseline revisions |
| 14. 다른 작업 | 다른 공개 API의 구조를 다시 탐색하고 추상 전략의 적용 여부 판단 | 새 Task의 context와 적격성만 공유; 이전 URL/credential/행 데이터를 복사하지 않음 | held-out transfer, applicability/different schema 검증 |

마지막 작업에서 “endpoint가 바뀌었지만 pagination 관측 전략이 유효했다”는 결과가 있어야 전이의 근거가 된다. 정답 경로와 내용이 같은 파일 생성만 반복한 성공은 전이 증거가 아니다.

### 15.2 모델 능력에 따른 같은 Task의 차이

| 상황 | 추가 지원 | 동일하게 유지되는 기반 |
| --- | --- | --- |
| 도구 호출·계획이 안정적인 고성능 모델 | native calls, 더 넓은 context, 독립 작업 병렬 제안; 명시 planner 생략 가능 | Task 계약, 권한, 예산, operation, 결과 검증 |
| 작은 모델이 인자를 자주 잘못 작성 | 짧은 tool subset, schema 오류 feedback, subgoal, 검증된 변환 단계 assistance | 동일 Task/API. 실패하면 대체 모델 또는 입력 요청 |
| vision 미지원 모델에 이미지 첨부 | 허가된 vision adapter/모델 또는 OCR 도구로 실제 관찰 후 텍스트 전달 | 이미지 내용을 읽었다고 거짓 표시하지 않음 |
| 로컬만 허용 | 로컬 provider/embedding만 적격, 리소스 queue와 작은 context 보조 | 클라우드 fallback을 자동 사용하지 않음 |
| 고비용 검토가 이득 없는 단순 대화 | 직접 응답, 필요 없는 planner/reviewer/experiment 생략 | 응답은 AnsweredUnverified, 출처와 비용은 기록 |

### 15.3 사용자 개입 없이 장기 수행

“허가한 폴더의 자료를 정리하고 다음 날 새 자료가 오면 이어서 처리”는 Task에 wake subscription과 유효 grant, 기간/누적 budget을 둔다. 파일 이벤트가 오면 scheduler가 새 Run을 깨우고 이미 처리한 content digest는 건너뛴다. 사용자 개입은 범위가 바뀌거나 필수 정보·권한이 부족할 때만 필요하다. 시간 경과가 새 권한이나 예산 연장을 뜻하지 않는다.

### 15.4 외부 변경 후 장애

승인된 서비스 변경 API가 성공했지만 응답을 받기 전에 process가 죽으면 Operation은 OutcomeUnknown이다. 재시작한 Coordinator는 `reconcile(operation_id, remote_idempotency_key)`를 먼저 요청한다. readback이 일치하면 receipt를 확정하고 다음 단계로 간다. 일치 여부를 판정할 수 없으면 NeedsReview로 멈추고 같은 변경을 반복하지 않는다. 실패 원인 보고와 정책 개선 후보 생성은 가능하지만 불확실성을 숨겨 성공 표본으로 삼지 않는다.

## 16. 기존 구현 재사용·통합·교체 방안

| 현행 파일 / 연결점 | 유지할 자산 | 변경·목표 소유자 |
| --- | --- | --- |
| `bootstrap.rs::serve`, `main.rs`, launcher/platform | 실행·복구·sidecar 진입 | coordinator/supervisor lifecycle 조립; 플랫폼 설치는 engine 밖 |
| `chat/runtime.rs::run_turn_with_policy`, `run_turn_body` | 현행 사용자 턴·도구 loop·acceptance·stream 계약 | RunCoordinator로 책임 추출. 별도 새 loop 복제 금지 |
| `chat/runtime.rs::dispatch_tool_call` | model/assisted 공통 경계 | ActionGateway의 authority/budget/journal/receipt/reconcile |
| `chat/runtime.rs::record_selection_snapshot`, `finalize_execution` | bundle identity·context manifest·finalize 연결 | DecisionRecorder/Finalizer. check provenance·원자 commit 보강 |
| `gateway.rs`, `chat/provider.rs`, `llm.rs`, `compact.rs`, `models.rs` | 공통 모델 transport·local lifecycle | capability registry와 root usage/reservation 추가; provider별 native 기능 보존 |
| `chat/context.rs`, `memory.rs`, `experience.rs` | context packing·lexical·revision | ContextAssembler/MemoryRepository, source grants·optional hybrid |
| `learning.rs`, `reflect.rs` | 추출·반성 trigger | MemoryCandidate/FailureHypothesis 생성; 검증 전 global 사실화 제거 |
| `improve.rs` schedule/apply, `policy.rs`, `policy_patch.rs` | 실험 예약·후보·planning/recovery 소비 | typed 슬롯·불변 revision·실험 근거·scope deployment 원자화 |
| `plan_quality.rs` | 저비용 구조 진단 | 보조 metric으로 유지; 모든 정책의 필수 성능 gate에서는 분리 |
| `procedure.rs` | validated DAG·ready·receipt·edge 평가 | 선택적 ProcedurePolicy/PlanAdapter; ActionGateway만 실행 권한 소유 |
| `goals.rs`, `workflows.rs` | 장기 목표·review/repair/replan·checkpoint | Task frontend/PlanAdapter. 일반 대화와 같은 실행·복구 기반 |
| `jobs.rs`, `run_state.rs`, `lease.rs`, `wake.rs`, `scheduler.rs` | queue·lease·wake·실험 시작 | Scheduler/Supervisor·fencing·durable waits·linked cancellation |
| `tools.rs` delegate, agents/rooms | 에이전트·페르소나·그룹 흐름 | AgentSupervisor와 parent envelope/grant/budget; preset 제한 없음 |
| `tools.rs`, `mcp.rs`, extensions 관련 API | 실제 builtins/MCP/설치 기록 | stable ToolRegistry, contract adapter, discovery·effect classification |
| `discord.rs`, `telegram.rs`, bridge/inbox/outbox | 외부 채널과 room 연결 | ChannelAdapter+명시 channel-room binding, 메시지 멱등키·scope |
| dev/video 및 OAuth/vault 서비스 | 기존 서비스와 credential 연결 | 작업은 Run/Operation, 비밀은 CredentialBroker, 영상 upload 별도 effect |
| `db.rs::create_attempt`, `finalize_once`, delta merge | 기존 transaction·멱등성과 기록 | typed repository, lease CAS, atomic deployment/state/event/outbox |
| `events.rs` + `web/src/api.ts` | durable log/replay·stream UI | DB cursor·schema 생성·오류/재연결 처리; 운영 상태의 source는 Rust |
| `api/experiments.rs`, `api/benchmark.rs`, benchmark binary | 격리 실행·identity·실제 trials | frozen manifest·raw trial 검증·ITT/cluster 통계·claim CAS |
| maintenance/backup/rotation | 백업·복원·키 회전 경로 | 변경 전후 integrity·restore drill, daemon supervision |
| web + src-tauri + CI | React/Vite/Tailwind·Rust sidecar·build/release | 기능 parity와 typed client. Node 실행엔진/Electron 재도입 없음 |

재사용은 작동하는 **현재 Rust 코드**를 통합한다는 뜻이다. 과거 Node 엔진·타 프로젝트 엔진을 가져오지 않는다. 옛 기능 동등성은 사용자 행동·계약 목록으로 비교한다. 각 추출마다 기존 façade가 새 소유자 하나를 호출하도록 바꾸고, 오래된 경로는 side-effect 없는 projection으로만 남긴다. 신·구 두 loop를 shadow라는 이름으로 실제 동시 실행하지 않는다.

## 17. 새로운 단계별 개발 로드맵

이전 S0~S12와 H0~H10 기록은 이력이다. 아래 R 단계는 **현재 9eff603 이후 남은 작업**을 의존 관계로 재구성한 것이다. 이미 존재하는 gateway·lease·wake·policy·backup을 처음부터 다시 만들지 않는다.

```text
R0 기준 고정 → R1 신뢰·원자성 → R2 공통 실행 소유
                              ├→ R3 모델·context·root 예산
                              ├→ R4 도구·협업·채널
                              └→ R5 기억·관측
R0 + R1 → R6 실험 판정 정합성
R2~R6 → R7 전 슬롯 정책 자율개선 → R8 알고리즘·메타개선
각 단계 운영 계측 / R9 전체 복구·배포 인수
```

| 단계 / 목적 | 수정 중심 | 핵심 전달물 | 선행 / 완료 판정 |
| --- | --- | --- | --- |
| R0 보존·기준 확정 | docs/benchmark manifests/contract fixtures | dirty diff 보존, 기능/API/권한 matrix, source/binary hash, 미실행 상태 명시 | 선행 없음. 기존 명령 실행 결과를 새로 기록; 과거 통과 수치 재사용 금지 |
| R1 정확한 권한·저장 | lease/wake/run_state/db/events/api/experiments | C01~03·05~07·13 수정, grant pin/fence, atomic state/outbox/promotion intent | R0. lease loss·restart·stale claim·disk 오류에 외부 중복/권한 확대/거짓 성공 0 |
| R2 공통 실행엔진 | runtime/goals/jobs/workflows/API | Task/Run/Operation 타입, 단일 coordinator, reactive default, durable wait/resume | R1. 일반 대화·룸·Goal·예약 작업이 동일 실행 경계 사용; 기존 도구/repair/replan 기능 보존 |
| R3 모델·자원 | gateway/providers/llm/compact/context | capabilities·native options, role routes, root reservation·usage, context snapshot, latency 분해 | R2. helper·child·fallback 포함 예산, 모델 교체·no-plan·강한 모델 ablation; URL 문자열 능력 추정 의존 제거 |
| R4 도구·협업 | tools/mcp/agents/rooms/channels/dev/video | ToolRegistry/ActionGateway, AgentEnvelope·grant/budget/cancel, 채널 room binding, Browser/Computer port | R2/R3 계약. 새 adapter 추가 시 runtime loop 변경 없음; 두 채널 메시지 중복/권한 누출 없음 |
| R5 기억·경험 | memory/learning/reflect/experience/store/UI | source/claim/episode 구분, sharing grant, conflict/expiry·hybrid adapter, 기억·정책 이벤트 | R2. 개인/공유 격리·정정·망각·재검색 검증; 저장만으로 learned success 표시 안 함 |
| R6 평가·벤치마크 | benchmark/experiments/plan_quality/improve | ITT/cluster·manifest·raw trials·slot metric, missing/timeout/regressed 상태, 통계 라이브러리 검증 | R0/R1. no-plan 사례 timeout 손실이 전체 보고됨; heuristic 상승만으로 승격 불가 |
| R7 자율 정책 개선 | policy/policy_patch/improve/registry/scheduler | 전 슬롯 candidate→실험→canary→적용→rollback, scope CAS·pin·자원 quota | R2~R6. 사용자 재지시 없이 한 폐쇄 루프, 실패 후보는 미승격; 여러 held-out family 효과/비용 기록 |
| R8 알고리즘·메타개선 | AlgorithmRegistry/CandidateGenerator/ExperimentRunner/release | 새 implementation patch 격리 build, optimizer revision, 독립 nested evaluation | R7. 정책 선택과 새 구현 개선 구분, 고정 총예산·outer holdout 비교; 효과 없으면 incumbent 유지 |
| R9 운영 인수 | bootstrap/maintenance/backup/desktop/web/CI | restore drill·장기 soak·daemon·device auth·generated API·Mac/Windows release | 각 단계 결과. UI 종료 후 장기 실행·재접속·철회·복원, 플랫폼별 실제 결과와 NotRun 명시 |

### 17.1 구현 중 기능 유지 기준

기존 채팅 옵션, 파일·이미지 입력, 모델·추론 설정, 그룹 대화, 에이전트 기억, 목표 review/repair/replan, MCP, 외부 채널을 별도 feature matrix로 고정한다. 모든 요청에 계획·reviewer·여러 모델 호출을 강제하지 않는다. 단순 답변은 짧은 경로, 복잡한 작업은 검증·복구가 가능한 경로를 같은 엔진에서 선택한다. 성능 우선순위는 first-token/응답완료/도구완료/후처리 각각의 실측에 따른다.

### 17.2 그대로 전달할 구현 지시

> 이 문서를 기준으로 최신 HEAD와 dirty 작업을 먼저 대조하라. R0→R1→R2에서 현재 Rust 실행엔진을 강화하고, 일반 대화·그룹·도구·장기 작업을 같은 실행 계약으로 통합하라. `/goal`, DAG, bandit, 특정 모델 크기를 하네스 전체로 취급하지 마라. 이미 추가된 gateway·policy·복구·backup을 재작성하지 말고 필요한 계약을 보강하라. 단계별로 수정 파일·호출 경로·회귀 결과·실모델 결과·미검증을 구분해 남겨라. 정책 데이터 변경은 검증 후 자동 반영하고, 새 알고리즘 코드는 격리 build/test/release 경로를 거쳐라. 테스트 통과나 계획 점수 상승을 범용 자기개선 입증으로 표현하지 마라.

## 18. 성능·성공률·자율개선 검증 방법

### 18.1 서로 다른 세 가지 검증

| 층 | 입증하는 것 | 입증하지 않는 것 |
| --- | --- | --- |
| deterministic/mock contract tests | 상태·권한·도구 evidence·취소·저장·복구의 정확성 | 실제 LLM의 계획/추론·일반화 능력 |
| controlled real-model task suite | 특정 모델/환경/작업 분포에서 정책의 효과·비용 | 모든 모델·모든 작업에 대한 일반 성능 보장 |
| production canary/soak | 실제 분포·운영 조건에서 회귀·자원·복구 동작 | 관찰만으로 엄밀한 인과 효과 확정 |

이번 설계 갱신에서는 제품 테스트와 실제 모델 실험을 실행하지 않았다. 정적 분석과 기존 실험 기록을 구분했으며 아래 fixture는 구현 단계의 요구 사항이다.

### 18.2 필수 contract fixture

| Fixture | 주입 조건 | 반드시 관찰할 결과 |
| --- | --- | --- |
| F01 owner provenance | goal/room/delegation의 파생 prompt | 원문 source ID·principal 유지, 파생 내용 owner로 학습 안 함 |
| F02 all tool origins | model/assisted/scheduler/child의 같은 write | 같은 grant·approval·ledger·operation schema 적용 |
| F03 disabled tools | 모델 노출=false, execution=false의 각 조합 | 노출과 권한 구분, 권한 false에서는 모든 origin 거절 |
| F04 procedure oracle | nodes-only 절차·빈 receipt, 두 절차 동일 node ID | 미실행을 followed로 기록 안 함, instance 간 결과 격리 |
| F05 dependency | 병렬 완료 순서 뒤바뀜·누락 ancestor | 실제 happens-before 검사, opSeq 정렬만으로 pass 안 됨 |
| F06 approval crash | 승인 대기 중 재시작/args 변경 | 같은 요청 durable 재개 또는 재승인; stale 승인 적용 안 됨 |
| F07 effect uncertainty | remote write 성공 직후 response drop | 중복 write 0, reconcile 또는 NeedsReview |
| F08 cancel fence | 새 사용자 턴 시작 후 오래된 worker wake | 이전 epoch worker dispatch 0 |
| F09 child budgets | 재귀 위임·동시 fallback | root 한도 초과 예약 불가, 1 worker deadlock 없음 |
| F10 store faults | prepared/finalize/outbox write 실패 | 기록 없는 외부 실행 0, 성공 거짓 표시 0 |
| F11 concurrent finalize | 같은/다른 Run이 같은 정책을 갱신 | 동일 평가 한 번, 서로 다른 관측 모두 반영 |
| F12 stream recovery | 느린 UI·버퍼 초과·재연결 | 메모리 bounded, cursor로 terminal 상태 복구 |
| F13 privacy | private lesson→room/channel/model fallback | 허가 없는 조회/전송/embedding 0 |
| F14 provider variance | tool/schema/vision/usage 미지원 | 명시적 capability/estimate/degraded 결과 |
| F15 evaluation labels | semantic judge+deterministic 혼합 | check별 provenance·Unknown 유지; aggregate 왜곡 없음 |
| F16 policy pin | Run 도중 active revision 변경/철회 | 일반 갱신은 다음 Run, 권한 철회는 즉시 recheck |
| F17 sandbox isolation | 잘못된 server identity/roots/credentials | 실험 시작 거절; production tool invocation 0 |
| F18 migration | legacy global/validated/unknown schema | 보존·quarantine·projection, 가짜 실험 증거 생성 안 함 |

### 18.3 실제 모델 suite와 전이 분할

모델은 4B/9B/27B/70B급 로컬 및 상위 cloud 등 서로 다른 capability profile을 평가하되, 모든 환경에 특정 크기가 있어야 개발할 수 있는 구조로 만들지 않는다. 각 모델군의 이름·version·quantization·context·hardware·runtime을 기록한다. 모델이 없으면 그 행은 NotRun이며 추정으로 통과 표시하지 않는다.

작업 family는 대화·지시 유지, 파일 변환, 다단계 코드 수정, 정보 조사·출처, 문서 품질, API pagination/rate limit, 장기 재개, 멀티에이전트 합성, 의미 기억·충돌, 외부 효과 불확실성으로 나눈다. 코드 수정은 숨겨진 기능 테스트, 문서는 gold factual constraints+blind rubric, API는 mock remote state와 읽기 검증을 사용한다.

같은 template의 파일명만 바꾼 train/test는 약한 전이 시험이다. train은 후보 생성, validation은 후보 선택, held-out test는 final 비교, meta-test는 optimizer 비교에 사용하고 family/template/출처 중복을 확인한다. 생성기는 holdout 정답과 oracle 내부 코드를 읽지 못한다. fixture가 사용하는 tool/environment 구조는 필요한 범위에서 공개하되 답은 감춘다.

### 18.4 분리할 실험

| 실험 | 고정할 것 | 비교 | 주장 가능한 범위 |
| --- | --- | --- | --- |
| E1 실행 보조 | 같은 모델·같은 seeded procedure·같은 계약 | model-guided vs assisted | 보조 실행의 준수/비용 효과; 학습 효과 아님 |
| E2 기억 활용 | 같은 모델·고정 기억 snapshot | memory off/lexical/hybrid | retrieval/context의 효과 |
| E3 경험 기반 개선 | 같은 모델·동일 task family split | frozen baseline vs 경험에서 생성·검증한 policy | 하네스 정책 학습과 held-out 전이 |
| E4 선택 알고리즘 | 같은 후보 집합·같은 총예산 | fixed/greedy vs bandit | selector의 추가 효과 |
| E5 모델 협업 | 총비용 또는 시간 상한을 맞춤 | single model/agent vs routed/multi-agent | 협업 또는 라우팅 이득 |
| E6 메타개선 | 같은 generation+evaluation budget | optimizer O_v vs O_v+1 | 탐색 효율과 outer holdout 개선 |
| E7 새 compiled algorithm | 동일 계약·adapter·dataset | 기존 구현 vs 후보 빌드 | 실제 알고리즘 revision의 효과 및 회귀 |

현행 runner에는 seeded procedure뿐 아니라 planning/config 정책 비교 경로가 있다. 따라서 전부 단순 scaffold로 취급하지 않는다. 다만 seeded 정답·동일 fixture 반복·완료 pair만의 결과는 E3 전이/E6 메타개선 근거가 아니다. 각 실험의 실제 데이터 생성·학습 on/off·후보 생성 경로를 manifest로 구분한다. 실험 fixture의 승인 자동 처리는 exact tool/args/resource hash에 한정하며 production `autoApproveAll`을 켜지 않는다.

### 18.5 Experiment manifest와 결과물

```text
manifest = {
  experiment_id, schema_version, source_head, dirty_source_digest, engine_binary_digest,
  task_suite_revision, split_manifest_digest, task_template_ids,
  model_configs, provider_versions, hardware/runtime,
  baseline_policy_revision, candidate_policy_revision,
  oracle_revision, environment_image_or_fixture_digest,
  workspace_root, data_root, engine_instance_nonce,
  allowed_fixture_tools_and_args, network_allowlist, secret_policy,
  randomization_unit, order_seed, repeats, stopping_rule,
  primary_metric, minimum_effect, noninferiority_margin,
  cost_latency_limits, subgroup_gates, retention_policy
}
```

runner는 child PID/engine nonce, canonical data/workspace, benchmark mode, binary digest를 확인한 뒤 seed/execute한다. 임시 port를 먼저 탐색하고 나중에 bind하는 race를 identity handshake로 감지하며 가능하면 bound listener/port 전달로 제거한다. 잘못된 identity 서버에 어떤 seed/write도 보내지 않는다.

각 trial은 시작 전 artifact/environment reset을 검증하고 run_id, policy/context snapshot, raw receipt, model usage, check results, crash/incomplete 원인을 append-only로 기록한다. API 오류를 elapsed=0 정상 표본처럼 평균내지 않는다. 누락·취소·외부 장애의 처리 규칙을 사전에 정하고 intention-to-treat 결과와 completed-only 보조 결과를 구분한다.

필수 결과물은 `manifest.json`, `trials.jsonl`, artifact/evidence digests, `analysis.json`, `report.md`, promotion/rollback 판단이다. 보고서에는 전체 시도 수, 누락, 모델별/family별 결과, 비용·시간, confidence interval, 효과 없는 결과를 포함한다.

### 18.6 성능·운영 예산

아래 값은 **초기 SLO 제안이며 실측 사실이 아니다**. R0 hardware baseline에 맞춰 확정한다.

- 모델을 호출하지 않는 Task 제출·상태 조회 p95 100ms 이내를 목표로 한다.
- 로컬 permission/registry/checkpoint overhead는 외부 tool latency와 분리해 계측한다. p95 50ms를 초기 조사 기준으로 둔다.
- 취소 요청→신규 dispatch 차단 p95 200ms를 목표로 한다. remote 부수효과 중단 시간과는 구분한다.
- retrieval·context 준비는 모델 호출 전 별도 budget을 두고 초과 시 허용된 lexical/degraded path로 간다.
- event delta는 bounded bytes/messages, artifact는 blob 참조, DB query는 task/run/partition index로 제한한다.
- scheduler는 interactive/background/improvement별 quota와 aging을 갖고 reflection이 foreground 자원을 독점하지 못하게 한다.
- soak는 장기 Task·반복 restart·느린 consumer·대규모 memory/operation index에서 queue depth, DB lock time, RSS, GPU occupancy, dropped delta, pending outbox age를 측정한다.

개선의 순이득에는 후보 생성·실험·embedding·추가 검토 비용도 포함한다. 운영 Task에서 5% 빨라졌지만 탐색 비용 회수에 과도한 기간이 필요하면 이를 별도로 표시한다.

### 18.7 timeout·독립 표본·승격 판정의 정확성

Primary operational success는 할당된 모든 trial의 deadline 내 목적 달성 여부다. treatment가 느려 timeout이면 해당 지표에서 실패다. 사용자 철회·외부 인프라 장애 등은 별도 이유로 기록하고, 사전 제외 규칙·전체 ITT·민감도 분석을 함께 낸다. 실행이 완료됐지만 의미 판정이 Unknown이면 Unknown rate를 별도로 공개하고 자동 성공으로 세지 않는다.

paired success의 `d_i∈[-1,1]`에 독립성을 가정한 Hoeffding 예시는 `h=sqrt(2*ln(2/α)/n)`, `CI=[max(-1, mean(d)-h), min(1, mean(d)+h)]`다. **h 자체를 먼저 1로 자르면 안 된다.** 실제 같은 template 반복은 cluster 분석을 우선하고, 독립 cluster가 너무 적으면 폭넓은 구간과 Inconclusive를 낸다. 선택 후 평가·반복 중간 관찰에는 이 단순 고정 표본식을 그대로 적용하지 않는다.

결과는 Improved / NoninferiorWithSavings / Inconclusive / Regressed / InvalidExperiment로 구분한다. timeout 두 개를 버린 usable 8과 실제 할당 10을 별도 필드로 유지한다. planning heuristic은 diagnostic이며 recovery/retrieval/비용 절감 정책의 공통 필수 gate가 아니다.

승격 요청은 `manifestDigest, trialArtifactDigest, analysisRevision, candidateRevision, expectedActiveRevision, claimToken, leaseEpoch`를 요구한다. 서버는 immutable trial에 근거해 재계산하거나 신뢰 등록된 runner의 artifact를 검증한다. 결과 요약을 보낸 클라이언트가 단독으로 units/delta/verdict를 정할 수 없다. 정책 활성 pointer·claim 소비·deployment event는 같은 transaction 또는 멱등 promotion-intent 처리로 일치해야 한다.

## 19. 기술적 위험·한계·대안

### 19.1 설계 위험과 대응

| 위험 | 관찰할 신호 | 기본 대응 / 대안 |
| --- | --- | --- |
| 작은 모델의 제한 | invalid schema, 목적 이탈, repair 반복 | capability 기반 보조·대체 모델·명시적 미완료; DAG로 모든 사고 대체 안 함 |
| 고성능 모델 과잉 제약 | planner/reviewer 비용 증가, baseline보다 낮은 성공 | 보조 ablation과 단독 모델 경로; optional plan |
| reward hacking | verifier 통과지만 사용자 목적 미달 | 독립 rubric/gold task, artifact revision 고정, critical checks |
| judge 편향 | 같은 모델 자기평가 과대, rubric 불안정 | blind judge, 다른 모델/인간 calibration, Unknown 허용 |
| 비정상 온라인 탐색 | production 손상, 낮은 성공률 | sandbox-first·allowlisted canary·rollback; 위험 작업 exploration 금지 |
| 인과 귀속 실패 | 여러 정책·모델 동시 변경 | bundle treatment 또는 분리 ablation, legacy observation 분리 |
| 메모리 오염/누출 | 가설이 사실로 재인용, scope cross | typed provenance·SharingGrant·삭제 lineage·근거 없는 사실 거부 |
| 분포 변화 | task/model/tool version별 회귀 | stratum metrics·expiry·revalidation·baseline fallback |
| restart/외부 불확실성 | dispatch receipt 누락 | reconcile/idempotency/NeedsReview; 외부 exactly-once 주장 금지 |
| 비용 폭증 | 재귀 agent·fallback·meta 실험 증가 | root reservations·global admission·search quota |
| SQLite 한계 | `all()` scan·writer 대기·큰 JSON | typed indexes·writer queue·blob 분리; 필요할 때 서버 DB adapter 검토 |
| sandbox 한계 | shell이나 plugin이 허가 영역 밖 접근 | OS/container isolation과 capability 제한; path check만으로 process sandbox 주장 금지 |
| plugin/protocol drift | tool schema나 auth 방식 변경 | pinned revision·conformance fixture·명시적 incompatibility |
| 장기 작업 정책 노후 | Run pin이 오래된 tool/provider와 불일치 | checkpoint boundary revalidation·새 Run migration |
| 설계 복잡성 | domain과 facade 중복·migration 장기화 | phase별 단일 ownership, 첫 구현은 최소 slot/interface만 |

예산, 정보, 모델 능력이 제한된 현실에서 모든 작업을 자동 완료할 수는 없다. 이 설계가 보장하려는 것은 일관된 실행과 측정 가능한 개선 경로다. 실제 성공률과 메타개선 효과는 R6~R8 실험으로 결정된다.

### 19.2 공개 시스템과 연구의 참고 범위

다음 공식 문서와 원문은 이번 갱신에서 확인한 참고다. 적용·한계 판단은 **DOMINION을 위한 독립 설계 제안**이며 외부 엔진·소스코드를 가져오지 않는다.

| 참고 | 공개 구조에서 참고한 점 | 자체 설계에 반영할 범위 |
| --- | --- | --- |
| [LangGraph Persistence](https://docs.langchain.com/oss/python/langgraph/persistence) | checkpoint/thread 및 영속 상태 | Run checkpoint와 Memory 분리; 모든 작업을 graph로 강제하지 않음 |
| [Hermes Persistent Memory](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory) | bounded curated memory와 사용자 정보 | 출처·scope·context snapshot; 저장 자체를 성능 개선으로 보지 않음 |
| [OpenClaw Agent Loop](https://docs.openclaw.ai/concepts/agent-loop) | 세션별 context/model/tool/stream/persistence loop | 채널별 복제 엔진 대신 동일 실행 경로와 세션 직렬화 |
| [MCP lifecycle 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle) | initialize·버전·capability 합의 | 서버와 protocol 협상, 하네스 권한은 별도. 특정 버전을 영구 최신으로 가정하지 않음 |
| [ReAct](https://arxiv.org/abs/2210.03629) | reasoning/action/observation 교차 | 환경 결과에 따른 재판단. 비공개 chain-of-thought 노출 강제 없음 |
| [GEPA](https://arxiv.org/abs/2507.19457) | trace 반성·prompt 후보 진화 | 후보 생성기 비교 대상으로 사용; 논문 효과를 DOMINION 성과로 주장하지 않음 |

§10의 Reflexion·Doubly Robust 논문 링크는 기법의 추가 원문 참조다. 전체 제품의 비공개 구현이나 조사하지 않은 시스템은 추정해서 설계 사실로 쓰지 않는다. RunCoordinator·grant 상속·정책/알고리즘 registry·meta gate의 결합은 이 저장소를 위한 제안이다.

## 20. 최종 아키텍처 검토와 구현 인수 조건

### 20.1 요청한 12개 기술 질문의 대응

| 질문 | 목표 대응 | 현재 상태 | 남은 작업 / 증거 |
| --- | --- | --- | --- |
| 1. 4B부터 상위 cloud? | capability ModelGateway | 공통 gateway·local/cloud 연결 | R3 모델별 지원/실측 matrix, 없는 모델 NotRun |
| 2. 능력 제한 방지? | optional assistance·native options·동적 budget | 고정 한도·heuristic routing 잔존 | R3 no-plan/strong-model ablation·native 기능 보존 |
| 3. 새 작업 수행? | reactive planning·discovery·recovery | 일반 tool loop·goal 계획 있음 | R2/R4 unseen task와 실패 복구 trace |
| 4. DAG 없이 실행? | reactive default | 이미 일반 채팅 가능 | R2 저장 절차 없는 multi-step 회귀 |
| 5. 오판 수정? | tool evidence→evaluation→repair | acceptance/repair/절차 평가 있음 | R1/R2 false-success·stale effect 방지 |
| 6. 장기 자율 수행? | durable waits·fenced lease·scheduler | jobs/lease/wake 연결, 권한/fence 결함 | R1/R2 재시작·timer·UI 종료·승인 재개 |
| 7. 확장? | registry+adapters+ActionGateway | builtins/MCP/서비스 있음 | R4 core loop 변경 없는 새 tool/channel |
| 8. 협업? | Supervisor·root scope/budget | rooms/delegate/role model 있음 | R3/R4 linked cancel·다중 모델·합성 검증 |
| 9. 기억 vs 개선? | Memory/Episode/Hypothesis/Evaluation 분리 | 기록·추출·실험 경로 존재, provenance 혼재 | R5/R6 기억 저장과 성능 증거 분리 |
| 10. 내부 알고리즘 개선? | revision registry·generator·release·meta | planning/recovery 파라미터 후보/소비 연결 | R7 전 슬롯, R8 새 implementation·optimizer 비교 |
| 11. 실제 비교? | frozen manifest·ITT·holdout | 실제 paired 기록 있으나 censoring/metric 결함 | R6 통계/승격 수정 후 R7/R8 전이 비교 |
| 12. 기존 코드 활용? | façade 유지·단일 소유로 추출 | Rust 자산 다수와 신 기능 존재 | R0~R4 기능 parity·원본 보존·중복 엔진 없음 |

### 20.2 마스터 설계의 완료 조건

이 문서는 범용 하네스 정체성, 모델의 동적 능력 지원, 실행·기억·도구·알고리즘·개선의 통합, 비-DAG 기본 실행, 현재 코드와 목표 계약의 대조, 알고리즘 슬롯, 알고리즘 자체의 revision·메타개선, 파일별 이관, 실험 기반 인수 기준을 한 문서로 정의한다. 이는 **설계 산출물의 완료**이며 구현·실모델 개선의 완료가 아니다.

### 20.3 구현 인수 게이트

1. **실행 기반:** 모든 ingress와 모든 tool origin이 같은 권한·budget·operation lifecycle을 사용하고 R0 핵심 동작 회귀가 없다.
2. **내구성:** pending 승인·child wait·external uncertainty·restart crash matrix에서 허위 성공과 중복 외부 효과가 없다.
3. **범용성:** 저장 절차가 없는 여러 새 task family를 수행하고 필요한 계획/모델/도구를 동적으로 고른다. 실패도 typed 근거로 남긴다.
4. **모델 독립성:** 특정 파라미터 수·provider 이름 문자열이 core 실행 정책의 필수 전제가 아니다. 고유 capability와 명시 fallback을 시험한다.
5. **협업 경계:** 자식 권한·budget·memory scope가 부모를 넘지 않고 결과를 독립 검증한다. 고정 agent roster가 필요 없다.
6. **기억 품질:** 원문/가설/경험/정책을 구분하고 교차 partition 누출과 stale overwrite를 검증한다.
7. **자율개선 경로:** 실행→후보 생성→격리 실험→비교→canary→적용→재평가가 사용자 반복 지시 없이 허용 범위에서 연결된다.
8. **개선의 증거:** 동일 모델·정의된 분포·held-out 조건에서 효과와 비용을 보고한다. 통계적으로 불확실하면 Inconclusive이며 “성장 입증”으로 출하하지 않는다.
9. **알고리즘 발전:** prompt뿐 아니라 retrieval/planning/recovery/selector 등 한 개 이상의 policy implementation revision을 비교할 수 있고 optimizer revision도 독립 평가 경로가 있다.
10. **운영:** source/binary/policy/oracle/data revision으로 결과를 추적하고 rollback·backup restore·계정/기기 철회를 검증한다.

### 20.4 최상위 설계 결정 기록

| ADR | 결정 | 이유 / 재검토 조건 |
| --- | --- | --- |
| A01 | 기존 Rust 실행엔진을 단계적으로 통합 | 작동 경로와 회귀 자산 보존. 별도 엔진 복제 금지 |
| A02 | Task/Run/Operation/Evaluation이 중심 | 대화·장기·협업·개선에 공통 identity와 lifecycle 필요 |
| A03 | reactive 기본, DAG/절차는 선택 | 낯선 작업과 정적 반복 작업을 함께 지원 |
| A04 | native provider 기능 + 표준 envelope | 최저 공통 기능으로 모델 능력을 제한하지 않음 |
| A05 | 모든 부수효과는 gateway, 외부 exactly-once 미보장 | 권한·관찰·복구 일관성, 분산 효과의 현실적 한계 |
| A06 | SQLite 로컬 우선, typed 핵심+legacy projection | 운영 단순성과 migration 안전성. 부하 근거가 생기면 DB 확장 |
| A07 | 후보 생성·선택·평가·배포는 별도 책임 | bandit만으로 새로운 알고리즘 개선을 설명할 수 없음 |
| A08 | pinned immutable policy, next-run activation | 인과 추적·rollback·재현성 |
| A09 | held-out와 meta-evaluation gate 독립 | 자기평가·시험지 오염·자기 승인 방지 |
| A10 | root grant/budget 상속, linked child 기본 | 위임으로 권한·비용·수명 경계가 사라지지 않음 |

---

## 부록 A. 소스 근거와 재확인 방법

§3·§4·§16의 파일/함수는 이 작업 트리 기준이다. 구현 시작 시 `git rev-parse HEAD`, `git status --short`, `rg`로 다시 찾는다. 줄 번호는 후속 수정에 쉽게 틀리므로 고정하지 않는다.

핵심 근거: `runtime::{run_turn_with_policy,run_turn_body,dispatch_tool_call,record_selection_snapshot,finalize_execution}`, `Store::{create_attempt,finalize_once}`, `lease::LeaseGuard`, `run_state::apply`, `wake::recover`, `gateway`, `policy::{propose_candidates,apply_experiment_result}`, `policy_patch`, `plan_quality`, `api/experiments`, benchmark의 `pair_effect`. 외부 채널·영상·계정은 경로/소유권 정적 검토이며 실제 계정 동작 검증과 구분한다.

분석 스냅샷은 문서 머리의 source diff hash로 식별한다. 기존 `tmp/` raw 실험 artifact 전체를 검증하지 않았으므로 H7 기록의 수치는 해당 문서 출처를 명시했다. 소스가 바뀌면 C01~C14의 재현 fixture로 해결 여부를 확인하고 과거 결함을 무조건 현재 사실로 복사하지 않는다.

## 부록 B. 문서 검증 범위와 인계 조건

이 작업의 변경 대상은 이 마스터 Markdown 한 파일이다. 제품 코드 수정·실행·배포·커밋·푸시는 수행하지 않았다. 코드 테스트나 실모델 성능 향상을 새로 확인했다고 주장하지 않는다. 문서의 20개 필수 장·코드 fence·이전 HEAD/테스트 주장 잔존·소스 변경 여부를 확인한다.

설계 인계물은 **현재 코드 사실 / 정적 결함 / 목표 계약 / 구현 순서 / 검증 기준**을 한 파일에 담는다. 문서 예시와 제안 SLO는 구현 완료 상태가 아니다. 구현자는 R 단계별 증거를 남기고 기능 보존·권한·성능·실제 개선을 각각 평가해야 한다.
