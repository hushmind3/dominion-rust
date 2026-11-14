# 서브4 전체 프론트 메뉴 · API · 엔진 검증 지도 (2026-11-14)

> 브랜치: `dominion-sub4`. 조사 대상: 현재 브랜치 HEAD의 웹·Rust 소스.
> 심판 이슈 [#8](https://github.com/hushmind3/dominion-rust/issues/8)을 반영했다.
> **판정 구분**: S = 화면 컴포넌트와 API/엔진 소스 정적 연결 확인. C = 소스 텍스트에 대한 자동 계약 검사 수행. U = 실제 브라우저, Rust/TS 빌드, 실모델·운영 검증 미실시.
> 각 표의 S는 실제 호출 성공·사용자 경험·품질·비용 측정이 아니다.

## 1. 사이드바 4개 카테고리 / 15개 화면

`web/src/components/WorkspaceShell.tsx::navigation`은 메인 기준 14개 메뉴에 서브4 **독립 자율 목표** 1개를 보존해 15개다. 탭 3개는 별도 사이드바 메뉴로 계산하지 않는다. `web/src/App.tsx::locationState`와 lazy page route가 진입점이다.

| 범주 / 실제 메뉴 | UI 경로 | 주요 API 경로 | 백엔드·핵심 능력 | 결과·오류·재개 확인 지점 | 증거 |
| --- | --- | --- | --- | --- | --- |
| 작업공간 / 홈 | `pages/Home.tsx` | `/api/sessions`, `/api/jobs`, `/api/channels`, `/api/model-library/runtime` | `api/sessions.rs`, `engine/jobs.rs`, `engine/models.rs` | 최근 작업 상태 및 각 메뉴 진입, 호출 실패/연결 상태 | S/U |
| 작업공간 / 채팅 | `pages/Chat.tsx` | `/api/chat`, `/api/sessions/*`, `/api/approvals`, `/api/tools`, `/api/policy` | `api/chat.rs` → `engine/chat/{runtime,provider,context}` → `engine/tools.rs`, `gateway.rs` | 메시지·도구 영수증·승인·중지/대화 지속·세션 이력 | S/U |
| 작업공간 / 그룹대화 | `pages/Rooms.tsx` | `/api/rooms`, `/api/rooms/:id/messages`, `/api/rooms/:id/stop`, `/api/executions` | `api/rooms.rs` → `engine/rooms.rs`, `rooms/{speakers,economy}.rs`, `chat/room_context.rs` | 발언/모델 선택·사용량·공동 이력·중단·피드백 | S/U |
| 작업공간 / 모델 | `pages/Models.tsx` | `/api/providers`, `/api/providers/catalog`, `/api/providers/cli/*` | `api/providers.rs`, `api/models.rs`, `engine/gateway.rs`, `vault.rs` | 제공자 등록·키/CLI/OAuth·모델 선택·실패 표시 | S/U |
| 작업공간 / 에이전트 | `pages/Agents.tsx` | `/api/agents`, `/api/agents/bulk-model`, `/api/personas/import` | `api/agents.rs`, `engine/personas.rs`, `engine/jobs.rs` | 역할·모델 설정, 생성/삭제/가져오기, 실행과 연계 | S/U |
| 작업공간 / 자율 목표 (서브4) | `pages/Goals.tsx` | `/api/goals`, `/api/goals/:id/{steer,resume,cancel,apply,discard}`, `/api/jobs` | `api/goals.rs` → `engine/goals.rs` → `goal_git.rs`, `verification.rs`, `goal_control.rs` | 계획 버전·증거·결과 SHA·토큰 사용·중단/재개·격리 변경 검토 후 반영 | S/U |
| 작업공간 / 영상 | `pages/Video.tsx` | `/api/video/{config,jobs,outputs,status}` | `api/video.rs` → `engine/video.rs` → `video/{runway,comfy,quality,shot_plan,frame_anchor}.rs` | 생성 단계·유료 재제출 차단·클립 검사·산출물·재조회 | S/U |
| 확장·연결 / 연결 채널 | `pages/Connections.tsx` | `/api/channels`, `/api/oauth/{providers,connections}` | `api/channels.rs`, `api/oauth.rs`, `engine/channel_adapter.rs` | 채널 인증·진단·활성 상태·오류·전송 큐 | S/U |
| 확장·연결 / 기억 | `pages/Memory.tsx` | `/api/memories`, `/api/memories/:id/edit`, `/api/memory/overview`, `/api/synergy`, `/api/transfer`, `/api/model-usage` | `api/collections.rs`, `api/memory.rs` → `engine/memory.rs`, `learning.rs`, `chat/context.rs` | 범위별 검색·등록·직접 편집·삭제·근거·학습 이력·성장 집계 | S/U |
| 운영 / 자동화 | `pages/Operations.tsx` (`automations`) | `/api/automations`, `/api/jobs` | `api/collections.rs` → `engine/scheduler.rs`, `jobs.rs` | 반복 등록·실행 여부·오류·작업 상태 | S/U |
| 운영 / 활동 | `pages/Operations.tsx` (`activity`) | `/api/audit`, `/api/executions`, `/api/browser/status`, `/api/jobs` | `api/system.rs`, `api/executions.rs`, `api/browser.rs` → 실행 영수증·감사 저장 | 작업 로그·브라우저 상태·오류 추적·취소 정보 | S/U |
| 운영 / 자기개선 | `pages/SelfImprovement.tsx` | `/api/rsi/{start,campaigns}`, `/api/rsi/campaigns/:id/{choose,verify,apply}`, `/api/jobs/:id/cancel` | `api/rsi.rs` → `engine/rsi_evolve.rs` 및 `rsi/{planner,execution,evaluation,lineage,learning,meta}` | 독립 평가·선택·계승·중단/검증·작업 기록; 비방해 상태 조회 | S/U |
| 운영 / 설정 | `pages/Operations.tsx` (`settings`) | `/api/policy`, `/api/tools`, `/api/providers`, `/api/oauth/providers` | `api/approvals.rs`, `api/settings.rs`, `engine/policy.rs`, `vault.rs` | 도구 권한·확인·계정 설정 및 오류 | S/U |
| 고급 / 워크플로 | `pages/Workflows.tsx` | `/api/workflows`, `/api/workflows/:id/run`, `/api/jobs` | `api/collections.rs` → `engine/workflows.rs`, `jobs.rs` | 단계별 에이전트 작업·실행 상태·세션 결과 | S/U |
| 고급 / 개발자 | `pages/Developer.tsx` | `/api/dev/{status,config,diff,update,jobs}`, `/api/dev/preview/*` | `api/dev.rs` → `engine/dev.rs`, `src-tauri` | Git diff·작업·빌드/업데이트·오류/작업로그 | S/U |

## 2. 연결 채널 내부의 진짜 기능

- **채널 연결**: `ConnectionsPage` → `api/channels.rs`, OAuth 계정 연결, 플랫폼 카탈로그, 채널 등록/진단 및 활성 상태.
- **MCP·스킬**: 기존 `ExtensionsPage` 그대로 표시 → `/api/mcp`, `/api/mcp/discover`, `/api/mcp/catalog`, `/api/skills`, `/api/skills/import-repo`, `/api/skill-catalog/import` → `api/extensions.rs`, `api/collections.rs`, `engine/mcp.rs`, `skills.rs`. 하위 탭은 **마켓 / 플러그인 / 설치됨 / 스킬**을 유지한다.
- **역량 마켓**: `CapabilitiesPage` → `/api/capabilities`, `/api/channels/catalog`로 제공 능력/상태를 보여주고 채널·MCP 화면으로 이동.
- **연결 호환**: `#/extensions` 또는 `#/capabilities` 구형 링크는 `App.locationState`에서 통합 연결 화면으로 매핑한다. `#/connections?section=extensions|capabilities`도 지원한다. `showSection`이 URL을 저장하므로 탭을 새로고침해도 복원 가능.
- **없애지 않은 경로**: `web/src/pages/Extensions.tsx`, `Capabilities.tsx`는 삭제하지 않고 연결 탭 안에서 재사용한다. 메뉴 통합과 실제 MCP/스킬 실행 경로 보존을 분리해서 점검한다.

## 3. 기억 하위 기능

- **기억 등록**: `MemoryPage.save` → `POST /api/memories` → `collections::save` → SQLite.
- **직접 편집 (추가)**: `MemoryPage.update` → `POST /api/memories/:id/edit` → `collections::action` → `Store::patch_if`. 이전 텍스트가 변했으면 충돌 오류. 직접 등록한 `source=user` 활성 기억만 편집할 수 있고 출처/증거/agentId/scope는 갱신하지 않는다.
- **검색·표시**: `GET /api/memories`가 `retrievalEligible`을 계산해 반환; 화면은 상태·담당 에이전트·세션 범위별 필터. 검색 엔진은 `engine/memory.rs`의 검색/중복·기간·가중치 정책이며 실제 채팅 문맥에서 사용하는 경로는 `engine/chat/context.rs` 및 관련 런타임에 위치한다.
- **삭제**: `DELETE /api/memories/:id` → collection store 제거. UI에서 확인 후 진행.
- **학습/성장**: `memory/LearningRecords.tsx`와 `memory/Growth.tsx`는 학습 작업·전이·알고리즘·모델 사용량을 개별 API로 읽는다. 대화 비용/검색 품질은 별도 실측이 필요하다.

## 4. 자기개선 알림과 안전한 실패 표시

`SelfImprovementPage`는 처음 페이지를 열었을 때만 기록을 읽고 작업 상태를 표시한다. `/api/rsi/campaigns/:id`, `/api/dev/jobs/:id` 활성 작업 조회 주기를 1.5초 → 5초로 완화하고 완료·취소·실패 상태에서는 타이머를 종료했다. 상태 폴링 실패는 `Notice(role=alert)` 반복 호출 대신 **페이지 내부 비방해형 텍스트**(`role=status`, `aria-live=off`)로 표시한다. 명시적 실행/적용 오류와 영구 작업 감사 기록은 삭제하지 않는다.

**범위 주의**: 이 변경은 DOMINION 프론트의 해당 경로이다. 브라우저/운영체제 자체 알림, 외부 스케줄러, 사용자가 만든 별도 알림은 전역 차단하지 않는다. 자동 팝업 전면 제거의 E2E 증거는 아직 없다.

## 5. 이번 변경에서 실제 실행한 소스 계약 체크

연결된 GitHub 커넥터로 브랜치 파일을 재조회해 JS 정적 assertion **12/12 통과**:

1. 사이드바 범주 4개
2. 서브4 독립 목표를 포함한 항목 15개
3. extensions/capabilities 별도 사이드바 항목 미노출
4. 기존 직접 링크 connections 변환 코드
5. 연결 탭 3개와 선택 기억
6. MCP·스킬 API 유지
7. 역량 카탈로그 API 유지
8. RSI 기존 1.5초 폴링 구문 제거
9. RSI 반복 `Notice` 알람 제거 및 비방해형 상태
10. RSI 완료 상태에서 폴링 중단
11. 기억 편집 UI 진입
12. 기억 편집의 서버 출처·동시 수정 검사

이는 **소스 문자열/호출 진입점 검증(C)**이다. 타입 검사, Rust 컴파일, 실제 클릭, MCP 설치·OAuth, 모델 호출, 영상 생성, RSI 자가개선 결과 및 비용 절감 측정은 실행하지 않았다(U).

## 6. 추후 실행 검증 시나리오 (미실행)

| 화면/시나리오 | 확인할 결과 |
| --- | --- |
| 3개 통합 탭 및 `#/extensions`, `#/capabilities` 옛 링크 | 탭 열림·페이지 재진입 시 상태 보존, 퀵내비 중복 항목 없음 |
| MCP 스킬 마켓 검색→설치→도구 발견→비활성/삭제 | API 영수증·실제 도구 호출·저장 데이터·보안 승인 모두 일치 |
| 기억 사용자 입력→수정, 동시 편집 충돌, 학습 기억 수정 시도 | 원래 scope/증거 유지, 동시 수정은 409, 자동 학습 증거 직접 변경 불허 |
| 종료된 RSI 캠페인 30초 이상 열어두기, 서버 연결 장애/복구 | 전역 팝업·반복 경고 없이 내용/오류가 페이지 내부에 유지, 작업 이력 유지 |
| 채팅·그룹·에이전트·목표·영상·자동화·워크플로·개발자별 실제 실행 | 생성→실도구/제공자→검증→중지/실패 복구→산출물과 비용 추적 |

통합 브랜치·챔피온은 이 문서의 **S/C/U를 그대로 유지**하여 아직 실행하지 않은 기능을 심사 만점으로 승격시키지 않아야 한다.
