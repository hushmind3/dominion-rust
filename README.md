# dominion-rust

DOMINION을 **Rust 백엔드 + Rust 실행 엔진**으로 다시 구현한 저장소입니다.

- 프론트엔드: 기존 React UI 그대로 (`web/`)
- 백엔드: Rust (axum)
- 실행 엔진: Rust (모델 스트리밍, 추론 토큰, 도구 호출 루프, 승인 게이트)
- 저장소: SQLite (WAL), 레코드는 JSON 문서
- Node.js: 런타임이 아니라 **빌드 도구로만** 사용 (`vite build`)

## 구조

```
src/
  main.rs            부트스트랩 (설정, DB, 키, 라우터)
  config.rs          DOMINION_PORT / DOMINION_DATA / DOMINION_WEB / DOMINION_WORKSPACE
  db.rs              SQLite 레코드·메시지·감사 로그·시크릿
  vault.rs           API 키 암호화 (ChaCha20-Poly1305, 키 파일 0600)
  events.rs          /api/events 용 인프로세스 이벤트 버스
  state.rs           공유 상태 + 승인 대기열
  engine/
    chat.rs          스트리밍 채팅 + 도구 호출 루프 + 승인 요청
    tools.rs         Rust 네이티브 도구 (파일/셸/HTTP/검색/메모리/스킬)
    jobs.rs          내구성 있는 작업 큐 + 자동화 스케줄러
    learning.rs      장기기억·스킬 초안 추출 (재학습)
    llm.rs           계획/검토/학습용 비스트리밍 호출
    workflows.rs     순차 워크플로 실행 ({{input}} / {{previous}})
    rooms.rs         멀티 에이전트 라운드테이블 실행
    goals.rs         자율 목표 루프 (도구 사용 · DONE 판정)
    compact.rs       대화 컨텍스트 압축 (요약 체크포인트)
    models.rs        로컬 모델 런타임 (llama.cpp) · HF 검색/다운로드
    oauth.rs         GitHub/Google OAuth 설정·state·토큰 교환
    skills.rs        SKILL.md 카탈로그 스캔 · 가져오기
    dev.rs           소스 개발 스튜디오 (git · npm · vite 미리보기)
    mcp.rs           MCP stdio/HTTP 클라이언트 + 도구 네임스페이스
  api/
    core.rs          핵심 API
    collections.rs   JSON 컬렉션 CRUD
    compat.rs        아직 이식되지 않은 표면의 명시적 응답
web/                 React 프론트엔드 (Vite)
```

## 실행

**평소:** `/Applications/DOMINION.app` 더블클릭 → 이미 도는 Rust 서버에 Chrome만 연결.
**서버는 로그인 시 자동 시작되고, 죽으면 launchd가 다시 띄웁니다.**

```bash
./target/release/dominion-rust --install-app /Applications/DOMINION.app
./scripts/install-launch-agent.sh
```

`DOMINION.app`의 실행 파일은 셸/AppleScript가 아니라 **Rust 바이너리 자체**입니다.
`--open` 이면 서버 기동 후 Chrome을 열고, 이미 4317이 사용 중이면 브라우저만 엽니다.
번들 앱은 `Contents/Resources/home`에 저장소 경로를 기록해 Finder에서 실행해도
`runtime/`, `web/dist`를 정확히 찾습니다.

개발 중 수동 실행:

```bash
./start-rust.command          # 소스가 바뀐 경우에만 빌드
DOMINION_FORCE_BUILD=1 ./start-rust.command
```

수동 실행:

```bash
source "$HOME/.cargo/env"
cargo build --release
DOMINION_PORT=4317 DOMINION_WEB=web/dist DOMINION_DATA=runtime \
  ./target/release/dominion-rust
```

## API

React 클라이언트가 이미 사용하는 계약을 그대로 구현했습니다.

