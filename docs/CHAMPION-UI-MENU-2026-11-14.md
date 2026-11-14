# 챔피온 2026-11-14 UI 전체 메뉴 및 실제 연결 점검

범위: dominion-champion HEAD의 `web/src/components/WorkspaceShell.tsx`, `web/src/App.tsx`, 화면 코드, `src/api/mod.rs` Rust 라우터와 엔진 진입점. 기존 메인 `b1c85d2c`, `da78dd7e` UI 통합 변경을 챔피온에 계승하며, 챔피온 고유의 자율 목표 메뉴는 유지합니다.

## 1. 현재 사이드바 전체 15개 메뉴

| 분류 | 메뉴 | 프론트 | Rust API/영속 경로 | 핵심 엔진·검토 내용 |
| --- | --- | --- | --- | --- |
| 작업공간 | 홈 | `pages/Home.tsx` | `/api/status`, `/api/jobs` | `system.rs`, `jobs.rs`: 실행 현황·대시보드 |
| 작업공간 | 채팅 | `pages/Chat.tsx` | `/api/chat`, `/api/sessions/:id/messages`, `/api/approvals` | `engine/chat/runtime.rs`: 도구·세션·실행 증거 |
| 작업공간 | 그룹대화 | `pages/Rooms.tsx` | `/api/rooms`, `/api/rooms/:id/run`, `/api/rooms/:id/messages` | `engine/rooms.rs`: 화자/모델/도구 라우팅·예산 |
| 작업공간 | 모델 | `pages/Models.tsx` | `/api/providers`, `/api/model-library/*` | `providers.rs`, `engine/models.rs`: Provider·다운로드·로더 |
| 작업공간 | 에이전트 | `pages/Agents.tsx` | `/api/agents`, `/api/agents/bulk-model` | `agents.rs`, `engine/chat/runtime.rs`: 역할·모델 선택 |
| 작업공간 | 영상 | `pages/Video.tsx` | `/api/video/jobs`, `/api/video/config`, `/api/video/outputs` | `engine/video.rs`: 생성·영속 작업·출력 |
| 확장·연결 | 연결 채널 | `pages/Connections.tsx` → `Extensions.tsx` / `Capabilities.tsx` | `/api/channels`, `/api/oauth/connections`, `/api/mcp`, `/api/skills`, `/api/skill-catalog`, `/api/capabilities` | `channels.rs`, `extensions.rs`, `engine/mcp.rs`, `engine/skills.rs`: 채널·MCP·스킬·역량 마켓 |
| 확장·연결 | 기억 | `pages/Memory.tsx` → `memory/LearningRecords.tsx`, `memory/Growth.tsx` | `/api/memories`, `/api/memories/:id/edit`, `/api/memory/overview` | `engine/memory.rs`, `api/collections.rs`: 범위·검색·회상·수정·삭제 |
| 운영 | 자율 목표 (챔피온) | `pages/AutonomousGoals.tsx` | `/api/goals`, `/api/goals/:id/steer`, `/api/goals/:id/resume`, `/api/goals/:id/changes` | `engine/goals.rs`: 실행·조향·차이·복구 |
| 운영 | 자동화 | `pages/Operations.tsx` | `/api/automations`, `/api/jobs` | `engine/jobs.rs`: 반복 작업·내구성 있는 큐 |
| 운영 | 활동 | `pages/Operations.tsx` | `/api/audit`, `/api/jobs`, `/api/executions` | 감사 원장·작업·도구 기록 |
| 운영 | 자기개선 | `pages/SelfImprovement.tsx` | `/api/rsi/start`, `/api/rsi/campaigns`, `/api/rsi/campaigns/:id/verify` | `engine/rsi/*`: 실험·평가·채택·계승 |
| 운영 | 설정 | `pages/Operations.tsx` | `/api/policy`, `/api/settings/assistant` | `approvals.rs`, `settings.rs`: 권한·접근 |
| 고급 | 워크플로 | `pages/Workflows.tsx` | `/api/workflows`, `/api/workflows/:id/run` | `engine/workflows.rs`: 단계·결과·작업 |
| 고급 | 개발자 | `pages/Developer.tsx` | `/api/dev/status`, `/api/dev/diff`, `/api/dev/:kind` | `engine/dev.rs`: 저장소·빌드·검증·업데이트 |

표의 연결은 **파일/라우터를 읽어 확인한 코드 경로**입니다. 각각의 API가 실모델·외부 MCP·영상 공급자와 사용자 시스템에서 성공적으로 실행됐다는 의미는 아닙니다.

## 2. 연결 채널 탭 통합: 2026-11-14

- 기존 별도 사이드바의 `extensions`와 `capabilities` 항목을 숨기고, `connections` 하나만 노출합니다.
- `ConnectionsPage`는 **채널 연결 / MCP·스킬 / 역량 마켓** 3개 탭을 제공합니다.
- 채널 탭: 채널 설정과 OAuth/계정 연결, 실사용 진단.
- MCP·스킬 탭: 기존 `ExtensionsPage` 전체를 실제 렌더링. 마켓/설치됨/플러그인/스킬, 다운로드·가져오기·토글·삭제 API 경로를 유지합니다.
- 역량 마켓 탭: 기존 `CapabilitiesPage` 전체를 실제 렌더링. 제공 가능한 내장 도구·채널 카탈로그, 검색·상세·원래 연결 화면으로 이동 유지.
- 기존 `#/extensions`, `#/capabilities` 페이지 컴포넌트와 API는 호환성을 위해 남기되 사이드바·명령 팔레트에 별도의 중복 메뉴로 노출하지 않습니다.
- 챔피온 고유의 `goals` 페이지·엔진은 삭제하지 않았습니다.

