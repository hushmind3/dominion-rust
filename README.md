# dominion-rust

DOMINION을 **Rust 백엔드 + Rust 실행 엔진**으로 다시 구현한 저장소입니다.

- 프론트엔드: 기존 React UI 그대로 (`web/`)
- 백엔드: Rust (axum)
- 실행 엔진: Rust (모델 스트리밍, 추론 토큰, 도구 호출 루프, 승인 게이트)
- 저장소: SQLite (WAL), 레코드는 JSON 문서
- Node.js: 런타임이 아니라 **빌드 도구로만** 사용 (`vite build`)

## 그룹 대화와 내 에이전트

`그룹 대화 · 회의` 화면에서 주인이 대화에 직접 참여하고, 에이전트들이 같은 기록을 읽으며
서로 의견·검토·잡담을 이어갑니다. 예전처럼 에이전트마다 가짜 사용자 메시지를 넣지 않습니다.
전체 토론·순번 발언·멘션 응답을 고르고, 사용자 개입이나 중지로 대화 흐름을 바꿀 수 있습니다.

에이전트 화면의 `기존 페르소나 가져오기`는 사용자의 로컬 9개 프로필에서 SOUL·아이콘만
읽어 DOMINION 프로필로 만듭니다. 원문·이미지는 `runtime/`에 보관하며 기존 모델 설정과
사용자가 수정한 프로필을 보존합니다. 다른 프로젝트의 실행 엔진·토큰·스킬을 가져오지 않습니다.

도구를 허용한 대화방에서는 실제 Rust 파일·셸·HTTP 도구를 사용합니다. 작업 큐·진행·승인·
실행 증거는 Rust와 SQLite에 남으므로 브라우저를 닫아도 실행은 계속됩니다.

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
    learning.rs      원문 증거 검증·학습 스냅샷·체크포인트
    memory.rs        에이전트/채널 격리·한국어 검색·정정 이력
    discord.rs       자체 Discord REST 수신·라우팅·전송 기록
    personas.rs      사용자 소유 페르소나·아이콘 데이터 이식
    llm.rs           계획/검토/학습용 비스트리밍 호출
    workflows.rs     순차 워크플로 실행 ({{input}} / {{previous}})
    rooms.rs         멀티 에이전트 라운드테이블 실행
    goals.rs         자율 목표 하네스 (계획·실행·증거 검증·수정)
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

## 런타임 구성

```
React (Vite build) → Rust Axum :4317 → Rust engine → SQLite / tools
                                            ↘ 모델 API (HTTPS 직접 연결)
                                            ↘ llama.cpp :4319 (로컬 GGUF)
```

- `4317`: DOMINION Rust 엔진·API
- `4319`: Rust가 관리하는 llama.cpp (로컬 모델을 쓸 때만)
- 모델 요청은 별도 게이트웨이 프로세스 없이 제공자 API로 직접 나갑니다.
- Node는 `vite build`용 개발 도구로만 사용합니다.

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

그룹 대화는 `GET/POST /api/rooms/:id/messages`, `GET /api/rooms/:id/state`,
`POST /api/rooms/:id/stop`을 사용합니다. 메시지와 작업은 함께 저장되며 `clientMessageId`로
재전송 중복을 막습니다. `POST /api/rooms/:id/run`도 같은 사용자 메시지 경로를 사용합니다.

프론트 개발 명령도 Rust를 사용합니다.

```bash
cd web
npm run dev:server   # Rust 백엔드 (cargo run)
npm run dev          # 별도 터미널에서 Vite 개발 서버
npm run check        # TypeScript + Rust 테스트 + Vite 빌드
npm start            # release Rust 엔진과 화면 실행
```

`npm start`, `dev:server`, `gate:engine`은 Node 서버나 옛 JavaScript 엔진을 실행하지 않습니다.

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

### 로컬 모델 백엔드 (GGUF + MLX)

맥에서는 두 가지 로컬 추론을 모두 지원합니다. **Python은 쓰지 않습니다.**

| 형식 | 런타임 | 선택 방법 |
| --- | --- | --- |
| GGUF | llama.cpp (`llama-server`) | 모델 라이브러리의 `*.gguf` |
| MLX | LM Studio 네이티브 MLX (`lms`) | LM Studio에 설치된 모델 (`lms:<modelKey>`) |

- MLX 모델은 LM Studio의 `lms ls --json`에서 자동으로 목록에 합쳐집니다.
- MLX 로드: `lms server start` → `lms load <key> --gpu max` → `127.0.0.1:1234/v1` 제공자 등록
- GGUF 로드: `llama-server`를 4319에 띄우고 `dominion-native` 제공자 등록
- 언로드 시 MLX는 `lms unload`, GGUF는 프로세스 그룹 종료
- `runtime.capabilities.mlx.available` / `gguf.available`로 현재 백엔드 확인

### 로컬 모델

`llama-server`(llama.cpp)를 Rust가 직접 관리합니다. 모델 로드 시
`127.0.0.1:4319` 에 서버를 띄우고 `dominion-native` 제공자를 자동 등록하므로,
채팅 엔진은 원격 제공자와 동일한 경로로 로컬 모델을 사용합니다. 언로드하면
프로세스 그룹을 종료하고 제공자를 제거합니다.

### 추론 언어

`reasoningLanguage: "ko"` 이면 시스템 프롬프트로 한국어 사고를 지시하고,
모델이 영어로 사고하는 경우(예: DeepSeek) 추론 텍스트를 한국어로 번역해
저장합니다. GLM-5.3 / Kimi-K3.1-Pro는 처음부터 한국어로 사고합니다.

### 자체 기억·실행 하네스

외부 하네스의 코드나 엔진을 가져오지 않습니다. 실행·기억·학습·회의·채널 드라이버는
이 저장소의 Rust 모듈이며, 기존 Rust 라이브러리 외에 새 의존성은 추가하지 않았습니다.

- **장기기억**: 에이전트와 채널 연결/서버/대화방을 분리합니다. 로컬 개인 기억은
  Discord에 자동 공개하지 않습니다. 한국어 조사·띄어쓰기에 대응하는 자체 검색을 사용합니다.
- **학습**: 성공한 턴의 고정된 메시지 스냅샷만 처리합니다. 사용자 원문 또는 실제 성공한
  도구 결과의 메시지 ID·정확한 인용을 검증합니다. 모델의 주장은 사실로 승격하지 않습니다.
  정정 시 이전 기억은 보존하고 새 값으로 대체하며, 학습 위치를 저장해 재처리를 막습니다.
- **스킬**: 자동 추출 절차는 비활성 `draft`로 저장합니다. 사용자가 검토 후 활성화합니다.
- **목표**: plan → execute → review → repair를 수행하며, Rust가 파일 상태·실제 도구
  결과를 따로 검증합니다. 읽기 도구나 실패한 셸 명령만으로 행동형 목표를 완료할 수 없습니다.
- **회의·워크플로**: 에이전트별 화자와 단계 커서를 보존합니다. 실제 모의 모델 서버로
  9명 에이전트의 독립 발언과 완료된 발언의 재실행 방지를 검증합니다.
- **복구**: 실행 중이던 외부 작업이나 전송의 결과가 불확실하면 `needs_review`로 남겨
  중복 실행을 막습니다. 안전하게 완료된 단계부터만 복구합니다.

여기서 재학습은 **기억·재사용 절차 개선**입니다. 모델 가중치를 훈련하는 기능은 아닙니다.
상세한 계약·검증 조건·Discord 연결 방법은 [ENGINE.md](ENGINE.md)를 참고하세요.

### 런타임 구성

```
React (Vite build) → Rust Axum :4317 → Rust engine → SQLite / tools
                                            ↘ 모델 API (HTTPS 직접 연결)
                                            ↘ llama.cpp :4319 (로컬 GGUF)
```

- `4317`: DOMINION Rust 엔진·API
- `4319`: Rust가 관리하는 llama.cpp (로컬 모델을 쓸 때만)
- 모델 요청은 별도 게이트웨이 프로세스 없이 제공자 API로 직접 나갑니다.
- Node는 `vite build`용 개발 도구로만 사용합니다.

## 실행 제한·취소

한 턴은 최대 8라운드·40회 도구 호출·10분입니다. 제한에 도달하면 실패로 기록합니다.
셸의 0이 아닌 종료 코드와 MCP `isError`도 실패로 기록하고, 도구 인수·결과를 보존합니다.
OpenAI 호환 네이티브 도구 호출과 텍스트 도구 프로토콜(`toolMode: compat`)을 지원합니다.

