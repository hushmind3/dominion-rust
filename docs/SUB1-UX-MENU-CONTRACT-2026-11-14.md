# Sub1 프론트 14메뉴·연결 통합·자기개선 비방해 계약 (2026-11-14)

> 판정 기준: [공통지침](../README.md)의 전체 제품 완성도. 정적 코드 연결과 CI 타입/빌드 검증을 실제 브라우저·실모델 완수 검증으로 혼동하지 않습니다. 현재 변경의 주목적은 최신 main의 통합 메뉴를 Sub1에 계승하고 전역 RSI 추천 알림을 억제하는 것입니다.

## 1. 4개 카테고리·14개 실제 노출 메뉴

`web/src/components/WorkspaceShell.tsx::navigation`에 4개 그룹·14개 노출 항목. 옛 `extensions`/`capabilities`는 사이드바에서는 제거하되 `web/src/App.tsx::locationState`에서 옛 해시 URL을 `connections`의 해당 탭으로 매핑합니다. 검색 명령 팔레트도 `navigation`에서 직접 구축하므로 중복 항목이 사라집니다.

| 카테고리 | 메뉴 | 프론트 → API → 실행/상태 근거 | 확인 및 추가 검증 |
|---|---|---|---|
| 작업공간 | 홈 | `pages/Home.tsx` → `/api/sessions`, `/api/jobs`, `/api/providers` → 세션/Job 대시보드 | 정적 연결, 클릭 탐색 필요 |
| 작업공간 | 채팅 | `pages/Chat.tsx` → `/api/chat` → `api/chat.rs` → `engine/chat/runtime/turn.rs` 및 도구 승인/세션 기록 | 모델·도구 호출의 실제 결과 추가 검증 |
| 작업공간 | 그룹대화 | `pages/Rooms.tsx` → `/api/rooms` 및 메시지/실행/중지 → `api/rooms.rs` → `engine/rooms.rs` | 참가자/비용/중단·재개 실사용 추가 검증 |
| 작업공간 | 모델 | `pages/Models.tsx` → `/api/providers`, `/api/model-library/*` → `api/providers.rs`, `api/models.rs` | Provider/OAuth/로컬 런타임 실연결 추가 검증 |
| 작업공간 | 에이전트 | `pages/Agents.tsx` → `/api/agents` → `api/agents.rs` → `engine/personas.rs` | 신규 생성·삭제/역할 실행 실사용 추가 검증 |
| 작업공간 | 영상 | `pages/Video.tsx` → `/api/video/jobs`, `/api/video/config` → `api/video.rs` → `engine/video.rs` | FFmpeg 파일/요금/재생성 영수증 실검증 추가 |
| 확장·연결 | 연결 채널 | `pages/Connections.tsx` → `channels`·`oauth`·`mcp`·`skills`·`capabilities` API → `api/{channels,oauth,collections,extensions,system}.rs` | 3개 탭·설치/계정/실행 경로 정적 연결, 실제 연결 실험 별도 |
| 확장·연결 | 기억 | `pages/Memory.tsx` → `/api/memories`, `/api/memories/:id/edit` → `api/collections.rs` → `engine/memory.rs` → 채팅 컨텍스트 | 수동 기억 수정 범위/동시수정/출처 불변 테스트 추가 |
| 운영 | 자동화 | `pages/Operations.tsx` → `/api/automations` → `api/collections.rs` → `engine/scheduler.rs` | 예약·반복 Job 실재개 추가 검증 |
| 운영 | 활동 | `pages/Operations.tsx` → `/api/executions`, `/api/audit`, `/api/browser/status` | 실제 동작/오류·실행 기록 조회 추가 검증 |
| 운영 | 자기개선 | `pages/SelfImprovement.tsx` → `/api/rsi/campaigns`, `/api/rsi/start` → `api/rsi.rs` → `engine/rsi*.rs` | RSI 페이지 내에만 캠페인 상태 표시; 실제 계승 검증 별도 |
| 운영 | 설정 | `pages/Operations.tsx` → `/api/policy`, `/api/oauth/providers`, `/api/tools` | 실제 승인 정책·비밀 상태 추가 검증 |
| 고급 | 워크플로 | `pages/Workflows.tsx` 및 `workflows/GoalConsole.tsx` → `/api/workflows`, `/api/goals`, `/api/jobs` → `engine/goals.rs` | 작업 완료·체크포인트/복구·검증 영수증 별도 |
| 고급 | 개발자 | `pages/Developer.tsx` → `/api/dev/status`, `/api/dev/diff`, `/api/dev/update` → `api/dev.rs`, `engine/dev.rs` | Git/빌드 작업·재시작 추가 검증 |

## 2. 연결 채널 통합: 프론트 → API → 실행