| 영역 | 경로 |
| --- | --- |
| 상태 | `GET /api/health`, `GET /api/status`, `GET /api/events` |
| 채팅 | `POST /api/chat` (SSE: `text`, `reasoning`, `tool_start`, `tool_end`, `tool_error`, `approval`, `assistant_saved`, `done`, `error`) |
| 제공자 | `GET/POST /api/providers`, `DELETE /api/providers/:id`, `GET /api/providers/:id/models` |
| 세션 | `GET/POST /api/sessions`, `GET/DELETE /api/sessions/:id`, `.../messages`, `.../rename`, `.../clear`, `.../fork` |
| 승인 | `GET/POST /api/approvals` |
| 정책 | `GET/POST /api/policy` |
| 컬렉션 | `agents`, `memories`, `skills`, `mcp`, `automations`, `workflows`, `rooms`, `channels`, `jobs`, `executions`, `goals` |
| 작업 | `GET /api/jobs`, `POST /api/workflows/:id/run`, `POST /api/rooms/:id/run`, `POST /api/goals` |
| 실행 기록 | `GET /api/executions`, `GET /api/executions/:id/operations`, `POST /api/executions/:id/cancel`, `POST /api/sessions/:id/cancel` |
| 개발 스튜디오 | `GET /api/dev/status`, `POST /api/dev/connect`, `GET /api/dev/diff`, `POST /api/dev/{setup,test,build,package,commit}`, `POST /api/dev/preview/{start,stop}` |
| MCP | `GET /api/mcp`, `GET /api/mcp/discover`, `GET /api/mcp/catalog` (공식 Registry), `GET /api/tools` (`mcp__서버__도구`) |
| 스킬 | `GET /api/skill-catalog`, `POST /api/skill-catalog/import`, `POST /api/skills/import-repo`, `GET /api/skills/:id/export` |
| 컨텍스트 | `POST /api/sessions/:id/compact` |
| 로컬 모델 | `GET /api/model-library/{catalog,details,installed,jobs,runtime}`, `POST /api/model-library/{download,import}`, `POST /api/model-library/runtime/{load,unload}` |
| 계정 연동 | `GET /api/oauth/providers`, `POST /api/oauth/providers/:id`, `GET /api/oauth/:id/start`, `GET /api/oauth/:id/callback`, `DELETE /api/oauth/:id` |
| 기타 | `GET /api/audit`, `POST /api/translate` |

## 엔진 동작

1. 사용자 메시지를 SQLite에 저장
2. 제공자(OpenAI 호환)로 `stream: true` 요청
3. `content` → `text`, `reasoning_content` → `reasoning` 이벤트로 즉시 전달
4. `tool_calls` 가 오면:
   - 승인이 필요한 도구는 `/api/approvals` 로 사용자 결정을 기다림 (120초)
   - Rust에서 직접 실행 (`workspace_*`, `file_*`, `shell_exec`, `terminal_exec`, `http_fetch`, `web_search`, `memory_*`, `skill_propose`)
   - 결과를 `tool` 메시지로 모델에 되돌려 다음 라운드 실행 (최대 8라운드)
5. 최종 답변을 저장하고 `assistant_saved` + `done` 전송

도구 실행은 모두 감사 로그(`audit`)에 기록됩니다.

### 로컬 모델

`llama-server`(llama.cpp)를 Rust가 직접 관리합니다. 모델 로드 시
`127.0.0.1:4319` 에 서버를 띄우고 `dominion-native` 제공자를 자동 등록하므로,
채팅 엔진은 원격 제공자와 동일한 경로로 로컬 모델을 사용합니다. 언로드하면
프로세스 그룹을 종료하고 제공자를 제거합니다.

### 추론 언어

`reasoningLanguage: "ko"` 이면 시스템 프롬프트로 한국어 사고를 지시하고,
모델이 영어로 사고하는 경우(예: DeepSeek) 추론 텍스트를 한국어로 번역해
저장합니다. GLM-5.3 / Kimi-K3.1-Pro는 처음부터 한국어로 사고합니다.

### 자율 목표 하네스

`/goal` 은 단순 반복이 아니라 다음 단계를 돕니다.