같은 세션의 동시 턴은 거부합니다. 취소는 모델 응답이나 승인 대기 중에도 작동합니다.
`POST /api/sessions/:id/cancel`, `POST /api/executions/:id/cancel`을 사용합니다.

### 작업 큐

작업은 SQLite에 먼저 저장한 뒤 원자적으로 선점합니다. 기본 4개 워커이며
`DOMINION_JOB_WORKERS=1..16`으로 설정할 수 있습니다. 목표 검토가 필요한 결과를
부모 job에서 `completed`로 덮어쓰지 않습니다.

- `POST /api/workflows/:id/run` — 정의와 단계별 결과를 고정하여 순서대로 실행
- `POST /api/rooms/:id/run` — 참여 에이전트가 화자를 구분하여 발언
- `POST /api/goals` — 증거 검증을 포함한 목표 실행
- automation — 30초마다 예정된 작업을 큐에 등록

### Discord

`연결` 화면에서 Bot Token·서버 ID·채널 ID와 에이전트 또는 최대 9명 회의실을 지정합니다.
저장하고 활성화하면 자체 Rust 드라이버가 Discord 공식 REST v10을 약 5초마다 조회합니다.
토큰은 암호화되며 API 응답에서 제외됩니다. 기존 외부 하네스 설정은 자동 가동하지 않습니다.

외부 봇과 대화하려면 허용 봇 ID를 지정하고 DOMINION 봇을 멘션해야 합니다.
봇 응답은 연결별 분당 2회로 제한하며 봇 메시지의 도구 실행·사용자 기억 학습은 차단합니다.
사람의 도구 실행은 명시적 옵션과 허용 사용자 ID가 있어야 가능합니다.
실제 Discord 연결에는 사용자의 봇 토큰과 서버 권한이 필요합니다.

### MCP

파일·셸·git·메모리는 **Rust 내장 도구**가 담당하므로 npm 기반 MCP preset을
기본 제공하지 않습니다. 외부 MCP 서버를 직접 등록했을 때만 해당 프로세스가
뜨고, DOMINION 엔진 자체는 Rust로 유지됩니다.

활성화된 MCP 서버(`kind: mcp`)의 도구를 `mcp__{server}__{tool}` 이름으로
모델 도구 목록에 합칩니다. stdio 서버는 자식 프로세스로, HTTP 서버는
JSON-RPC(스트리밍 응답 포함)로 호출합니다. MCP 호출은 기본적으로 승인
대기이며, `policy.autoApproveMcp` 로 자동 승인할 수 있습니다.

## 자기개선 루프

모든 일반 대화는 다음 루프에 들어갑니다 (`autoLearning` 기본 켜짐).

```
대화/지시 → 실제 도구 실행 → 결과·실패 분석
→ 기억/스킬 + 실행 전략 + 도구 정책 후보 생성
→ 결정적 검증 → 채택 → 다음 턴 프롬프트에 주입
→ 5회 관찰 → 기준 성공률보다 나쁘면 자동 롤백
```

- `improvement` 레코드: `strategy` / `tool_policy` / `harness`
- `strategy`·`tool_policy`는 증거와 통계 검증을 통과하면 `adopted`
- 채택 후 5회 사용에서 성공률이 기준보다 0.2 이상 낮으면 `rolled_back`
- 공유(`shared`)와 에이전트 전용(`agent`) 스코프를 구분
- `harness`(하네스 코드 변경)는 `proposed`로만 저장하며 주인 승인 없이는 적용하지 않음
- API: `GET /api/improvements`, `POST /api/improvements/:id/{approve,reject}`

### 경험 기반 교정 반영

주인이 "아니 그게 아니라", "다시 해", "틀렸어"처럼 이전 답을 교정하면 단순히 그 문장을
기억에 저장하지 않습니다. `reflect` job이 직전 요청·대화·실제 도구 기록을 다시 읽고
**근본 원인·기대·실제**를 분석한 뒤 다음을 갱신합니다.

- **기억**: 무엇을 잘못 이해했는지 `lesson` 기억으로 저장
- **경험**: 같은 작업 서명의 실패 사례로 기록 (`lesson`, `avoid`, 사용 도구, 증거 ID)
- **스킬**: 절차 수정안을 `draft` 스킬로 생성 (자동 활성화하지 않음)
- **전략**: 검증을 통과하면 `adopted`, 5회 관찰 후 나쁘면 자동 롤백
- **협업**: 경험은 `shared` 또는 `agent` 스코프로 저장되어 다른 에이전트가 검색

경험은 자유 서술 기억과 분리된 **사례 은행**입니다. 새 턴의 요청과 토큰이 겹치는
사례만 검색해 시스템 프롬프트에 `[failure]` 경고와 `avoid`로 주입하고, 사용 횟수를
기록합니다. 같은 서명·결과의 사례는 중복 저장하지 않고 `occurrences`만 올리며,
`learn` job이 오래된 중복과 300개 초과 사례를 `archived`로 정리합니다.

```
POST /api/chat {"input":"아니 그게 아니라 ...","autoLearning":true}
→ reflect job → experience + strategy + memory + skill draft
GET /api/experience
```

교정 턴 자체가 실패해도 직전 답변이 있으면 `reflect`는 큐에 들어갑니다. 실패도
추가 증거이기 때문입니다. 교정 문장만 있고 이전 답변이 없으면 첫 요청으로 간주해
반성하지 않습니다. 모델 가중치는 바뀌지 않습니다.

### 학습 전용 모델과 상시 기억

- 대화 모델이 작아도 학습·반성은 `learning: true` 제공자를 우선 사용합니다.
  (예: 1.7B 대화 + 4B 학습 교사) 대화 자체는 계속 같은 모델입니다.
- 로컬 모델은 4k 컨텍스트에 맞춰 최근 약 1,000자만 학습 입력으로 보냅니다.
- 로컬 비스트리밍 호출은 한 번에 하나만 실행하고, 빈 응답·컨텍스트 초과 시
  더 큰 로컬 모델 → 설정된 제공자 순으로 폴백합니다.
- `preference`/`procedure` 타입이고 importance가 0.7 이상인 기억은 질문과 단어가
  겹치지 않아도 매 턴 주입됩니다. "앞으로 답변 끝에 X를 붙여" 같은 상시 규칙이
  다음 세션에서도 적용됩니다.
- 교정에서 나온 기억·경험·전략은 `global` 스코프로 저장되어 일반 채팅, 회의실,
  외부 채널 사이에도 전파됩니다. 전역 경험은 원문 대신 작업 서명을 저장합니다.
- 전략은 실제로 프롬프트에 주입된 실행이고 도구 수행이 있었을 때만 검증
  카운터에 포함됩니다. 도구 없이 `completed`만 기록해서는 `validated`가 되지
  않습니다.
- 대화창에는 학습 상세를 띄우지 않고 "학습 기록이 갱신되었습니다" 한 줄만 표시합니다.
  상세 내용은 기억 화면에서 확인합니다.

### 절차 준수 검증

학습된 전략은 자연어 설명과 기계 검증 가능한 절차를 함께 저장할 수 있습니다.
절차는 `appliesTo`, `before.when/require`, `after`의 파일·도구 조건으로 구성되며
특정 파일명에 의존하지 않습니다.

- 대상 도구 호출 전에 선행 조건을 확인하고, 위반 시 도구를 실행하지 않고 수리
  메시지를 반환합니다.
- 최종 답변 전에 후속 조건을 다시 검사하고, 실패하면 최대 2회 수리 턴을
  요구합니다.
- 실행 기록에 적용 여부·검증 여부·준수 여부·위반 내용을 남깁니다.
- `unverified` 전략은 검증 카운터에 포함되지 않으며 `validated`가 될 수 없습니다.
- 조건이 하나라도 잘못된 절차는 일부만 채택하지 않고 revision 전체를 `invalid`로
  거부합니다.
- `before`가 전혀 발동하지 않은 실행은 `not_triggered`로 기록하며 준수 성공으로
  세지 않습니다.
- owner 교정에서 나온 `standing` 전략의 위반은 2회 수리 후에도 남으면 완료가
  아니라 실패로 끝냅니다. 학습 후보(`adaptive`)의 미준수는 작업 성공과 분리해
  기록합니다. standing은 `authorKind: owner` 교정에만 부여합니다.
- 경로 조건은 정확 일치로 비교하며, symlink·workspace 밖 경로·2MiB 초과 파일은
  검사에서 거부합니다.
