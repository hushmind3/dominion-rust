# 서브1 종합 제품 고도화 — 2026-11-14 · Issue #9

## 범위와 실행 증거

작업 브랜치: `dominion-sub1` 단독. 개발 목적은 기존 15개 메뉴의 *기능 이름*이 아니라 모델 → 에이전트(설치 스킬·기억) → 워크플로(실제 모델/도구) → 결과 검수 → 영속 산출물 및 작업 기록의 사용자 흐름을 만드는 것이다. 기존 그룹대화·Goal·RSI·영상·백업·개발자 도구를 제거하거나 메인에 강제 병합하지 않는다.

### 실제 호출 흐름 A: 모델 연결부터 2개 에이전트 결과물까지

1. **모델** `web/src/pages/Models.tsx`: 연결한 Provider/Model을 고르고 선택한 Agent에 적용 → `POST /api/agents/bulk-model` → `Store::patch_records`. 재실행 중인 기존 작업의 모델이 바뀌지 않도록 *워크플로 등록 당시 모델 스냅샷*은 불변으로 둔다. 신규/미등록 제공자 또는 선택 모델 누락은 API에서 거부한다.
2. **연결/스킬** `web/src/pages/Agents.tsx` → `GET /api/skills` → `extensions::installed_skills` (마켓 검색 결과가 아닌 실제 DB `skill` 레코드). 사용자가 설치·활성화한 최대 8개 스킬을 `POST /api/agents`의 `preferredSkillIds`로 저장. `agents::save_agent`는 미설치/비활성 ID를 거부한다. `chat/context.rs::context_block_with_manifest_at`는 선택한 스킬을 실제 Chat/Room/Workflow 모델 컨텍스트의 우선 5개로 정렬하고 출처·개정 지문을 남긴다. MCP 호출·외부 작업 승인 정책은 별개로 강제한다.
3. **에이전트** 카드의 `#/workflows?agent=<uuid-or-id>`를 누르면 **워크플로** 작성기에서 실행자·독립 검토자 2단계를 미리 구성한다. 자동 유료 API 호출은 없다. 다른 에이전트가 있으면 독립 검토자로 선택하며, 없을 때는 동일 에이전트를 재사용한다고 표시된다.
4. **워크플로** `POST /api/workflows` → `api/workflows.rs::normalize`: 단계 ID, 실존 에이전트, 모델 재지정, 24단계 상한, 파일 변경 검증 조건을 검사 후 저장. `POST /api/workflows/:id/run` → `runnable`는 활성 Provider/모델을 선확인하고 현재 Agent/Model을 동결된 `job.input.definition`으로 저장. 클라이언트 `requestId` UUID 및 `jobs::enqueue_once`로 중복 클릭/응답 유실 재요청이 동일 작업 ID를 가리킨다. 동일 UUID에 다른 입력은 HTTP 409로 거부한다.
5. **실제 실행** `engine/jobs.rs::run_job` → `engine/workflows.rs::run` → `run_quiet_outcome` → provider + 실제 도구 실행 → `execution_operation` 영속 receipt. 각 단계의 `agentId`, `providerId`, `model`, `messageId`, `committedToolReceiptIds`, `toolErrorCount`, `usageAfterStep`를 **job.workflowState.outputs**에 checkpoint한다. 모델 설명문은 도구 영수증으로 인정하지 않는다.
6. **검증·복구** 단계가 `verifyPath`를 선택한 경우 `verification::baseline_checks`가 실행 전 파일 fingerprint를 저장하고 `check_acceptance`로 실제 변경 여부를 검증한다. 미충족 시 **후속 에이전트 실행을 막고** `needs_review` 및 `workflowState.lastAttempt.receiptIds`, `acceptanceGaps`를 남긴다. 중단된 `inFlight`는 자동으로 외부 부작용을 재호출하지 않는다. 마지막 단계가 정상 종료한 경우만 `workflow_deliverable`을 작업 ID로 upsert해 중복 생성 없이 납품한다.
7. **산출물** `GET /api/workflows/deliverables/:id` → `web/src/pages/workflows/DeliverablePanel.tsx`: 최종 텍스트, 각 작업자 모델, 단계별 실제 DB 도구 영수증 ID, 세션 링크, 사용량을 확인·복사한다. **독립적으로 사람이 평가한 결과물은 아님** (`independentlyVerified:false`). 오류·보류 시 마지막 성공 checkpoint와 작업 상태를 유지한다.

