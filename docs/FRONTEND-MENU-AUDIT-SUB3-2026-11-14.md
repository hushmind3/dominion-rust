# 서브3 · 15개 메뉴 연결·RSI 알림·기억/목표 신뢰성 점검 (2026-11-14)

## 목적과 출처

2026-11-14 심판 이슈 [#8](https://github.com/hushmind3/dominion-rust/issues/8), 메인 `WorkspaceShell.tsx`/ `Connections.tsx` 최신 3탭 구조를 `dominion-sub3`에 계승했습니다. 서브3의 자율 목표 하네스, 독립 Git/RSI 경로, 기존 MCP·스킬 설치 API 및 그룹대화 구현은 제거하지 않았습니다.

## 실제 프론트 UX

- **사이드바 4개 카테고리, 15개 메뉴**: 작업공간 7(독립 `자율 목표` 추가), 확장·연결 2, 운영 4, 고급 2. `extensions`/`capabilities`는 더는 별도 사이드바 항목이 아닙니다.
- **연결 채널 3개 탭**: `ConnectionsPage` → 채널 연결(`ChannelConnectionsPanel`) / MCP·스킬(`ExtensionsTab`) / 역량 마켓(`CapabilitiesTab`). 각 탭은 기존 화면 및 API를 재사용하며, 채널 외 화면은 사용자가 탭을 열 때 `React.lazy`로 로드합니다. 다른 탭 사용 중 채널 훅은 마운트되지 않습니다.
- **기존 링크 유지**: `#/extensions`, `#/capabilities`, `#/connections?section=extensions`, `#/connections?section=capabilities`가 모두 같은 연결 허브로 연결됩니다. 탭 이동은 URL 기록을 갱신하며 뒤로 가기에서 복원합니다.
- **RSI 자동 알림 제거**: 전역 `ImprovementNotice` 마운트와 15초마다 `/api/rsi/signals`를 폴링하던 컴포넌트를 삭제했습니다. 자동 팝업·반복 추천 알림은 이 UI 경로에서 더 이상 발생하지 않습니다. 수동 `Chat.tsx`의 `자기개선으로 수정`은 `ImprovementIssue.ts`의 `POST /api/rsi/issues`를 통해 유지됩니다. RSI 캠페인 결과·오류·승인·실행 기록은 각 화면과 서버 저장소에 남습니다.

## 메뉴 → 화면 → API → 엔진 → 결과/복구 경로 전수 점검표

| 메뉴 | 화면 | 대표 API | 엔진/영속 처리 | 소스 확인 / 남은 검증 |
|---|---|---|---|---|
| 홈 | `Home` | `/api/sessions`, `/api/jobs`, `/api/providers` | 세션/작업 DB | 화면·정적 경로 확인 / 브라우저 확인 필요 |
| 채팅 | `Chat` | `/api/sessions/:id/messages`, `/api/approvals`, `/api/tools` | `chat/runtime`, `tools`, 승인 게이트, SQLite | 메시지·취소·도구 상태 연결 / 실모델 확인 필요 |
| 그룹대화 | `Rooms` | `/api/rooms/:id/messages`, `/state`, `/stop` | `rooms`, `room_context`, 공유 메시지 | 발언·중지·그룹지식 화면 연결 / 실행 확인 필요 |
| 모델 | `Models` | `/api/providers`, `/api/providers/:id/models` | `gateway`, `models`, `vault` | 공급자·선택·인증 UI / 실모델 확인 필요 |
| 에이전트 | `Agents` | `/api/agents` | 페르소나 저장·실행·권한 | 설정·등록 UI / 실작업 확인 필요 |
| 자율 목표 | `Goals` → `GoalWorkbench` | `/api/goals`, `/api/goals/:id/resume`, `/api/goals/:id/changes`, `/api/goals/:id/apply` | `goals`, `goal_control`, `goal_git`, `jobs` | 별도 메뉴·검토 후 수동 재개·SHA 패치 검토 경로 / 실에이전트 장기 실행 확인 필요 |
| 영상 | `Video` | `/api/video/jobs`, `/api/video/outputs` | `video`, `video/quality`, 작업 큐 | 산출·품질 검사 경로 / 실제 유료 생성 미검증 |
| 연결 채널 | `Connections` + `Extensions` + `Capabilities` | `/api/channels`, `/api/oauth/connections`, `/api/mcp`, `/api/skills`, `/api/skill-catalog`, `/api/capabilities` | `channels`, `mcp`, `skills`, OAuth | 새 통합 메뉴·기존 API 경로 확인 / 설치 실동작 추가 확인 |
| 기억 | `Memory` + `memory/LearningRecords` + `memory/Growth` | `/api/memories`, `/api/agents` | `memory`, `learning`, 채팅 문맥 검색·주입 | 목록·등록·검색·삭제·scope·성장 UI 확인. **직접 입력 활성 기억만 편집 가능**. 원문 대조 CAS, 출처/상태 확인, 감사 기록 원자적 저장. 학습 기억 수정 금지 |
| 자동화 | `Operations(automations)` | `/api/automations`, `/api/jobs` | `jobs` 스케줄러, 영속 큐 | 등록/반복/상태 UI / 시간 기반 실동작 확인 필요 |
| 활동 | `Operations(activity)` | `/api/executions`, `/api/audit`, `/api/browser/status` | 실행·감사 로그, 브라우저 | 실활동 상태·로그 UI / 브라우저 연결 확인 필요 |
| 자기개선 | `SelfImprovement` | `/api/rsi/campaigns`, `/api/rsi/start`, `/api/rsi/evaluation-profiles` | `rsi_evolve`, 동결 평가·채택·계보 | 전용 화면 유지·전역 팝업 제거 / 실세대 채택 확인 필요 |
| 설정 | `Operations(settings)` | `/api/policy`, `/api/tools`, `/api/oauth/providers` | 정책·승인·암호화 저장 | 권한/설정 UI / 실제 정책 변경 검증 필요 |
| 워크플로 | `Workflows` | `/api/workflows`, `/api/workflows/:id/run` | `workflows`, `jobs` | 편집·실행 UI / 단계 복구 검증 필요 |
| 개발자 | `Developer` | `/api/dev/status`, `/api/dev/diff`, `/api/dev/{test,build,commit}` | `dev`, Git, 빌드·프리뷰 | 상태·Git·명령 UI / 실제 워크스페이스 작업 확인 필요 |

**판정 경계:** 표의 '확인'은 *GitHub 소스의 화면·호출 연결 확인*입니다. 실브라우저 조작·실제 모델 품질·네트워크 전송·복구 성공을 실행한 근거로 쓰지 않습니다. 각 메뉴의 실사용 흐름은 통합 테스트 항목으로 남깁니다.

## 회귀 보호와 실행

`scripts/sub3-connection-hub.test.mjs`는 Node 내장 테스트로 다음 정적 계약을 검사합니다.

1. 15개 메뉴 및 연결 허브의 단일 진입점.
2. 구형 확장/역량 마켓 북마크 호환성과 URL 기반 탭 상태.
3. 기존 MCP/스킬/마켓 API 및 지연 로딩 진입점.
4. 자동 RSI 팝업/폴링 코드 삭제 + 수동 개선 이슈 생성 유지.
5. 기억 독립 메뉴 및 기록/성장 하위 화면 유지.
6. 실패·검토 대기 목표의 사용자 승인 재개 API와 독립 목표 메뉴.
7. 기억 편집의 서버 출처 보호·CAS·원자적 감사 기록과 편집 UI.

CI의 `node --test scripts/sub3-connection-hub.test.mjs`에 등록했습니다. 이는 실제 브라우저 테스트를 대체하지 않으며, CI 완료 여부는 해당 커밋의 Actions 상태로 별도 검증해야 합니다.

### 실브라우저 수동 시나리오(미실행)

1. 모바일·데스크톱 사이드바에서 연결 채널 1개 메뉴만 확인. 3개 탭 전환·뒤로 가기·새로고침·직접 주소 진입.
2. `MCP·스킬`에서 마켓/설치됨/플러그인/스킬 보기·등록·비활성화 확인. `역량 마켓`에서 채널 연결 버튼으로 돌아오기.
3. 자기개선 화면이 아닌 홈/채팅을 1분 이상 열어 자동 RSI 팝업이 없는지 확인. 수동 채팅 오류 화면의 '자기개선으로 수정'은 정상 동작 확인.
4. 기억 화면에서 직접 작성·활성 기억 편집, 오래된 내용 동시 수정 충돌(409), 학습 기억 편집 불가, 검색·scope/학습 기록/성장 조회.
5. 자율 목표 메뉴에서 시작·중단·검토 후 재개·분리된 Git 변경 검토·패치 적용, 중단 작업 복구 정책 확인.

## 서버 불변식 (2026-11-14 보강)

- `src/db.rs::edit_owner_memory`는 SQLite 단일 트랜잭션에서 `source=user`, `status=active`, `text=expectedText` 검증 후 텍스트를 교체합니다. 감사 테이블 `memory.owner_edited` 행도 같은 트랜잭션에 저장하고, 감사 상세에 원문은 포함하지 않습니다. 출처·scope·검증 근거·세션 참조는 그대로 유지됩니다.
- `POST /api/memories`는 신규 사용자 작성 기억만 수용합니다. ID 지정이나 학습/증거 출처 위조는 거부합니다. 기존 레코드 편집은 `POST /api/memories/:id/edit`의 엄격한 `{text,expectedText}` 계약으로만 처리합니다.
- `POST /api/goals/:id/resume`는 `failed/needs_review` 상태에서만 `acknowledgeUnknownEffects=true` 수동 승인을 요구합니다. 신규 durable job ID, 이어가기 이력, 반복·재개 상한, 이전 활성 작업 여부 검사 후 작업을 등록합니다. 복구 검증 없이 이전 도구의 부작용을 자동 재생하지 않습니다.
- `src/api/collections.rs`의 Rust 테스트는 원자적 경합·학습 출처 위변조 차단·감사 이력, `scripts/sub3-connection-hub.test.mjs`의 Node 테스트는 프론트·라우트 연결 계약을 검사합니다. **Rust 컴파일/런타임·실브라우저 성공 여부는 CI 결과로 별도 보고**합니다.