- 실행 종료는 finalization marker와 실행별 멱등 카운터로 기록해, 재시도·crash
  후에도 통계가 한 번만 반영됩니다.

### 작업 성능 벤치마크

`scripts/task-benchmark.sh`는 같은 모델과 에이전트로 학습 전/후 작업 성능을
측정합니다. 단순히 대화가 성공했는지가 아니라, 교정으로 배운 절차가 새 세션과
다른 에이전트에서도 지켜지는지를 파일 내용으로 검사합니다.

```bash
BENCH_ROOT=/tmp/dominion-bench-$(uuidgen) \
DOMINION_BASE=http://127.0.0.1:4401 \
BENCH_PROVIDER="Ollama Qwen3 4B" BENCH_ROUNDS=3 scripts/task-benchmark.sh
```

- `BENCH_ROOT`에는 `benchmark`가 적힌 `.dominion-benchmark-sentinel`이 있어야 합니다.
- `DOMINION_BASE`가 없거나 라이브 `4317`이면 실행을 거부합니다.
- 저장소 루트나 운영 workspace를 `BENCH_ROOT`로 쓰면 거부합니다.
- 승인은 `workspace_write` 등 fixture 도구에만 범위 한정하며, 전역
  `autoApproveAll`을 켜지 않습니다.
- 서버 identity·data/workspace handshake와 Rust 실험 runner는 아직 미구현입니다.

- Baseline: 규칙 없이 `bench/rule.txt` 생성
- 교정 학습: 작업 전 `bench/audit.txt`에 `before:rule.txt`를 쓰도록 교정
- Post: 새 세션에서 같은 작업을 3회 반복
- Cross-agent: 학습을 공유받은 다른 에이전트가 같은 규칙을 따르는지 확인
- 환경변수: `DOMINION_BASE`, `BENCH_PROVIDER`, `BENCH_ROUNDS`
- `bench/`는 gitignore 대상이며, 벤치마크는 작업공간에만 파일을 만듭니다.

결과는 `baseline`, `post N/N`, `cross` pass로 출력됩니다. 반성 job이 끝난 뒤
측정해야 하므로 스크립트는 최대 300초를 기다립니다.

### 약한 모델(4B) 지원

- 네이티브 function calling이 없어도 `<tool_call>`, bare JSON, fenced JSON,
  문자열 인자, function 래퍼를 파싱
- 인자 JSON 오류는 턴 실패가 아니라 도구 오류로 되돌려 재시도
- 중복 tool_call ID는 새 ID로 교정
- 같은 호출 3회 반복 시 중단하고 최종 답변 유도
- 로컬 제공자는 기본 `compat` 모드, 관련 도구 10개만 압축 안내

## 멀티에이전트 위임

일반 대화의 에이전트는 다른 에이전트에게 작업을 넘길 수 있습니다.

- `delegate_task` — 대상 에이전트/지시/대기 여부를 받아 **실제 자식 세션과 durable job** 생성
- `task_status` — 진행 상태 확인
- `task_result` — 완료된 자식의 최종 답변과 성공한 도구 결과 확인
- 자식 작업은 워커 큐를 기다리지 않고 `start_now`로 즉시 실행되어,
  부모가 기다리는 동안 워커 풀이 고갈되지 않습니다.
- 부모가 중단되어도 자식 작업은 백그라운드에서 계속되고 기록이 남습니다.

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
- Slack/Telegram 등 Discord 이외의 메신저 어댑터 (카탈로그는 유지, 실행 드라이버 미구현)
- 동영상 생성 파이프라인
- 설치 파일 자동 적용 (데스크톱 셸의 설치 기능 필요)

## 검증

```bash
cargo test --release
```

- `tests/e2e.rs` 는 실제 바이너리 + 가짜 OpenAI 호환 서버를 띄워
  SSE 채팅, SQLite 영속화, 토큰 집계, Origin 차단, 동시 세션 거부,
  명령 실패 증거·텍스트 도구 프로토콜·반복 제한·대기 중 취소·도구 비활성화를 검증합니다.

- 학습은 모의 모델로 출처·스냅샷·중복 방지·정정·실패 시 체크포인트 보존을 검증합니다.
- 경험 은행은 중복 병합, 유사 작업 검색, 에이전트 스코프 격리, 300개 초과 정리를 검증합니다.
- 교정 반성은 이전 답변이 있을 때만 트리거되고, 턴이 실패해도 `reflect` job이 남는지 검증합니다.
- 목표는 실제 파일 변경과 거짓 완료 차단, 회의는 9명 독립 발언·재실행 방지를 검증합니다.
- Discord는 모의 HTTP 서버로 인증·메시지 순서·전송 정책·취소·복구·토큰 비공개를 검증합니다.

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

## 라이선스

Copyright 2026 hushmind3.

DOMINION은 [Apache License, Version 2.0](LICENSE)에 따라 배포됩니다. 외부 라이브러리·구성요소에는 각각의 라이선스가 적용됩니다.

## 2026-11-13 서브2: 실제 개선기 계승·재수정 폐회로 (소스 구현)

**실행 경로:** `src/api/rsi.rs` (`jobs::enqueue`) → `src/engine/jobs.rs` (`rsi_evolve::run`) → `src/engine/rsi/execution.rs::run_loop` → 새 커밋으로 빌드된 `rsi/worker.rs::worker` → `rsi/planner/compiled.rs::compiled_system_generation_plan` → 실제 모델/도구 편집 → `rsi_worktree/{candidate,comparison}.rs`의 동결 평가 → `rsi/lineage.rs::adopt` → 다음 커밋·바이너리·실행기.

**새로 연결한 후손 개선기 피드백:**

1. `rsi_system/history.rs::revalidated_method_history`는 후보의 원본 paired 영수증과 지표를 재생해 신뢰할 수 없는 저장 성적을 폐기합니다.
2. `rsi_system/method_lineage.rs::inherited_method_receipt`는 **실제로 호출되는 개선기 소스**의 채택 전·후 SHA와 새 후손의 현재 소스 SHA를 대조합니다. `rsi/worker.rs`에서 재컴파일된 개선기 계획과 실제 focus에 연결하며 `rsiMetaMethodInheritance` 실행 기록을 남깁니다.
3. 후손이 실제 작업·독립 평가를 거치면 `rsi_system::observed_method_outcome`이 측정·채택·추가 메타소스 수정 여부를 세대 원장에 보존합니다. 다음 세대에서는 동일 영수증을 **독립 재검증**하여 원장 변조로 인한 허위 학습을 제한합니다.
4. `rsi_meta_controller.rs::inherited_method_repair`는 같은 상속 소스·정책 revision에서 독립 평가된 실패 2회와 성공 0회를 관측하면 해당 개선기 소스 자체를 재수정 대상으로 올립니다. `rsi/planner/compiled.rs::evidence_based_meta_repair_focus`가 그 결정을 실제 source focus에 연결하며, 기존 동결 평가·프로덕션 적용은 별도 경계를 유지합니다.

**독립 증명 경계:** 새 루프는 소스·행동 계보 및 실패 후 개선 방법 **재설계 경로**를 구현한 것입니다. 실제 컴파일·실모델 후손 3세대의 연속적인 개선, 동등 예산 A/B 성과, 일반화 능력은 아직 실행 증거가 없으므로 완전한 RSI 성공으로 표시하지 않습니다. 모델 가중치 변경·LoRA·파인튜닝은 연결하지 않았습니다. 이 변경은 `dominion-sub2`에만 반영됐습니다.

구조·한계와 증거 경계는 `docs/research/SUB2-EXECUTABLE-RSI-BOUNDARIES-2026-11-13.md`에 정리했습니다.

## 2026-11-13 서브2: 채택 소스 원본 결속과 독립 후손 계수

- `rsi/execution.rs`: 실험 worktree 채택 직후 계산한 변경 소스 SHA-256을
  세대 요약과 별도로 **후보의 experimentAdoption 영수증**에도 보존합니다.
- `rsi_system/history.rs`: 동일 child job에서 생산된 원본 paired 후보만
  세대 평가로 재생하고, 후보 영수증과 세대 요약의 source SHA가 일치할 때에만
  `sourceLineageVerified`를 설정합니다.
- `rsi_system/method_lineage.rs`: 위 독립 채택 원본 결속 없이는 변경된
  개선 방법이 다음 컴파일된 후손에게 계승되었다고 인정하지 않습니다.
