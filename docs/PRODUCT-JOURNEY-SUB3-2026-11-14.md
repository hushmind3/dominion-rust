# 서브3 · Issue #9 전 제품 연결 개선 (2026-11-14)

## 사용자 결과 중심의 새 경로

**실제 실행 경로**: `Agents.tsx`의 에이전트 프로필에서 **조사→검토 워크플로 만들기** → `POST /api/agents/:id/workflow` → SQLite workflow 정의(조사 에이전트 + 별도 검토 에이전트) → `Workflows.tsx`의 실행 → `POST /api/workflows/:id/run` → `jobs::run_job` → `workflows::run` → `run_quiet_outcome`에서 실제 모델/도구 요청과 `execution_operation` 영속화 → 단계별 출력·작업 영수증·모델 사용량 → 최종 산출물 SHA-256 + provenance와 함께 `Store::put_if_absent("workflow_artifact")`로 보존.

완성된 산출물은 `GET /api/workflow-artifacts`와 `GET /api/workflow-artifacts/:id`에서 조회할 수 있다. `Workflows.tsx`에서 원문 확인·Markdown 다운로드·다른 워크플로 입력 선택이 가능하다. 실행 등록 시 `workflows/artifacts.rs::resolve_run_input`가 이전 원문의 SHA와 길이를 검증한 뒤 `sourceArtifact`(ID·digest·원본 job/flow)를 기록한다. 이전 결과 내용은 별도 **참고 자료**로 감싸 모델에 전달한다. 변조/과대 입력은 생성 전에 400으로 거부한다.

- **실제 납품:** 이전의 일회성 채팅 메시지 대신 작업/세션/에이전트/모델/출처 메시지에 연결된 불변 결과물. 같은 job ID로 내용이 다른 결과를 덮어쓰려 하면 거부한다.
- **검토 단계:** 사용자 요청 `{{input}}`과 앞 단계의 실제 출력 `{{previous}}`를 별도 검토 에이전트에 보내 조사·검토·납품으로 구성한다. 동일 에이전트를 지정한 경우에도 2회 실행되며 독립 모델이라고 주장하지 않는다.
- **행동 영수증:** 개별 워크플로 단계에서 `requireActionEvidence`를 선택할 수 있다. 해당 단계가 실제 성공 도구 행동을 보고하지 않으면 **needs_review**로 종료하고 그 단계를 완료로 기록하지 않는다. 모델 텍스트만으로 외부 작업 완료를 인정하지 않는다.
- **예산:** 워크플로 실행 입력 `tokenBudget`는 최대 200만 토큰이며, 실제 세션 `model.usage` 합계를 다음 모델 호출 전에 검사한다. 이미 시작한 호출의 사후 과금 초과까지 보장하는 선결제 제한은 아니다. 원화 청구 자료와 추정 사용량은 별개다.
- **장애:** 완료 단계 outputs, 동결된 definition, inFlight 상태를 job 체크포인트에 남긴다. 미확정 도구 실행 단계는 자동 재실행하지 않고 needs_review로 멈춘다. 원문 확인 없이 미완결 결과를 성공/납품으로 둔갑시키지 않는다.

## 정체됐던 자동화의 실제 워크플로 실행

`Operations.tsx`에서 자동화 유형으로 **협업 워크플로**를 선택하고 flow/입력/토큰 상한/간격을 지정할 수 있다. `collections.rs`가 등록된 workflow·입력 길이·artifact SHA·토큰 상한을 검사한다. `jobs::schedule_automations`는 예약 회차의 `automation.id + nextAt`를 SHA-256에 넣어 **동일 회차 작업 ID**를 만들고 `jobs::enqueue_once`로 중복 삽입을 막는다. 예약 회차를 다시 스캔해도 새 유료 모델 job을 만들지 않는 단위 테스트를 포함한다. 다른 회차에서는 다른 ID로 실행되고, 각 회차별 산출물이 독립 보관된다.

이 구조는 2026년 Temporal/AWS의 영속 실행·작업 ID 기반 중복 방지 설계와 목표가 유사하지만, 별도 Temporal 서버 없이 자체 Rust/SQLite에 연결했다. **예약 실행 한 번의 동일 job 중복 등록 차단**이며 모든 외부 도구 부작용의 exactly-once 처리를 보장한다는 주장은 하지 않는다.

