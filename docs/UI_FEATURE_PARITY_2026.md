# Dominion sub4 — latest main frontend baseline and native API gap

**As of 2026-11-15:** latest `main` HEAD is `879dc23dce309688691c200b001b180bead95a12`. Its `web/` Git tree `9e2be9b7376c1eefb6195e3ea7f9463b952456c0` is copied intact into `dominion-sub4`.

## Accurate distinction

The **full frontend source is matched**, including all main navigation and page implementations. The native sub4 Rust server is **not** the main server: the frontend expects `/api/*` but the sub4 backend currently serves `/api/v1/*`. A page whose API is missing will not fully function. We deliberately removed the sub4 substitute screens instead of displaying incomplete features as if complete.

| Original main page | Some original main service contracts | Sub4 native service(s) requiring true frontend compatibility |
| --- | --- | --- |
| Chat | `/api/sessions`, `/api/agents`, `/api/approvals` | `/api/v1/runs`, run events/approval; no full session/profile API |
| Models | `/api/providers`, `/api/model-library/runtime` | `/api/v1/config`, configured model provider; no full provider CRUD |
| Agents | `/api/agents`, `/api/personas/import` | `/api/v1/runs/{id}/subagents`; no profile CRUD |
| Rooms | `/api/rooms`, room session/control/knowledge | `/api/v1/groups`; different schema/capabilities |
| Video | `/api/video/*`, `/api/youtube/*` | `/api/v1/videos`; different operations |
| RSI | `/api/rsi/campaigns`, `/api/rsi/start` | `/api/v1/improvements`; different experiment UI/data |
| Memory | `/api/memories`, `/api/agents` | `/api/v1/memory`; distinct entry schema |
| Workflows / automations | `/api/workflows`, `/api/automations`, `/api/jobs` | No equivalent complete workflow/scheduler contract |
| Extensions / connections | `/api/mcp`, `/api/skills`, `/api/channels` | Rust skills/MCP execution, incomplete management API |
| Operations / developer | `/api/executions`, `/api/dev/*` | Rust run journal, tools; distinct operational contract |
| Profile | `/api/settings/owner-profile` | `/api/v1/profile`; not routed under legacy name |
| Development server | Proxy to `127.0.0.1:4317` | Sub4 Rust binds `127.0.0.1:4317`; no full route adaptation |

The correct follow-on work is to implement actual main API semantics **in Rust**, with durable state and proper security, while preserving original frontend source and behavior. Do not create fabricated lists, dummy buttons, synthetic success responses, or simplified page replacements.

See `docs/FRONTEND_ALIGNMENT.md` for exact Git object evidence. The frontend-only tree replacement was not compiled or tested; no CI was run at user direction.

## 실제 2026-11-15 연결 진행

이 문서 위 표는 호환 API 도입 이전 상태입니다. 메인 프런트 소스를 수정하지 않고 `/api/*` 요청을 Rust에서 직접 처리하는 `src/frontend_api.rs`를 추가했습니다. 홈·기본 채팅·에이전트·세션·도구 승인·실행 기록·기억·프로필은 Rust 데이터와 연결됩니다. 모델 추가/교체, 로컬 런타임 제어, RSI 캠페인, 영상 관리/업로드, 그룹 세부 제어, 워크플로/자동화/채널/MCP 관리, 브라우저 운영 등은 아직 미연결입니다. 세부 API 표는 `docs/FRONTEND_RUST_CONTRACT.md`를 기준으로 합니다.

연결이 없는 API는 성공 응답 대신 HTTP 501 오류로 끝납니다. 채팅의 도구 비활성, 개별 추론 설정, 자기학습 끄기 등 현재 엔진이 보장하지 못하는 옵션도 조용히 무시하지 않습니다. CI·테스트·실제 UI 작동 검증은 수행하지 않았습니다.

### 최신 main 영상 화면 연결 (2026-11-15)

본래 `VideoPage` 파일은 수정하지 않았습니다. `/api/video/status`, `/outputs`, `/jobs`, `/jobs/{id}`, `/jobs/{id}/content`를 Rust `Videos` 저장소/FFmpeg 렌더러에 연결했습니다. 제목, 3–180초 길이, 9:16·16:9·1:1 비율이 실제 렌더링 값에 사용됩니다. 텍스트 타이틀 카드+선택적 TTS 영상으로, ComfyUI의 생성 영상이나 유튜브 업로드를 구현했다고 표시하지 않습니다. 지원하지 않는 외부 기능은 명시적으로 거부합니다. 해당 변경에 빌드·테스트·CI는 실행하지 않았습니다.