1. **plan** — 목표를 3~5단계 계획으로 분해
2. **execute** — 실제 Rust 도구로 단계 수행
3. **review** — 별도 모델 호출이 도구 증거만 보고 `{complete, gaps}` 판정
4. **repair** — 미완료면 지적된 gaps 를 넣어 다시 실행
5. **evidence gate** — 행동형 목표인데 성공한 도구 호출이 없거나, 목표에 적힌
   파일이 실제로 없으면 `completed` 로 만들지 않고 `needs_review` 로 남깁니다.

#### 장기기억과 재학습

턴이 끝나면 `autoLearning`(기본 켜짐)에 따라 `learn` 작업이 큐에 들어갑니다.

1. 최근 30개 메시지와 실제 도구 실행 결과를 읽음
2. 모델에게 **JSON만** 요구: `{memories:[{text,type,importance,scope}], skill:{...}|null}`
3. 중복을 정규화해 제거하고 최대 8개를 `memory` 레코드로 저장
4. 재사용 가능한 절차가 있으면 `skill`을 **draft(비활성)** 로 저장
5. 다음 턴부터 관련 기억·활성 스킬이 시스템 프롬프트에 자동 주입됨

기억은 `type`(fact/preference/decision/procedure/lesson)과 `importance`로
점수화해 검색하고, 도구별 성공/실패 통계(`tool_stat`)도 함께 넣어
실패가 잦은 도구를 피하도록 안내합니다.

모델 제공자는 **OpenAI 호환 엔드포인트만** 사용합니다. Anthropic 네이티브
형식은 엔진에서 제거했습니다.

## 보안

- 쓰기 요청은 `Origin` 이 `http://127.0.0.1:<port>` / `http://localhost:<port>`
  일 때만 허용합니다. 외부 웹페이지의 로컬 API 호출은 403 입니다.
- `DOMINION_ADMIN_TOKEN` 을 설정하면 모든 쓰기 요청에
  `X-Dominion-Token` 헤더가 필요합니다.
- CORS 는 permissive 가 아니라 위 두 origin 만 허용합니다.

### 작업 큐

`POST /api/workflows/:id/run` 과 `POST /api/rooms/:id/run` 은 job 레코드를
`queued` 로 저장하고 Rust 워커가 순서대로 실행합니다.

- **워크플로**: 단계마다 실제 엔진 턴을 실행하고 `{{input}}`/`{{previous}}` 를
  치환합니다. 진행 상황은 `stepIndex`/`stepTotal` 로 job에 저장되며,
  실패하면 `status: failed` + `error` 로 남고 성공으로 표시하지 않습니다.
- **회의실**: 참여 에이전트가 `rounds` 만큼 발언하며 직전 발언을 보고
  이어갑니다. 모든 발언은 하나의 세션에 저장됩니다.
- **자율 목표**: `/goal` 로 등록한 목표를 최대 N회 반복하며 실제 도구로
  수행합니다. 모델이 `DONE` 을 선언하면 완료, 아니면 `needs_review` 로
  남깁니다.
- **자동화**: 30초마다 `nextAt` 이 지난 automation을 큐에 넣습니다.
- **복구**: 부팅 시 `running` job 은 `queued` 로 되돌리고, 3회 이상 실패한
  job 은 `needs_review`, `running` execution 은 `interrupted` 로 회수합니다.

### 실행 제한

한 턴은 최대 8라운드, 도구 40회, 10분입니다. 실제 스트림 `usage` 를 누적해
`execution.tokens` 에 기록합니다. 같은 세션에 동시에 두 턴이 들어오면
`Session is already running` 으로 거부합니다.

### 실행 기록과 취소

모든 엔진 턴은 `execution` 레코드를 만들고 모델 호출 수·도구 호출 수·
반복 횟수를 기록합니다. 도구 호출은 `execution_operation` 으로 남습니다.

`POST /api/sessions/:id/cancel` 은 스트리밍 중에도 다음 청크에서 중단하고
`execution.status = cancelled` 로 기록합니다. `POST /api/executions/:id/cancel`
은 해당 세션을 중단시킵니다.

### MCP