참조: [Temporal Durable AI](https://docs.temporal.io/ai) · [Temporal AI schedule](https://github.com/temporal-community/temporal-dotnet-agents/blob/main/docs/how-to/MAF/scheduling.md) · [MCP workflow 오케스트레이션](https://docs.conductor-oss.org/devguide/ai/mcp-guide.html).

## 15개 메뉴 상태와 이번 실제 제품 변경

| 메뉴 | 실제 변화 | 새 실행/보존 경로 | 검증 수준 / 남은 문제 |
|---|---|---|---|
| 홈 | 신규 최근 결과물 섹션 | `Home.tsx → /api/workflow-artifacts → Workflows` | API 데이터 연결; 첫설정 마법사는 기존 유지 |
| 채팅 | 기존 공통 채팅과 세션 재사용 | workflow도 `run_quiet_outcome`와 session 사용 | 실모델 장기 완수율 미측정 |
| 그룹대화 | 이번 패치 직접 수정 없음 | 기존 rooms 엔진 유지 | 공동 산출물의 자동 결합 부족 |
| 모델 | 이번 패치 직접 수정 없음 | workflow 단계 agent/provider/model 설정 재사용 | 실제 모델별 품질/가격 자동 최적화 미구현 |
| 에이전트 | **새 조사·검토 협업 생성 액션** | `Agents → /api/agents/:id/workflow → workflow` | 등록 에이전트 존재 검증, 모델 실사용 별도 |
| 영상 | 기존 LTX/Omni/Comfy/네이티브 유지 | 영상 job·SHA 영수증 경로 불변 | 실영상 시청 품질·청구액 미측정 |
| 연결·MCP | 기존 통합 메뉴 보존 | workflow 실제 대화 턴이 기존 도구 실행 정책과 MCP 도구를 재사용 | 설치 스킬 권한·품질 실증 미완 |
| 기억 | 기존 범위/출처/편집 유지 | workflow session이 기존 의미학습 경로에 연결됨 | 기억 향상 수치 미측정 |
| 목표 | 이전 최신 소스 검증/재개 개선 유지 | `goals.rs → verification.rs` | 현재 테스트 컴파일 참조 복구는 보수 유지보수 |
| 자동화 | **새 반복 workflow 지원** | `Operations → collections → schedule_automations → enqueue_once` | 동일 슬롯 중복 등록 테스트; 실제 예약 운영 표본 필요 |
| 활동 | **새 영속 결과·실행 출처·토큰 영수증 확인** | `Operations.tsx → /api/workflow-artifacts → jobId/SHA/modelUsage` | 실제 데이터 조회; 원화 실청구 비교는 미구현 |
| RSI | 기존 독립 평가·후손 구조 유지 | 모델 결과물의 출처 계측 가능 | 모델 주도 RSI 실증 없음 |
| 설정 | 기존 정책/백업/관리 보존 | 워크플로 도구 호출의 승인 정책 재사용 | 백업 UI 직접 고도화 없음 |
| 워크플로 | **원문 납품·재사용·증거 옵션·예산 게이트** | `workflows.rs → artifacts.rs → SQLite → UI` | 실모델 결과 품질/총원가 추가 측정 필요 |
| 개발자 | 기존 테스트·Git·미리보기 유지 | 별도 산출물과 결합 가능하나 직접 UI 연동은 미구현 | 개발자 도구 화면의 신규 기능 미반영 |

## 검증 경계

- Rust 단위 테스트: 결과 영속성/충돌 거부·SHA 재사용, 변조/과대 입력 차단, 에이전트 2역할 workflow 등록, 예약 시각별 중복 실행 방지.
- 프론트 정적 연결 회귀: `scripts/sub3-connection-hub.test.mjs`에서 Agents→API→workflows→artifacts→automations→Home UI/엔진 경로 검증.
- GitHub Actions: 새 HEAD 기준 Ubuntu/macOS 전체 Rust 테스트, Windows 테스트 바이너리 컴파일, 3OS 프론트 타입/빌드. **커밋별 CI 결론을 별도로 확인한다.**
- 유료 실모델의 종단 간 실행, 사용자 시청 영상 품질, 독립 RSI 개선, 원화 실청구 절감률은 이번 코드만으로 실증한 것이 아니다. 저사양 로컬/클라우드 모델 선택 정책은 유지한다.

**실행 정의 동결과 단계 ID**: 직접 시작하거나 자동화로 예약한 작업의 `input.definition`을 등록 시점에 저장한다. 워크플로 정의가 이후 수정·삭제돼도 기존 job은 동결된 단계를 사용한다. UI에서 저장하는 단계에 `uuid` ID를 부여하고 중복을 거부해 checkpoints/영수증의 출처가 흐려지지 않게 한다. `workflow.cancelled`와 `workflow.completed` 감사 이벤트는 실제 최종 상태로 분리한다.

### 저장·조회 비용 및 실제 실행 증거 강화

- `Store::session_execution_operations(sessionId)`는 워크플로 단계 직전과 직후의 영속 `execution_operation`을 DB에서 읽어 추가된 작업 ID만 추린다. `status=completed` 및 `verification::is_action_evidence` 조건을 통과한 도구 작업만 단계의 성공으로 집계하고 `toolOperationIds`를 최종 아티팩트 `stepReceipts`에 보존한다. 화면의 원문 보기에도 해당 작업 ID가 표시된다.
- `GET /api/workflow-artifacts`는 최대 100건의 ID·180자 미리보기·SHA·원본 작업·토큰 메타데이터만 전송하고 대형 본문은 보내지 않는다. `GET /api/workflow-artifacts/:id`를 실제 원문 보기·Markdown 다운로드 시 요청한다. 브라우저의 목록 로드에서 100 × 256KiB의 본문 재전송을 피한다.
- 검증 범위: SQLite 영속 작업 조회 테스트, 위 API/UI 경로에 대한 Node 정적 회귀. **실제 모델 도구 실행을 포함한 장기 E2E 측정, 명시적 유료 과금 증빙, 동시 다중 사용자 벤치마크는 별도 운영 검증 대상**이다.