- `rsi_meta_controller.rs`: 계승된 개선 방법의 실패 후 자기수정은 동일
  job의 중복 기록을 1회로 계산합니다. 실제 독립 후손 2회 이상이 필요합니다.

예전 채택 후보에 새 source SHA 영수증이 없다면 과거 paired **평가 기록은
유지**하되 개선 방법의 소스 계승 증거로는 사용하지 않습니다. 이는
성능 개선 입증이 아니라 학습·계승 데이터의 출처 경계 강화입니다.
새 회귀 테스트는 작성했지만 지침에 따라 빌드·CI·테스트는 실행하지 않았습니다.

## 서브2 RSI 회귀 테스트 책임 분리

`src/engine/rsi_evolve.rs`의 `#[cfg(test)]` 진입점은 동일하며,
`src/engine/rsi/tests/evolve.rs`는 테스트 포함 파일을 순서대로 조립합니다.
실제 테스트 105개는 `src/engine/rsi/tests/evolve-*.rs`로 분리했습니다.
검색 전략, 타기팅, 연구·예산, 독립 평가와 채택, 후손 실행,
도구 수리, 학습 피드백, 방법 전이, 실험 대조군을 독립적으로 검토할 수 있습니다.
테스트 본문과 상대 경로 fixture는 변경하지 않았습니다.


## 2026-11-13 서브2: RSI 성공 판정 통합 · A→B→A 실제 실행 경로

**단일 성공 판정:** `src/engine/rsi/closure.rs::evidence` → `rsi::campaign_recursive_evidence`를
실제 캠페인 정지 조건(`rsi/execution.rs`)과 API(`api/rsi.rs::apply_capability`)가 공유합니다.
단순 3연속 채택, 메타 도메인 이전, 저장된 `capabilityDemonstrated` 플래그는 완성 판정이 아닙니다.

**재귀 개입 경로:** `rsi/planner/compiled.rs`가 검증된 B 후손 결과를
`rsi_system/return_feedback.rs::next_method_return`에 전달합니다. A에서 채택된
실제 개선 방법 소스 해시가 B 실행 원장에 상속되어 있고, B가 다른 영역에서
독립 평가상 개선을 얻은 경우에만 다음 C 계획의 `downstreamMethodFeedback`을 생성합니다.
현재 소스 인덱스가 일치하면 다음 C의 `metaPolicy`, source focus, 실행 정책,
모델 지시가 동일한 A 모듈의 재개선 실험으로 연결됩니다.
`rsi/worker.rs`는 원본 B 기록을 독립적으로 재생하고
`rsiMetaReturnExecuted`를 남깁니다. C가 실제 코드를 수정하고 외부 frozen
paired 평가에 통과·채택돼야 `closure::evidence`가 A→B→A를 인정합니다.

**원본 증거 체인:** A/B/C 각각의 후보·실행 job·바이너리 revision·소스 SHA
·원본 paired 평가를 재검증합니다. B가 A의 실제 메서드를 실행했고
B의 평가 영수증이 C의 편집 계획에 사용됐으며 C가 같은 메서드 소스를
다시 변경했는지 검사합니다. `rsi_worktree/validation.rs::classify`와
`rsi.rs::propose`는 동일한 실제 실행 메서드 소스 목록으로
improver 후보를 인정합니다. 증명기·원본 기록 재검증·frozen 평가 모듈은
모델 패치에서 제외합니다.

**근거·한계:** 신규 정적/회귀 테스트는 코드에 포함했습니다.
빌드·테스트·CI·실제 모델 캠페인은 지침대로 실행하지 않았습니다.
새 원장은 측정된 A→B→A를 *인정할 수 있는 구조*이며, 아직
3세대 실측 완성과 개선법 변경의 무작위 대조 인과효과를 입증한 것은 아닙니다.
`systemCapabilityDemonstrated`는 전체 지능 구성요소를 포괄하는 별도 엄격한 증명으로 유지합니다.

### 2026-11-13: A→B→A′→B′ 검증된 후손 재실행

서브2의 완료 게이트는 이제 **4개의 연속 독립 채택**을 요구합니다. A(개선기 소스 변경) → B(다른 능력에서 독립 평가 개선) → A′(B 평가 결과를 이용한 동일 개선기 소스 재수정) → B′(새 A′를 사용한 컴파일 후손이 B의 원래 능력을 다시 개선). `rsi_system/return_feedback.rs::next_capability_reapplication`이 마지막 B′의 소스·분야 선택을 실제 후손 계획으로 전달하고, `rsi/worker.rs`가 출처·상속·실제 소스 선택을 재확인합니다. `rsi/closure.rs::evidence`는 B′의 원본 후보·동결 평가까지 확인해야 `proven=true`를 반환하며, 동일한 증명 결과가 캠페인 중단과 API에 적용됩니다. 두 모듈은 `rsi_evolve.rs`, `rsi_system.rs`에서 직접 등록됩니다. 실모델 실행·빌드·테스트는 하지 않았고, 대조군에 의한 개선법의 독립적 인과 우위는 아직 증명하지 않았습니다.


### 2026-11-13 추가: 이전/현재 컴파일 개선기의 독립 대조 워커

4세대 증명은 이제 후손 B와 B′ 각각에 **소스-동일 바이너리 대조 작업**을 추가로 요구합니다. 직전 채택이 실행 가능한 개선 방법 소스만 바꾼 경우, 실행기는 이전 바이너리를 보존해 현재 HEAD 소스·동일 모델·동결 사례에서 별도 후손을 실행하고 평가합니다. 새 바이너리 후손도 같은 원본 소스에서 실행합니다. 대조 후보는 채택하지 않습니다. `src/engine/rsi/method_control.rs`가 두 원본 후보의 평가 영수증과 행동 변경을 다시 계산하며, 실제 실행군이 동결 품질에서 우위일 때만 `rsi/closure.rs`의 A→B→A′→B′ 성공 조건에 포함합니다. 자동 연속 전체 RSI에서 기본 활성화되며 `methodControl:false`로 추가 모델 호출을 끌 수 있습니다. 모델 샘플링 변동과 공유 연구 문맥의 영향은 여전히 별도 인과적 불확실성으로 기록합니다. 해당 구현에 대한 빌드·테스트·CI·실측 캠페인은 이 소스 작업 중 실행하지 않았습니다.

### 2026-11-13 — 서브2: 수정 가능한 실험 선택기 · 타입 계약 · 실행 책임 분리

RSI는 미리 정한 A→B→A′→B′ 순서의 재현이 아니라 **다음 실험을 설계하는 원리까지 재설계**해야 합니다. 일반 자동 전체 시스템 RSI의 후손은 `rsi/planner/operator_policy.rs::select`에서 모델 작성 프로그램·새 개선 원리·계승 프로그램·메타 제어 수정·아키텍처 탐색을 동일한 성과 인터페이스로 비교합니다. `rsi_system/history.rs`가 원본 작업 및 독립 평가를 검증한 결과만 선택기의 재학습에 쓰며, 선택 정책 자체도 편집 가능한 후손 소스입니다. A→B→A′→B′ 판정은 완성 증명용이지 일상 연구 단계의 스케줄러가 아닙니다.

`rsi/contract.rs::ExperimentContract`는 캠페인·소스 커밋/트리·대상 파일별 SHA·개선법 리비전·동결 평가 지문을 결합한 버전 타입 계약입니다. 후손 계획 → 실행기 → 워커 → 증명기가 이 계약을 공유합니다. 대형 조정기의 컴파일·생성기 계보를 `rsi/execution/generator.rs`, 컴파일 계획 실행·검증을 `rsi/execution/compiled_plan.rs`, 별도 CLI 계획 실행을 `rsi/worker/planning.rs`로 분리했습니다.

구체적 진입점과 파일 지도: [RSI 실행 계약/한계](docs/architecture/RSI-EDITABLE-EXPERIMENT-ENGINE-2026-11-13.md). 모델 가중치와 독립 동결 평가기는 수정하지 않습니다. **이 소스 개편에서는 빌드·테스트·CI·실모델 실행을 하지 않았고 완성 판정도 주장하지 않습니다.**

### 2026-11-13 CI 검증 단계