### 실제 호출 흐름 B: 개발자가 변경한 Git diff를 검토 결과물로 납품

`web/src/pages/Developer.tsx` → `GET /api/dev/diff` → 실제 저장소 변경 패치를 확인한 뒤 사용자가 워크플로를 선택하고 *검토 실행* → `POST /api/workflows/:id/run`으로 diff 입력 전달 → 검토 에이전트 실행·영수증·산출물 영속 보관. 무단 코드 수정이나 자동 커밋은 수행하지 않는다. Git 변경은 사용자가 별도 선택할 때만 모델에 전달된다. 기존 개발자 테스트·빌드·프리뷰·커밋 기능과 독립 유지한다.

## 15개 메뉴별 이번 발전과 검증 범위

| 메뉴 | 사용자 목표 / 기존 능력 | 이번 서브1 개선 · 실제 경로 | 증거 · 비용 / 남은 과제 |
|---|---|---|---|
| 홈 | 작업 시작·현재 상태·이력 | `Home.tsx` → 현재 Provider/Agent/Workflow/Skill/Job 응답으로 다음 연결 단계를 계산, 마지막 review/납품 상태 표시 | 정적/프론트 검증. 세션별 실결제 아직 미집계 |
| 채팅 | 단일 에이전트·기억·도구 활용 | `agents.preferredSkillIds` → 공통 `chat/context.rs` 컨텍스트 우선순위; 기존 대화 모델 호출에 적용 | 스킬 출처 지문. 실모델 품질 개선률 미측정 |
| 그룹대화 | 다중 모델 협력·공유 문맥 | 그룹이 쓰는 공통 chat 컨텍스트에 Agent 지정 Skill 적용, 정체성 유지 | 모델 간 공동 파일 산출물의 실측 성공률 부족 |
| 모델 | Provider/OAuth/CLI/로컬 실행 | `Models.tsx`에서 현재 모델을 특정 에이전트에 실제 API로 배정 | 모델별 지연·실제 원가 기반 자동 라우팅 부족 |
| 에이전트 | 역할·모델·프로필 | `Agents.tsx` 스킬 장착, 실행 능력과 작업 이력·워크플로 직접 연결 | 역할별 모델 성능/역량 자동 추천 부족 |
| 영상 | 유료 모델·장면·검수·재생성 | 직전 작업의 LTX 견적 승인·실행 전 재검증·FFmpeg 실디코드 경고 유지 | 인물/음성 의미적 연속성 및 실제 인보이스 미측정 |
| 연결 | MCP·스킬·마켓 | `GET /api/skills`의 실제 설치 레코드 → 에이전트 선택 → 실행 컨텍스트 적용 | MCP 서버 실패 복구·도구별 자동 라우팅 개선 필요 |
| 기억 | 추출·편집·검색·성장 | 기존 scope 기억과 병렬로 Agent Skill 컨텍스트 우선순위 통합 | 오래된 기억 자동 병합·실측 회상 향상 부족 |
| Goal | 검증·복구·체크포인트 | 직전 Rust 영수증/최신 테스트 검증·목표/작업 단일 트랜잭션 유지 | 장기 실모델 완주율 부족. 메뉴 수 가산 0 |
| 자동화 | 반복 작업·중복 방지 | 기존 기능 유지. 새 재사용 워크플로 엔진을 직접 실행하도록 안정화 | 자동화에서 워크플로 선택·결과 전달까지 추가 연결 필요 |
| 활동 | 도구/실행/감사 기록 | Workflow Job에 단계별 영속 도구 작업 ID·실측 토큰·검증 미달 정보 추가 | 전체 활동 대시보드의 1클릭 provenance 조회 확장 필요 |
| RSI | 후보·동결 평가·승계 | 기존 모델/비가중치 RSI 유지. 워크플로 검토 산출물을 RSI 성공 실적으로 **계상하지 않음** | 실제 모델 자기개선 연속 채택 미입증 |
| 설정 | 권한·운영·백업 | 기존 `/api/policy`를 스킬·워크플로 외부 도구에 그대로 적용 | 종합 복원 실측·진단 동선 부족 |
| 워크플로 | 다단계 실행·자동 복구 | **누락된 CRUD·Run·History·Deliverable 라우트 복구+확장**, 실행자·모델 동결, 입력 멱등, 독립 파일 변경 체크, 산출물 저장·열람 | 유료 모델 청구액·대형 작업 완수율 미측정 |
| 개발자 | Git·diff·빌드·프리뷰 | 실제 `/api/dev/diff`를 사용자 승인하에 워크플로 입력으로 전송·검토 보고서 납품 | 자동 PR/테스트 결과 무결성 리뷰는 다음 단계 |