## 3. 불필요한 자기개선 알림 제거

- 메인의 `ImprovementNotice`는 15초 간격으로 `/api/rsi/signals`를 조회하고 우측 하단 제안 배너를 렌더하던 구성입니다.
- 챔피온은 글로벌 `App.tsx`/쉘에서 이를 호출하지 않으며, `components/ImprovementNotice.tsx`를 **명시적 no-op 호환 모듈**로 고정했습니다. 자동 polling/timer, toast, popup, 자동 이동 없음.
- `openImprovementIssue`는 사용자의 **직접 조작**에 한해 API를 호출하도록 유지합니다.
- 실제 승인 요청·오류·캠페인 상태는 `SelfImprovement.tsx` 및 활동·로그 페이지 안에서 유지됩니다. 서버 RSI 검증/기억 자체는 삭제하지 않습니다.

## 4. 기억 화면 개선과 근거 보존

- 원래의 수동 저장·검색·상태/범위 필터·삭제·학습 기록/성장 대시보드를 유지합니다.
- **사용자 작성·활성·검색 가능 기억만 수정**합니다. `POST /api/memories/:id/edit`는 새 본문과 `expectedText`만 받습니다. `Store::edit_manual_memory`가 `expectedText`와 `expectedRevision`을 동시에 비교하고 source/status/expiry를 같은 SQLite 연결 락 아래 검증합니다. ABA 충돌도 거절합니다. 기존 ID·scope·agentId·partitionKey는 수정되지 않고 revision/editHistory/감사 기록을 남깁니다.
- 서버가 동적으로 계산하는 `retrievalEligible`은 저장 페이로드에 넣지 않습니다. 예전 문장에 연결된 인용 근거·verification은 수정 시 폐기해 최신 문장의 증거로 잘못 취급하지 않으며, 일반 `POST /api/memories`도 서버 생성 ID의 수동 기억 삽입만 허용합니다.
- 학습으로 생성된 기록과 원문 증거는 수동 텍스트 편집 대상에 넣지 않습니다.
- 실제 검색 정책과 범위 격리는 `engine/memory.rs`에서 유지됩니다.

## 4-1. 자율 목표 재개와 실패 복구

- 목표 생성은 job ID를 영속 목표 레코드에 기록하고, 작업 생성 실패를 성공으로 표시하지 않습니다.
- `failed` 또는 `needs_review`에 한해 소유자가 이전 외부 작업 상태를 확인한 뒤 `POST /api/goals/:id/resume`을 호출할 수 있습니다.
- 원래 `job`의 완료된 단계/확인 가능한 증거/세션을 참고해 **새로운 작업 레코드에 체크포인트를 삽입할 때부터 저장**합니다. 워커가 첫 재개 단계부터 지난 도구 호출을 반복하지 않게, 이전 in-flight 항목은 건너뛰고 읽기 전용 상태 재확인 지시를 줍니다.
- 실제 작업 오류를 기존 `goal`의 실패/검토 상태에 반영하므로 UI에서 실패가 고착된 running으로 표시되지 않습니다.
- 회귀 검증: `src/api/goals.rs` Rust 테스트와 `scripts/champion-ui-menu.test.mjs` 정적 연결 테스트. 외부 도구의 부작용에 대한 자동 취소나 실모델 성공률 보증은 아닙니다.

## 5. 검증 수준

| 항목 | 상태 | 근거 |
| --- | --- | --- |
| 메뉴 중복 제거 | 소스 반영 | `WorkspaceShell.tsx`에서 연결 채널 1개·기억/RSI/목표 유지 |
| 3개 탭 렌더링·기존 API 접근 | 정적 경로 확인 | `Connections.tsx`, `Extensions.tsx`, `Capabilities.tsx`, `api/mod.rs` |
| 전역 RSI 제안 차단 | 소스 반영 | 호환 컴포넌트 no-op, App 미마운트, 상태 로그는 그대로 |
| 수동 기억 수정·원자적 충돌 방지 | 코드 반영 | `Memory.tsx` → `collections.rs::action` → `Store::edit_manual_memory` / Rust 단위 테스트 |
| 소스 회귀 테스트 | 추가·CI 연결 | `scripts/champion-ui-menu.test.mjs`; CI는 Node 24에서 Rust 테스트 전에 실행 |
| 실제 브라우저 탭 전환·설치·팝업 미발생 | **미실행** | 사용자 환경의 DOM 이벤트/모델/MCP 연결 결과 아직 없음 |
| TypeScript 프로덕션 빌드·Rust 통합 CI | **이번 변경분 미실행** | GitHub Actions 이후 확인 필요 |

기존 정상 기능과 오류/승인/로그를 보존하는 것이 목적이며, 단순히 팝업을 숨겨 오류를 침묵시키는 방식으로 처리하지 않았습니다.