`rsi_system/history.rs::replayed_experiment_operator`는 실제 후손이 목표를 선택한 것만으로 실험 보상을 계산하지 않습니다. 동일 작업에서 생성된 모델 편집 후보가 **바로 그 목표 소스**를 변경했고 원본 도구 기록에 성공적인 쓰기가 있으며 동결 평가가 재생되는 경우에만 선택기 학습 근거로 인정합니다. `rsi/contract.rs` 회귀 검증에는 원본 동결 사례 대체와 소스 인벤토리 밖의 목표 편집이 계약을 통과하지 못하는 사례도 포함했습니다. GitHub Actions `CI` 워크플로에서 Rust release tests 및 웹 typecheck/build를 실행하도록 일반 푸시 커밋을 생성합니다. 결과는 별도 실행 링크로 기록합니다.

### 2026-11-13: 원본 영수증의 계보 평가·증명 저장 분리

성장 중인 연속 RSI 실행기의 보상 결정을 다시 분리했습니다. `rsi/execution/learning_feedback.rs::assess`는 이미 수집한 독립 평가를 이용해 실사용 개선법 실행·실험 설계자·메타 결과를 평가하지만 Git 커밋을 바꾸지 않습니다. `rsi/execution/proof_ledger.rs::persist`만 완성된 `rsi_generation` 기록을 영속화하고 동일 폐회로 증명기로 캠페인/API의 성공 필드를 계산합니다. 운영 루프는 모델 실행·예산·재시도 조정만 합니다. 기존 동결 평가 원본과 파일별 해시 검증은 계속 유지합니다. 검증은 GitHub Actions CI 실행 로그를 기준으로 진행합니다.

### 2026-11-13: 새 지능 역할을 자동 등록하는 실험 포트폴리오

`rsi/planner/operator_policy.rs::select`는 이제 고정된 5종의 실험 제안자와 함께, 실제 재빌드된 코드 그래프 `architectureFrontier`에서 해시로 입증된 **모든 추가 지능 역할·소스 파일**에 대해 안정적인 `source_graph_*` 실험 후보를 자동 생성합니다. 역할 이름을 기존 enum에 추가할 필요가 없습니다. 실측 평가를 재생한 독립 성과와 퇴행 횟수만 다음 선택 우선순위에 반영합니다. 이것은 후손이 새 구성요소를 만들었을 때 실제 실험 대상으로 등장하도록 연결한 것이며, 아직 임의 도구 플러그인의 안전한 동적 실행·독립 평가 마이그레이션을 전부 제공한다는 의미는 아닙니다. 모델 가중치는 고정됩니다.

### 2026-11-13: 새 개선법 소스 계보 자동 등록

`rsi_system/method_lineage.rs::executable_method_source`는 실제 런타임에서 사용되는 `rsi/search.rs`, `rsi_edit_genome.rs`, `rsi_decision_rules.rs`, `rsi_open_program.rs` 등 개선법 소스를 포함하며, 새 후손이 만들고 실제 컴파일한 `rsi/planner/`, `rsi/learning/` 하위 소스도 안전 경로·런타임 파일 조건을 통과하면 개선법 수정 대상으로 인정합니다. `inherited_method_receipt`는 고정 소스 목록 대신 **독립 평가를 거쳐 채택된 시스템 설계의 실제 변경 파일들**에서 계보를 검증합니다. 원본 소스 SHA·채택 SHA·후손 블루프린트 SHA 검사는 유지되며, 동결된 `rsi/closure.rs`와 `rsi/contract.rs`는 개선법 대상에서 제외됩니다.


## 2026-11-14 · Sub2 공개 출시 후 엔진·하네스·알고리즘 통합 개편

이 절은 `dominion-sub2` 브랜치의 **소스 구조 변경**을 기록합니다. 실제 API 키 기반 생성·비용 절감률·CI 통과를 증명하는 기록은 아닙니다.

- **턴키 영상:** `src/api/video.rs`에서 암호화 API 키 및 엔진 가용성을 제공하고 `web/src/pages/Video.tsx`에서 LTX 2.5 Pro/Fast·720p~4K·화면비·길이를 설정합니다. `src/engine/video.rs::enqueue/run`의 기존 내구성 있는 비디오 작업 경로가 `src/engine/video/ltx.rs::render`의 **V2 비동기 POST → 원격 ID 보관 → GET 상태 조회 → 안전한 MP4 다운로드**로 연결됩니다. Pro 및 고해상도 Fast는 최대 10초, 720p/1080p Fast는 24fps 기준 최대 20초입니다. 시네마틱·제품 광고·소셜 숏폼·원본 그대로 연출 프리셋은 별도 모델 호출 없이 입력 프롬프트를 구성합니다. 기존 ComfyUI·FFmpeg 카드 생성 및 YouTube 업로드는 유지합니다.
- **그룹 대화 맥락:** `src/engine/chat/room_context.rs::compile`은 기존의 주인 지시·참가자별 응답·도구 실행 증거를 보존하면서 `chat/room_context/role_routing.rs`의 역할 연관도를 **질문과 연관된 증거에만** 추가 가중합니다. 모델 호출을 늘리지 않으며 예상 미선택 토큰 수는 `room_context.accounting`에 기록합니다.
- **사용자 선택 모델 계층:** `src/api/rooms.rs::validated_config`가 `tierRoutingEnabled/tierRoutes`를 등록 공급자 기준으로 검증·저장합니다. `src/engine/rooms/model_tiering.rs::select`는 복잡한 요청과 가벼운 요청을 구분하고 `rooms.rs::run_locked`에서 실제 모델·제공자·추론 강도에 반영합니다. `web/src/pages/Rooms.tsx`에서 명시적으로 켜고 **선택한 모델만** 사용합니다. 수동 모델 지정은 항상 우선합니다. 공급자 요금·성능을 자동 추정하거나 미등록 키를 쓰지 않습니다.
- **RSI의 전체 시스템 인식:** `src/engine/rsi_system.rs::source_role/architecture_role_domain`에 영상과 그룹 대화 실행 소스의 `video/multiagent` 역할을 연결했습니다. 이 소스는 재빌드된 후손의 `source_inventory → blueprint → architecture_frontier`에서 개선 후보로 검토될 수 있습니다. 기존 원본 paired 평가와 채택 게이트는 동일하며 새로운 품질 개선의 증명으로 간주하지 않습니다.
- **유지보수:** `src/engine/video.rs`를 `video/{native,comfy,ltx,upload}.rs`로 분리했습니다. `models.rs`는 `models/{installer,management,loader,catalog,download}.rs`, `algorithms.rs`는 `algorithms/{builtins,registry,feedback,management}.rs`, `chat/provider.rs`는 `chat/provider/{openai,adapters,codex,anthropic}.rs`로 실실행 기능을 나눴습니다. 원래 공개 함수 이름과 Rust 모듈 재내보내기(re-export)를 보존했습니다. 그룹 실행·프롬프트 컴파일·지식 추출·모델 공급자·모델 라이브러리·알고리즘·컨텍스트·방 상태의 **회귀 테스트는 각각 production 모듈 밖 `*/tests.rs`**로 분리했습니다. 분리된 파일은 `#[cfg(test)]`에서만 참조합니다.
- **실측 비용 계측:** `rooms/usage_ledger.rs`가 완료 메시지의 공급자 보고 토큰을 집계해 작업 원장과 `room.exchange_finished` 감사 로그에 저장합니다. 캐시 사용량이 누락되면 0으로 취급하지 않고 `null`로 남깁니다. 그룹 화면에는 **보고된 호출에 한해** 캐시 입력 재사용률을 표시합니다.

코드 호출도·편집 경계·2026 연구 및 남은 핵심 결함은 [Sub2 출시 후 통합 아키텍처](docs/architecture/RELEASE-INTEGRATION-SUB2-2026-11-14.md)에 있습니다.

> **검증 경계:** 이 변경 동안 테스트·컴파일·빌드·벤치마크·유료 공급자 호출·CI를 실행하지 않았습니다. LTX 출력 품질과 토큰/비용 절감은 실측되지 않았으며, 모델 가중치 변경·LoRA·파인튜닝을 RSI 실행 경로에 추가하지 않았습니다.

## 2026-11-14 · 서브2: 자율 에이전트 25점 심사 대응

**사용자 실제 진입점:** 홈 `web/src/components/GoalConsole.tsx` → `POST /api/goals` → `jobs.rs::enqueue("goal")` → `goals.rs::run` → 코드 인덱스 기반 프로젝트 조사·계획·실제 도구·독립 검토·재계획·검증·최종 실행 보고·산출물·종료 기록입니다. 기존 홈의 일반 `prompt` 실행과 별도이며, 기존 Provider/채팅/도구/작업 큐를 재사용합니다.