- 상위: `App.tsx` → `ConnectionsPage`, 세부 탭 **채널 연결 / MCP·스킬 / 역량 마켓**. `WorkspaceShell.tsx`와 `CommandPalette`는 연결 채널 1개만 노출합니다.
- 채널: `/api/channels/catalog`, `/api/channels`, `/api/channels/:id`, 진단 엔드포인트 및 OAuth 계정/제공자. 기존 저장/삭제/진단/연결 편집기를 그대로 사용합니다.
- MCP·스킬: `ConnectionsPage` → 기존 `ExtensionsPage` → `/api/mcp`, `/api/skills`, `/api/mcp/presets`, `/api/skill-catalog`, `/api/skills/import-repo` → 기존 엔진에서 설치/선택/실행. 화면을 합친 것이 백엔드 삭제나 신규 설치 가능 보증을 뜻하지 않습니다.
- 역량 마켓: `ConnectionsPage` → `CapabilitiesPage` → `/api/capabilities` + `/api/channels/catalog`. 상세 정보에서 연결 채널 탭으로 복귀합니다.
- URL: `#/connections?section=channels|extensions|capabilities` 직접 진입을 지원. 기존 `#/extensions`, `#/capabilities` 북마크는 통합 탭으로 연결합니다. 상위 탭은 `tablist/tab/tabpanel` 접근성 역할을 부여했습니다.

## 3. 자기개선 추천 알림 비노출

- `web/src/main.tsx` → `App.tsx`에 `ImprovementNotice` 또는 `openImprovementIssue` 장착이 없습니다. 이 브랜치는 전역 추천 팝업/자동 RSI 홍보 polling을 등록하지 않습니다.
- `App.tsx` 이벤트 루프의 `dominion-refresh`는 단순 데이터 갱신 알림입니다. 사용자 화면 위에 토스트/팝업을 표시하지 않습니다.
- `SelfImprovementPage`의 1.5초 상태조회는 **해당 페이지가 열린 동안에만** 작동하며, 오류/진행/검증 상태는 페이지 안에 표시합니다. 사용자 승인·진짜 실패 메시지는 숨기지 않습니다.
- 다른 브랜치의 알림 컴포넌트를 병합하더라도 `App.tsx`/엔트리에 자동 장착하지 않는 것이 제품 계약입니다.

## 4. 기억 직접 수정: 출처 보존

- 기존: 목록·검색·등록·삭제/기억 범위/학습 기록/성장 탭. 이번: **사용자 수동 등록(source=user)** 기억에만 인라인 수정/저장/취소를 추가했습니다.
- API `POST /api/memories/:id/edit`는 현 레코드 ID를 다시 읽고 `expectedText`를 비교하여 오래된 화면의 덮어쓰기를 409로 거부합니다. 404/400/403과 기존 source/범위 보호를 적용합니다.
- 저장 시 `text`, `updatedAt`만 수정하며 과거 인용·원문 증거 표시는 지우고 `verification=manual_edit`로 구분합니다. 학습 기억의 출처를 사용자가 바꾼 것처럼 보이게 하지 않습니다.
- `src/api/collections.rs`에 성공·충돌·학습 출처 거부·유효성 테스트를 추가했습니다.

## 5. 검증 분류 및 명령

- **정적 계약 검사**: `node scripts/verify-frontend-navigation.mjs` (14개 메뉴/3탭, API 경로, 구주소, 전역 RSI 알림 미탑재, 메모리 API 수정 경로). CI의 `npm run typecheck --prefix web`, `npm run build --prefix web`, `cargo test --release`와 함께 실행됩니다.
- **실제 브라우저 미완료 항목**: 3탭 실제 클릭, 기존 URL 직접 접근, MCP 설치/삭제/실행, OAuth 리다이렉트, 기억 수정-새로고침, 15초 이상 대기 시 RSI 추천 팝업 미발생. 이 문서와 소스 검사는 브라우저 실험 결과가 아닙니다.
- **실제 과금/RSI 메타 계승**은 별도 운영 측정이 필요합니다. 자기개선 성공 알림·품질 상승을 임의로 생성하지 않습니다.

### 실브라우저 UI 계약 시나리오 (수동 실행)

`web/tests/connections.browser.mjs`에서 Chromium으로 실제 클릭·탭 selected 상태/URL/옛 북마크/수동 기억 수정 성공을 테스트하고, 채팅 화면을 이전 제안 폴링 주기인 15초보다 오래 열어 자기개선 자동 팝업과 네이티브 alert 미발생을 감시합니다. 서버/API는 deterministic stub이라 MCP 프로세스 연결이나 실제 과금/RSI 채택의 증거는 아닙니다. `.github/workflows/sub1-ui-browser-smoke.yml`은 수동 실행 전용이며 GitHub Actions의 일반 push CI에 브라우저 설치 부하를 추가하지 않습니다.