## 검증과 한계

- Rust API 회귀: `api/workflows.rs`에 실존 에이전트·Provider 확인/동결, 동일 요청 멱등성/변경 거부, 미활성 모델 차단 테스트.
- 통합 회귀: `engine/workflows.rs`에 실제 2개 에이전트 턴을 HTTP mock provider에 실행해 메시지·세션·최종 `workflow_deliverable` 영속화 테스트. **실제 유료 모델 완주 테스트와 구별**.
- 계약 회귀: `scripts/verify-frontend-navigation.mjs`에서 서버 라우트, 사용 화면, 실행 엔진/모델 컨텍스트/산출물 경로 확인.
- 기존 2026-11-14 실모델 Qwen3 4B 2개 과제 워크플로 [실행 기록](https://github.com/hushmind3/dominion-rust/actions/runs/43583057056): **오라클 0/2 성공, 2개 모두 제한시간 초과 취소**, 엔진 자체 설치와 측정 파이프라인만 성공. 이를 실모델 완주 성과로 보정하지 않는다. 새 다중 에이전트 워크플로 통합의 실모델 완수율은 별도 검증 전까지 미측정.
- 2026-07-28 [공식 MCP Client Best Practices](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/docs/2026-07-28/develop/clients/client-best-practices.mdx): 수십~수천 개 도구를 첫 컨텍스트에 모두 넣는 방식의 비효율, 점진 발견 및 안정적 캐시 가능한 정의 집합 권고. 이번 작업은 설치된 스킬을 에이전트별로 우선 지정하고 최대 5개만 컨텍스트에 보내지만, 전체 MCP 도구 카탈로그의 점진 발견/캐시 개선까지 완료했다고 주장하지 않는다.
- 공개 [Microsoft Conductor](https://github.com/microsoft/conductor): 모델 역할별 워크플로를 버전 보존하며 분리된 재사용 단위로 실행하는 패턴 참고. 이 구현은 DOMINION 기존 Rust 작업 엔진을 사용하며 해당 프로젝트를 종속성으로 들이지 않았다.

## 다음 개선 진입점

기능·사용자 결과물·비용 우선: `src/api/workflows.rs` → `src/engine/workflows.rs` → `web/src/pages/Workflows.tsx` → `web/src/pages/workflows/DeliverablePanel.tsx`. 향후 1) 고정된 평가 데이터에서 실제 워크플로 독립 성공률 2) 원화 인보이스와 토큰 사용량 및 작업 완수율 연계 3) 자동화·MCP 실제 실행 결과와 워크플로 결과 통합 4) 영상 연속성 인력/비전 검사 등이 필요한 상태다.