- `goals/project.rs`와 `goals/code_navigation.rs`: 로컬 매니페스트·프로젝트 규칙을 수집하고, **기존 영속 코드 인덱스**를 활용해 현재 목표에 적합한 파일·함수와 소스 SHA로 검증한 발췌문을 계획/실행 모델에 전달합니다. 12,000자 소스 지문 상한·비동기 인덱스 갱신을 적용하고 큰 발췌문은 첫 실행 턴 및 5턴 간격에만 다시 전송합니다. 테스트 명령은 제안만 하며 탐색 단계에서는 실행하지 않습니다.
- `goals/artifacts.rs`와 `goals/receipts.rs`: 실제 성공한 파일 도구 기록에서 파일 해시와 실행 단위 영수증을 만듭니다. 실패/중지 시에도 이미 성공한 도구 증거를 먼저 저장합니다.
- `goals/diff.rs`: 검증된 산출물 경로로 범위를 제한한 **읽기 전용 Git diff**를 사용자에게 보여줍니다. 신규 추적 외 파일은 파일 해시로 별도 표시합니다.
- `api/goals.rs::resume_goal/edit_plan`: 불확실한 외부 실행은 사용자 확인 뒤 새로운 작업으로 이어받고, 진행 중이 아닌 목표의 계획을 직접 고칠 수 있습니다. `api/jobs.rs::cancel`은 대기 중인 목표도 즉시 취소 상태로 전환합니다. 중단·복구·증거 경계는 원래 실행 기록과 분리합니다.
- `verification.rs::successful_evidence`: 위임 작업이 아직 진행 중이거나 실패했을 때 성공으로 세지 않습니다. `jobs/goal_status.rs`는 실패한 작업이 목표를 영원히 running으로 남기는 상태 불일치를 줄입니다. `api/goals.rs::goal_subtasks/goal_approvals`는 자기 목표와 하위 위임 세션의 실제 작업·승인만 화면에 노출하고 `GoalConsole`에서 도구 허용·거절을 직접 처리합니다.
- `goals.rs`: 기계 검증 실패 시 불필요한 별도 리뷰 모델 호출을 생략하고, 실행 모델의 세션 토큰 예산을 다음 턴 진입 전에 확인합니다. 리뷰 생략 횟수는 실행 영수증에 남깁니다.
- `rsi_system.rs` + `rsi_system/agent_feedback.rs`: 실행 중인 자율 목표 하네스를 `agent_harness` 개선 역할로 노출하고, 최근 실제 목표 시도의 완료·실패·토큰 사용·절약한 검토 호출 통계를 **재컴파일 후손 `architecture_frontier`의 `ownerTaskFeedback`**에 전달합니다. 후손이 우선 개선할 진짜 목표 실행 소스를 고르는 입력이며 일반 사용자 목표 영수증은 RSI 독립 평가의 성공 증명으로 사용하지 않습니다.

[**자율 에이전트 전체 호출도·책임 경계·2026 근거**](docs/architecture/AGENT-HARNESS-SUB2-2026-11-14.md)

> 심판의 52/100은 개편 이전 정적 점수입니다. 신규 빌드·테스트·실모델 실행·CI·비용 실측은 수행하지 않았으므로 개선 후 점수나 Claude Code·Codex 동등 성능을 주장하지 않습니다.

## 2026-11-14 · 서브2 재심사 이후: Git 격리·병렬 조사 직접 연결

심판 재심사 **57/100** 이후, 소스 수준에서 다음 실행 경로를 추가했습니다. 변경 후의 새 점수·성능 향상 수치를 주장하지 않습니다.

- **소스 격리:** 홈 `GoalConsole`에서 Git 격리 옵션을 켜면 `api/goals.rs::create_goal → goals.rs::run → goal_workspace.rs::prepare`가 공용 체크아웃의 깨끗한 Git HEAD로 데이터 폴더에 detached worktree를 준비합니다. `chat/runtime/dispatch.rs`의 도구 실행·효과 기록, `goals/project.rs`의 프로젝트 조사, `goals/code_navigation.rs`의 코드 인덱스, `verification.rs`의 수용 조건, `goals/artifacts.rs`의 파일 증거가 **동일한 목표 전용 폴더**를 사용합니다. 원본 파일은 목표 수정으로 덮어쓰지 않습니다.
- **검토/폐기:** `GET /api/goals/:id/diff`로 격리된 변경과 untracked 경로를 확인하고, 목표 종료 후 `POST /api/goals/:id/discard`로 사용자 확인을 거쳐 해당 worktree만 폐기합니다. 목표가 삭제되거나 폐기된 후 공용 폴더로 자동 복귀하지 않습니다. Git 격리는 OS 샌드박스가 아니며 승인된 셸·외부 명령이 다른 경로를 읽고 쓸 수 있습니다.
- **병렬 읽기 전용 조사:** `tools.rs::definitions/execute → tools/batch.rs::delegate_many`로 최대 4개 조사 작업을 실제 세션/작업 큐에 등록하고 `task_result`로 모읍니다. 읽기 전용 하위 세션은 도구 스키마·실제 native/MCP 디스패치 모두에서 변경 도구를 거부합니다. 하위 세션의 Git 경로는 부모 목표에서 계승합니다.
- **RSI로 피드백:** `rsi_system.rs::{COMPONENTS,source_role}`에 신설 하네스/도구 소스를 실제 개조 가능 영역으로 노출했습니다. 기존 독립 쌍대 평가·계보·채택 기준은 그대로입니다.

[실행 호출도·증거 경계·2026 근거·남은 문제](docs/architecture/AGENT-HARNESS-SUB2-REJUDGE-2026-11-14.md)

이전 커밋 `ffcb6f6`은 GitHub Actions에서 **Rust 942개 테스트 및 웹 타입 검사·빌드 통과** 기록이 있습니다. **이 절의 신규 코드에 대해 CI·컴파일·테스트·유료 모델 실험은 실행하지 않았습니다.**

### 2026-11-14 · 영상 MP4 기술 검수

`engine/video.rs::run`의 LTX 2.5·ComfyUI·기본 FFmpeg 생성 후처리에 `video/quality.rs::inspect`를 연결했습니다. 사용 가능한 `ffprobe`로 비디오/오디오 스트림·해상도·화면비·길이를 검수하고 작업 체크포인트 및 결과에 `technicalQuality`를 저장합니다. 요청과 다르면 `needs_review`로 종료하며 유료 API를 자동 재호출하지 않습니다. `ffprobe`가 없으면 `not_verified`로 표시합니다. **이 검사는 시각적 미학·프레임간 연속성·생성비용 최적화의 실측이나 보증이 아닙니다.** 신규 영상 경로에 대해 빌드·CI·실제 영상 생성은 실행하지 않았습니다.

### 2026-11-14 · 검토한 Git 패치의 원본 반영

격리 목표의 `GET /api/goals/:id/diff`는 **수정된 추적 파일과 새로 생성된 일반 텍스트 소스 파일을 함께 포함**한 전체 패치(최대 128KiB)의 `patchSha256`·`canApply`를 반환합니다. `goal_workspace.rs::patch_bytes`는 원본·격리 체크아웃의 실제 Git 인덱스에 `add`하지 않고 임시 인덱스를 만들며, 신규 소스는 `git add -N`(intent-to-add)로 그 인덱스에서만 검토 패치에 넣습니다. 경로는 64개·32KiB 목록·파일당 128KiB로 제한하고, 격리 경로 밖 파일·심볼릭 링크·바이너리·비 UTF-8 파일은 별도 Git/PR 검토 대상으로 거부합니다. 사용자는 홈에서 **새 파일의 전체 추가 내용까지** 확인하고, `POST /api/goals/:id/apply`에 검토한 SHA-256을 제출합니다. 적용 직전 Rust는 원본 HEAD 일치·원본 변경 없음·다른 실행 작업 없음·현재 패치 해시 일치·`git apply --check`를 재확인하며, 적용 후 신규 파일을 포함한 패치를 다시 구성해 SHA-256을 대조하고 `gitApply` 영수증을 남깁니다. 사용자 승인 없이 원본에 쓰지 않으며 자동 커밋·병합은 없습니다.

기본 `DOMINION_DATA=runtime`가 소스 체크아웃 내부인 경우에도 격리된 Git worktree는 소스 밖 형제 디렉터리로 배치됩니다. Git worktree는 OS 샌드박스를 제공하지 않습니다. [구현 호출도](docs/architecture/AGENT-HARNESS-SUB2-REJUDGE-2026-11-14.md)에서 확인할 수 있습니다.