파일·셸·git·메모리는 **Rust 내장 도구**가 담당하므로 npm 기반 MCP preset을
기본 제공하지 않습니다. 외부 MCP 서버를 직접 등록했을 때만 해당 프로세스가
뜨고, DOMINION 엔진 자체는 Rust로 유지됩니다.

활성화된 MCP 서버(`kind: mcp`)의 도구를 `mcp__{server}__{tool}` 이름으로
모델 도구 목록에 합칩니다. stdio 서버는 자식 프로세스로, HTTP 서버는
JSON-RPC(스트리밍 응답 포함)로 호출합니다. MCP 호출은 기본적으로 승인
대기이며, `policy.autoApproveMcp` 로 자동 승인할 수 있습니다.

### 장기기억과 재학습

턴이 끝나면 `autoLearning`(기본 켜짐)에 따라 `learn` 작업이 큐에 들어갑니다.

1. 최근 30개 메시지와 실제 도구 실행 결과를 읽음
2. 모델에게 **JSON만** 요구: `{memories:[{text,type,importance,scope}], skill:{...}|null}`
3. 중복을 정규화해 제거하고 최대 8개를 `memory` 레코드로 저장
4. 재사용 가능한 절차가 있으면 `skill`을 **draft(비활성)** 로 저장
5. 다음 턴부터 관련 기억·활성 스킬이 시스템 프롬프트에 자동 주입됨

기억은 `type`(fact/preference/decision/procedure/lesson)과 `importance`로
점수화해 검색하고, 도구별 성공/실패 통계(`tool_stat`)도 함께 넣어
실패가 잦은 도구를 피하도록 안내합니다.

모델 제공자는 **OpenAI 호환 엔드포인트만** 사용합니다. Anthropic 네이티브
형식은 엔진에서 제거했습니다.

## 보안

- API 키는 레코드 JSON에 저장하지 않습니다.
- `runtime/secret.key` (0600) 로 ChaCha20-Poly1305 암호화 후 `secrets` 테이블에 저장합니다.
- 서버는 `127.0.0.1` 에만 바인딩합니다.
- 워크스페이스 도구는 `..` 및 심볼릭 링크 이탈을 거부합니다.
- 파괴적 도구(`*_write`, `shell_exec`, `terminal_exec`)는 기본적으로 승인 대기입니다.

## 아직 이식되지 않은 부분

Node 구현의 다음 기능은 Rust로 옮기지 않았습니다. 해당 경로는 HTML 대신 명시적인 JSON 오류(`501`, `ported: false`)를 반환합니다.

- MLX 런타임 (llama.cpp GGUF만 지원)
- 다운로드 일시정지/재개 (시작·취소·재시작은 지원)
- OAuth 실제 동의/토큰 발급 검증 — 사용자의 GitHub/Google 앱 자격 증명 필요 (구현·state 검증·토큰 교환 경로는 완료)
- 메신저 채널 어댑터 (Slack/Discord/Telegram) — 봇 토큰 필요
- 동영상 생성 파이프라인
- 설치 파일 자동 적용 (데스크톱 셸의 설치 기능 필요)

## 검증

```bash
cargo test --release
```

- `tests/e2e.rs` 는 실제 바이너리 + 가짜 OpenAI 호환 서버를 띄워
  SSE 채팅, SQLite 영속화, 토큰 집계, Origin 차단, 동시 세션 거부를 검증합니다.

- 볼트 암호화/변조 거부
- 레코드 CRUD 및 클라이언트 필드(`id`, `createdAt`, `updatedAt`)
- 메시지 추론/도구 호출 보존
- 워크스페이스 경로 이탈 차단
- 도구 정의/승인 플래그
- 워크플로 프롬프트 치환
- 작업 큐 등록/실패 기록
- MCP 이름 네임스페이스 및 정책 기반 승인
- 실행 기록/취소 상태 전이
- 자율 목표 실패 경로
- 개발 스튜디오 설정 저장 및 패키지 경로 해석
- 스킬 카탈로그 스캔/가져오기
- 컨텍스트 압축 요약과 히스토리 대체
- 로컬 GGUF 설치/가져오기/언로드
- OAuth state 만료/재사용 차단, 자격 증명 암호화