## 2026-11-14 · 서브2: 연결 채널 통합 · 조용한 자기개선 · 기억 직접 수정

메인의 연결 채널 UX를 `dominion-sub2`에 계승했습니다. **사이드바에는 연결 채널 1개만 표시**하고, 화면 내부에 **채널 연결 / MCP·스킬 / 역량 마켓** 3개 탭을 제공합니다. 원래 채널 설정·OAuth, MCP 설치·실행, 스킬 가져오기·설치 및 역량 마켓 API는 유지합니다. 이전 `#/extensions`, `#/capabilities` 주소는 해당 연결 탭으로 열리고, 탭 선택이 `#/connections?section=...` 주소에 반영됩니다. 구현 경로: `web/src/{App.tsx,components/WorkspaceShell.tsx,pages/Connections.tsx}`, 내부 패널 `pages/{Extensions,Capabilities}.tsx`.

자기개선 화면은 자동 캠페인 조회 실패나 이전 작업 오류를 반복적인 상단 알림으로 노출하지 않고, **사용자 조작 실패만 즉시 표시**합니다. 실제 RSI 캠페인·설치·공급자 오류는 자기개선 화면의 접힌 기록에서 확인하며, 실행·승인·감사·복구 경로는 그대로입니다. `web/src/pages/SelfImprovement.tsx`의 백그라운드 조회 간격을 1.5초에서 4초로 조정했습니다. 실제 OS/브라우저 푸시 알림 코드는 검사한 프론트 진입점에서 발견되지 않았으며, 브라우저 상호작용 검증은 별도입니다.

심판이 지적한 기억 수정 누락도 보완했습니다. `Memory.tsx`에서 개별 기억의 내용을 직접 편집할 수 있고, `POST /api/memories/:id/edit`는 `api/collections.rs::action`에서 원본 문구 비교 후 원자적으로 반영합니다. 원래 범위·출처·근거를 보존하고 `owner_revision`으로 사용자 정정 사실을 명시합니다. 동시 수정 충돌은 HTTP 409로 반환합니다.

**당시 전체 메뉴 14개** 및 화면 → API → 엔진과 실행/검증 수준은 [서브2 전 메뉴·알림·기억 감사](docs/architecture/SUB2-NAVIGATION-AND-NOTIFICATIONS-2026-11-14.md)에 정리했습니다. 프런트 타입 검사·빌드가 통과한 커밋은 `d8aca686`; 이후 기억 API 테스트와 문서 변경의 Rust 전체 테스트/실제 브라우저 검증은 CI 실행 결과와 구분해야 합니다.

## 2026-11-14 · 서브2 재심사 대응: 자율 목표 15번째 메뉴 · 장편 AI 영상 · 기억 출처 보호

심판 재심사에서 `dominion-sub2`는 **68/100**(영상 5/15, 자율 에이전트 19/25)이었습니다. 이는 변경 전 HEAD `7fbc8a9`의 심사 점수이며 **변경 후 점수는 심판의 독립 재평가 전까지 주장하지 않습니다.**

- **실행 중 목표 조향:** `GoalConsole.tsx → POST /api/goals/:id/steer → goal_steering.rs → goals.rs::run` 경로에서 새로운 사용자 계획을 다음 실행 턴 경계에 반영합니다. 요청은 기대 버전·상태를 원자적으로 검사하고, 현재 진행 중인 도구 작업·승인·검증 조건은 변경하지 않습니다. 적용 대기 상태와 실제 적용 영수증은 각각 표시·감사합니다.
- **독립 자율 목표:** `web/src/components/WorkspaceShell.tsx`, `web/src/pages/Goals.tsx`, `web/src/App.tsx`에서 사이드바 작업공간에 **자율 목표**를 추가하여 전체 **15개 메뉴**로 변경. 홈의 `GoalConsole`와 **동일한** 목표·실행·계획 수정·승인·중단·재개·Git SHA 검토 및 적용 경로를 사용합니다. 기존 백엔드 목표 엔진은 중복하지 않습니다.
- **클라우드 LTX 2.5 멀티샷:** `engine/video/{film,ltx,quality}.rs` 및 `api/video.rs`를 통해 **2~6개**의 유료 장면을 작업 계획으로 동결하고, 각 장면 완료 시 SHA-256 영수증과 ffprobe 기술 검수 결과를 기록합니다. 정상 장면은 재사용하며 과금 결과가 불확실한 호출은 자동 재제출하지 않습니다. 장면별 미리보기, SHA가 고정된 선택적 재생성, 재생성 호출 수에 대한 별도 동의를 `web/src/pages/video/FilmRetake.tsx`에 구현했습니다. 결합은 FFmpeg를 사용합니다.
- **로컬 ComfyUI 멀티샷:** `engine/video/cinematic.rs`에 장면 계획, 시각적 연속성 지시, 최대 15개 장면, FFmpeg 1080p 최종 편집, 장면 영수증과 검수·재사용을 연결했습니다. 기존 `comfy.rs`의 `/prompt`·`/history`·`/view`와 함께 동작하고 `Video.tsx`에서 선택할 수 있습니다. 업로드한 워크플로에 `{{prompt}}`와 `{{duration}}`이 필요합니다. 로컬 GPU/CPU 원가 및 실제 제작 품질은 별도 측정 대상입니다.
- **기억 직접 수정의 출처 보호:** `POST /api/memories/:id/edit`는 `source=user`의 **활성·무인용** 기억만 허용합니다. 학습 기록·원문 인용·보관 기록은 HTTP 403으로 차단하고, `Store::patch_if_fields`로 기억 내용과 출처·상태·범위·근거를 원자적으로 비교하여 변경 충돌을 HTTP 409로 반환합니다. 편집 UI 역시 해당 기억에만 노출합니다.
- **오류·승인·복구:** 이전에 구현한 조용한 RSI 상태 표시, 연결 채널 통합, 사용자 승인형 Git 원본 반영을 유지합니다. 비디오 오류는 원인과 저장된 유료 장면을 보존하며 실영상의 시각적 일관성·시청 만족도 성공으로 표시하지 않습니다.

**실행 호출도·테스트 수준·요구 소프트웨어·API/부분 재생성 영수증·한계:** [서브2 장편 영상·자율 목표·기억 출처 보호](docs/architecture/SUB2-CINEMATIC-VIDEO-GOALS-MEMORY-2026-11-14.md). 실제 15개 메뉴의 화면·API·엔진 대응표는 [전 메뉴 감사](docs/architecture/SUB2-NAVIGATION-AND-NOTIFICATIONS-2026-11-14.md).

신규 LTX 다중장면 엔진 초기 작업 커밋 `97c98959`에서 GitHub CI 전체 통과를 확인했습니다. 이후 추가한 ComfyUI 멀티샷·메뉴·기억·미리보기 구현은 [최신 서브2 CI](https://github.com/hushmind3/dominion-rust/actions?query=branch%3Adominion-sub2)의 별도 검증 대상입니다. 유료 영상이나 실모델 장기 RSI 벤치마크가 실행됐다는 의미는 아닙니다.


## 2026-11-14 · 서브2: 실제 목표 완수·실패 복구 신뢰성 개선

재채점에서 서브2는 **73/100**으로 평가됐습니다(당시 HEAD `d1638cb4`, CI 성공). 명령어·메뉴 수는 자율 능력 가산점이 아니라는 판정을 반영하여, **실제 목표 실행 과정**의 결함 4개를 수정했습니다. 이 섹션은 재채점 이후 소스 변경을 설명하며 새로운 점수를 주장하지 않습니다.

- **유령 목표 방지:** `api/goals.rs::create_goal`에서 목표 레코드와 실행 작업을 `Store::transaction`으로 **한 번에 기록**합니다. 작업 큐 저장에 실패하면 목표도 롤백되며 실행 가능한 job ID가 없는 `queued` 목표가 생성되지 않습니다. SQLite의 작업 삽입 오류를 강제로 발생시키는 회귀 테스트를 추가했습니다.
- **실제 진전만 인정:** `engine/goals/progress.rs`는 새로운 성공 도구 작업과 실제 감소한 검증 조건을 비교합니다. 이전에는 실패 상태의 모든 턴이 무조건 정체 횟수를 늘려 생산적 작업도 과도하게 재계획했으나, 이제 의미 있는 진전은 정체 횟수를 초기화합니다. 같은 명령 재시도와 모델의 성공 주장만으로는 진전으로 판단하지 않습니다. **실제 DB에 저장된 도구 영수증의 ID·시간 순서가 없는 성공 이벤트는 완료 증거로 채택하지 않으며, 테스트 성공 이후 추가 소스 수정이 있으면 이전 테스트를 무효화하고 재검증을 요구합니다.** 완료·예산 제한으로 종료되는 마지막 턴까지 최대 64개 진전 기록을 보존하고, 영수증에 생산적/비생산적 턴 수·실제 해소한 검증 조건 수를 남깁니다.
- **외부 효과 미확정 시 중단:** `engine/goals.rs → db.rs::session_has_uncertain_effects`는 모델이 정상 응답했더라도 도구 실행 작업이 진행 중·중단·외부 결과 미확정이면 다음 턴 실행을 중단하고 사용자 확인을 요구합니다. 해당 목표의 세션과 위임 하위 세션(손자 세션 포함)을 SQLite 인덱스로 조사하며, 무관한 에이전트·그룹대화의 기록은 전체 스캔하지 않습니다. DB 조회 오류 역시 검토 상태로 전환합니다.
- **중복 문맥·토큰 낭비 억제:** `engine/goals/turn_input.rs`는 이전 모든 실행 보고를 매 턴 6,000자씩 재삽입하지 않고, 이미 보존된 동일 세션의 도구 이력을 활용하며 검증 조건·계획·현재 오류 정보를 짧게 전달합니다. 검증 근거와 원본 이력은 유지하고 실모델 완수율·청구 토큰을 측정하기 전까지 절감률은 주장하지 않습니다.

[**실제 API → DB → 작업 큐 → 도구 실행 → 증거 비교 → 복구 호출도, 테스트 및 한계**](docs/architecture/SUB2-GOAL-RELIABILITY-2026-11-14.md). 기존 LTX/ComfyUI 장면 제작, 그룹 사용량 원장, 기억 출처 보호, RSI 분리 검증과 Git 작업폴더 소스 반영 경로는 유지했습니다. [서브2 GitHub Actions](https://github.com/hushmind3/dominion-rust/actions?query=branch%3Adominion-sub2)에서 각 HEAD의 `npm` 검사와 Rust 전체 테스트 결과를 확인할 수 있습니다.

## 2026-11-14 · 서브2: 재심사 57점 이후 복구 원자성·실모델 성과 계측 경로

이번 재심사 서브2 점수는 **57/100**이며 당시 구현·테스트와 미측정 실제 성과를 엄격하게 분리했습니다. 다음은 재심사 **이후** GitHub 소스에 직접 반영한 경로입니다. 새 점수·실모델 성공률·원화 절감률을 주장하지 않습니다.

- **중단 후 재개도 원자적:** `POST /api/goals/:id/resume → api/goals/recovery.rs::resume_goal → db/goal_resume.rs::Store::resume_goal_atomic`에서 목표 상태·새 작업 큐·소유자가 직접 확인한 미확정 도구 기록을 단일 SQLite 트랜잭션으로 반영합니다. 저장 실패 시 모두 롤백해 실행 불가능한 `queued` 목표를 없앴습니다. 기존 실행·하위 세션의 불확실한 효과를 임의로 성공 판정하지 않으며, 실제 진행 중인 하위 에이전트 작업을 관리자 확인만으로 완료 처리하지 않습니다.
- **계획·검토·재계획까지 토큰 귀속:** `GoalControl::complete → llm::complete_for_session → gateway::record_usage → Store::session_usage → goals/receipts.rs` 경로로 실행자 외 모델 호출도 목표 세션 영수증에 포함합니다. 기존 `maxExecutorTokens` 필드가 총 목표 세션의 기록 가능한 호출량에 적용되므로 내부 명칭과 달리 사실상 소프트 목표 예산입니다. 실제 청구액·GPU 비용은 아직 미측정이며 영수증에서 이를 명시합니다.
- **현재 실행의 영수증만 노출:** `GET /api/goals/:id/receipt`는 현재 `goal.jobId`에 해당하는 **불변 영수증**만 반환합니다. 재개 전 작업의 이전 성공 기록으로 새로운 작업이 성공한 것처럼 측정되지 않습니다.
- **선택적 실모델 평가기:** `scripts/sub2-goal-eval.mjs`는 별도 격리 서버·실제 Provider로 3가지 파일 작업을 실행한 뒤, 서버 완료 판정뿐 아니라 **독립 파일 바이트 검증·현재 작업 영수증·실제 기록된 모델 호출 사용량**까지 확인하여 1회 성공률과 성공 건당 실제 기록된 전체 토큰 수를 JSON으로 남깁니다. 실제 유료 Provider는 별도 승인 변수가 필수이며, 비용 정보 누락 시 원화 청구액이나 절감률을 만들어내지 않습니다.
- **자동 회귀 검사:** `scripts/sub2-goal-eval.test.mjs`의 비용/검증 오염 방지 테스트를 `.github/workflows/ci.yml`에 연결했고, Rust API의 복구 트랜잭션 강제 실패·하위 작업 원본 기록 보존 테스트를 추가했습니다.

[복구·영수증·실모델 평가 실행 명령·점수 해석 한계](docs/architecture/SUB2-RESUME-AND-REAL-MODEL-EVAL-2026-11-14.md) · [최신 GitHub CI](https://github.com/hushmind3/dominion-rust/actions?query=branch%3Adominion-sub2).


## 2026-11-14 · 서브2: Issue #9 전 제품 연결 — 설치 스킬부터 검증 워크플로·자동화까지

이번 종합 심판의 **62점**은 개선 전 심사 결과입니다. 기능/메뉴 수 자체를 가점으로 계산하지 않고, 상속된 기존 화면에서 **실제 다른 영역의 작업을 완수하는 호출 경로**를 연결했습니다.

- **연결 → 에이전트 → 채팅·그룹대화:** 연결 메뉴에서 설치·활성화한 스킬을 에이전트별 최대 5개 선택합니다. `POST /api/agents`는 비활성·임시·삭제된 ID를 거절하고 `agent.skillIds`를 SQLite에 저장합니다. `chat/context.rs`는 활성·범위 검사를 통과한 해당 스킬만 모델 실행 문맥으로 주입합니다. 기존 에이전트의 이전 공유 스킬 동작은 원래 레코드에 설정이 없는 경우 유지됩니다.
- **에이전트 → 워크플로 → 산출물:** 에이전트 카드에서 해당 에이전트가 배정된 워크플로 작성으로 이동합니다. 각 워크플로 단계에서 파일 내용/존재 또는 도구·명령 성공 등 검증 조건을 지정할 수 있습니다. `workflows.rs → run_quiet_outcome → execution_operation → verification.rs`로 실제 영수증·파일 상태를 확인하고 **검증 실패 시 다음 에이전트로 넘기지 않고 `needs_review`로 보류**합니다. 이전의 조건 없는 워크플로는 `unchecked`로 명시하며 대화 작업을 유지합니다. 결과에는 단계별 완료·증거 ID·기록 토큰이 남습니다.
- **워크플로 → 자동화 → 활동·홈:** 작성한 검증 워크플로를 자동화의 새 `workflow` 유형으로 예약할 수 있습니다. `db/automation_claim.rs`는 반복 실행 슬롯·실행 작업 등록·다음 시각을 **원자적으로** 저장하고, 이전 작업이 대기·실행 중이면 중복 과금 위험이 있는 새 작업을 생성하지 않습니다. 활동 화면과 홈에서 실제 DB의 상태, 검증 결과 및 기록된 토큰을 확인할 수 있습니다.
- **검증 경계:** 격리 SQLite 강제 실패 테스트, 에이전트별 스킬 문맥 격리, 모의 HTTP 제공자→Rust 엔진→도구→파일→단계 검증 및 후속 단계 차단을 테스트합니다. CI의 각 HEAD 검증과 실모델·유료 호출 성공은 별개이며, 새로운 실모델 완수율·영상 시청 품질·원화 과금 절감액은 주장하지 않습니다.

**15개 메뉴별 실제 개선/상속·미해결·API→엔진→검증 수준:** [서브2 Issue #9 종합 연결 감사](docs/architecture/SUB2-ISSUE9-CROSS-MENU-2026-11-14.md). 워크플로 검증 실패는 자동 재실행하지 않고 원본 도구 영수증과 결과물을 보존합니다. 사용자 도구 승인 정책은 계속 적용됩니다.
