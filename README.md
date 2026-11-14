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


### 소스 수준 RSI — 실제 컴파일된 후손의 폐회로

일반 대화의 전략 학습과 별개로, `rsi_evolve` 캠페인은 **고정 모델 가중치** 상태에서 도미니언 코드·하네스·기억·도구·개선 방법을 수정하고 다음 후손이 그 코드를 실행하는지 추적합니다. LoRA·가중치 훈련은 연결하지 않습니다.

```text
src/api/rsi.rs::start
  → src/engine/jobs.rs::run_job (rsi_evolve)
  → src/engine/rsi_evolve.rs::run
  → src/engine/rsi/execution.rs::run_loop
  → 실험 소스 빌드 → --rsi-worker-plan
  → rsi/system/{inventory,blueprint,discovery,frontier,agenda,design}.rs
  → rsi/meta_controller/{provenance,reciprocal,selection,transfer,feedback}.rs
  → src/engine/rsi/worker.rs::worker (--rsi-worker-job)
  → jobs::run_prompt → provider + 모델/도구/실제 소스 편집
  → src/engine/rsi_worktree.rs::compare_at (동결 A/B, 원본 증거 해시)
  → src/engine/rsi_evolve.rs::adopt (분리된 worktree 채택)
  → 후손 재컴파일·새 계획·개선 방법 실행 → 다음 평가
  → src/engine/rsi/proof.rs::recursive_closure_report
```

`src/engine/rsi.rs::safe_path`는 동결된 독립 평가기와 `rsi/proof.rs`를 모델이 작성하는 후보 패치에서 제외합니다. 계획·기억·탐색·메타개선 모듈은 계속 수정 가능합니다.

`rsi_worktree::classify`는 실제 변경 경로에서 하위 `rsi/` 모듈도 개선기(`improver`) 수정으로 분류합니다. `rsi_system.rs`와 `rsi_meta_controller.rs`는 모듈 진입점이며 실제 자기개선 소스는 `rsi/system/`과 `rsi/meta_controller/`에 있습니다.

`recursiveProof.closedLoopObserved`는 3회 채택만으로 참이 되지 않습니다. **메타개선기(A) → 다른 능력(B) → 메타개선기(A) 재개선**이 연속한 3개 컴파일 후손에서 관찰되고, 첫째·셋째 세대의 메타 개선기 소스가 실제 변경되며, 각 단계의 향상과 교차 능력 전이가 원본 동결 평가로 확인되어야 합니다. `src/api/rsi.rs::apply_verified_capability`는 조회 시 저장된 성공 플래그를 신뢰하지 않고 원본 후보 영수증을 다시 검증합니다.

**검증 경계:** 소스에 폐회로 검사·계승 경로가 존재하는 것과 유료 모델이 실제 3세대를 완주한 것은 별개입니다. 이 브랜치 변경에서 CI·테스트·컴파일·실모델 캠페인을 실행하지 않았습니다. `causalAblationProven`은 동등 입력 반사실 실험이 없으므로 계속 `false`입니다.

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

## Sub1: 전체 RSI 구조와 실측 범위 (2026-11-13)

[현재 호출 흐름·컴파일 후손·대조 실험·동결 검증의 상세 구조](docs/RSI-SUB1-ARCHITECTURE.md)

**실제 연결:** `api/rsi.rs::start` → `jobs::run` →
`rsi_evolve::run` → `rsi/execution.rs::run_loop` →
재컴파일 `worker_plan` → 후손 `worker` → `tools::execute` →
`rsi_worktree::compare_at` → `rsi::verify_paired_receipt` →
`rsi/adoption.rs::adopt` → 차세대 계획 및 기억·방법 계승.

독립 실측은 `rsi/method_control.rs`의 **비채택 대조군 워커**와
`rsi/proof/method_contrast.rs`의 원본 평가 재검증을 사용합니다.
`rsi/proof/method_lineage.rs`는 B의 검증된 대조 차이만 C의
개선법 재수정 피드백으로 전달합니다. 캠페인·API의 성공 판정은 모두
`rsi/proof/full_cycle.rs`의 A→B→A2→B2 동일 검사를 사용합니다.

**현 단계:** 코드 연결은 되었지만 실제 4세대 실모델 성공 및
대조 결과의 통계적 인과 증명은 달성했다고 보고하지 않습니다.
테스트·컴파일·빌드·CI·벤치마크는 이번 소스 작업에서 수행하지 않았습니다.
기반 모델 가중치는 변경하지 않습니다.

## Evolving experiment design as editable intelligence (2026-11-13)

`rsi/experiment_policy.rs::design` is an **editable and compiled** descendant experiment allocator. It ranks authentic indexed source targets using history of independently adopted improvements, regressions and exploration, including its own `experiment_design` role, without prescribing A→B→A→B turns. When repeated experiments stall, the source policy can choose its own implementation as the next modification target. It chooses a bounded frozen source-pair protocol or an independently run method ON/OFF control for inherited method-assisted task experiments. `rsi/descendant_plan.rs` applies this choice to the **actual agenda target and source focus**, not just a side report.

The stable typed `rsi/contracts/experiment.rs` authenticates the source SHA-256 of the compiled policy implementation and target, generator commit, history length, target role/domain and frozen execution protocol. The parent coordinator and actual child worker both validate it before editing; neither recomputes the child's mutable selection scores. `rsi/execution.rs` records observed source edits and independent evaluation in `experimentOutcome`, which future policies use as evidence. Worker instructions receive the validated experiment proposal. The four-generation closure verifier is solely the independent **claim adjudicator**, not the intelligence's mandatory planning schedule. No model weights are trained.

**Generation build boundary:** `src/engine/rsi/execution/build.rs::ensure_compiled_generator` owns only release binary creation and source-commit/SHA identity checks. `execution.rs::run_loop` calls this before each generation; no independent oracle/adoption logic moved to the build stage. A descendant planner and experiment allocator still run from the built binary.

Experiment allocation now counts only source-edited, independently verified, actually executed descendant outcomes with matching policy revision; role `experiment_design` references a single non-overlapping evolvable source file. This selection score is a planning observation, never a proof of RSI completion.

### Stable experiment contract independent of planner and proof

The typed `rsi/contracts/experiment.rs` lives under an immutable `rsi/contracts.rs` composition root. `experiment_policy.rs` and `proof.rs` no longer import each other's private implementation. Both compiled descendant and supervisor call the same read-only `verify_experiment_design` through `rsi_evolve::contracts`. `rsi::safe_path` freezes the entire `rsi/contracts/` subtree; evolution modifies the experiment allocation algorithm, never its own source authentication or accepted evaluator.

The stable experiment contract now **seals and replays its canonical proposal revision** from source bytes, inherited policy revision, target role/path/SHA, intervention and question. The mutable experiment policy can change how it selects these fields but cannot invent arbitrary receipt/revision identities. Extra contract source assertions are present but not executed.

### Policy provenance and cost-aware experimental consent

`verify_experiment_design` now receives actual persisted generation history (rather than a caller-supplied count alone), rejects an invented `parentExperimentRevision`, and does not import the parent's mutable domain selector. An experimental protocol requiring a real ON/OFF worker is selectable only when that control is enabled in the user's campaign request; supervisor and worker both reject mismatched plans. The frozen interface accepts future domain identifiers that are verified against the agenda by the existing orchestration layer instead of freezing today's domain taxonomy into the typed shared schema.

**Child execution evidence boundary:** `src/engine/rsi/execution/evidence.rs::collect_generation_evidence` reads original child job/session events, method tool traces, model usage, elapsed time, and recorded source edits. The parent loop receives the same `generation` record, then performs independent frozen comparison and adoption. This separates observation from selection, proof and mutation policy. It changes no weights and has not been compiled or tested.

### GitHub CI recovery — exposed executable contracts

After the first real GitHub Actions Rust compilation report, Sub1 restored cross-module helper visibility following the tools/chat/models split, passed the build output directory and child job identity explicitly into their extracted functions, resolved shared meta classification, and updated API unit-state constructors. Continuous push CI is the source of the actual compiler/test result.

### Full-CI regression repair

The frozen independent evaluator remains non-editable. Unit fixtures in `rsi.rs` now patch the live `rsi/selection.rs`, never the frozen `rsi.rs`. The native mutation improver selector has been updated to that actual compiled source module, with assertions against its current optimizer shape. The source model's expected component count includes the new `experiment_design` role. Two heavy, no-paid-model fixture generation campaigns now allow enough token context for newly expanded source planning while preserving their expected two/four-generation executable lineage checks.

### Verified experiment-policy credit regression

The editable experiment allocator now credits a prior source-policy revision only when the persisted generation carries independently adopted paired evidence, a matching source edit and executed worker, and a matching explicitly verified experiment outcome. Negative unit fixtures cover forged quality labels, missing paired revalidation, revision mismatch and rejected parent experiments. The regular push CI workflow executes these alongside the Rust suite; no RSI live-model jobs are triggered by push.


## 출시 후 통합 완성도 개선 — Sub1 (2026-11-14)

**평가기준은 단순 기능 개수가 아니라 엔진·하네스·알고리즘의 실제 실행 경로와 유지보수성입니다.**
현재 브랜치의 전체 기능별 진입점/실행/계승 관계는 [2026 통합 품질 지도](docs/INTEGRATED-QUALITY-2026-11-14.md), RSI 후손 폐회로의 원본 증거 경계는 [Sub1 구조](docs/RSI-SUB1-ARCHITECTURE.md)에 정리했습니다.

- **턴키 영상 생성**: `web/src/pages/Video.tsx` → `src/api/video.rs` → `src/engine/video.rs::enqueue/run` → `video/{runway,ltx}.rs` → durable job·provider task·MP4. 2026년 Runway 720/1080p 자동·3~5컷 직접 연출/오디오 및 LTX-2.5 Fast/Pro 최대 4K·오디오를 선택할 수 있습니다. 오직 API 키와 비용 승인이 필요하고 대규모 로컬 추론은 강제하지 않습니다. `video::enqueue`가 결제 전에 조합을 검증하고, 원격 제출이 불확실하면 자동 중복 결제를 피하도록 `needs_review`를 유지합니다.
- **그룹 토큰 예산·모델 티어링**: `Rooms.tsx::RoomSettings/RoomModelPicker` → `api/rooms.rs` → `engine/rooms.rs::effective_rounds` + `rooms/model_tier.rs::select` → `run_room_quiet`. 신규 방 `balanced`, 기존 방 `quality` 유지. fast/deep 모델 라우팅은 사용자가 공급자 모델에 직접 역할을 지정하고 켰을 때만 작동합니다. `economy`는 반복 발언을 더 제한하고, 사용자가 직접 지정한 `maxTurns`·멘션·참가자 모델 선택은 우선합니다. 제공자 캐시 측정은 기존 `gateway.rs::record_cache_usage`의 원본 보고 값으로만 표시합니다.
- **구조/테스트 분리**: `rooms/{pool_tests,tests}.rs`, `chat/room_context/tests.rs`, `engine/video/tests.rs`, `api/video/tests.rs`와 memory/learning/algorithms/improve/jobs/db/providers의 inline 테스트를 전용 모듈로 이동했습니다. 새 LTX 실행기는 별도 `video/ltx.rs`입니다.
- **RSI**: API→job→compiled 후손→소스 도구→worktree 동결 평가→채택→방법·전략 재계획의 기존 실행 경로를 유지했습니다. 가중치 학습·LoRA·파인튜닝을 RSI에 연결하지 않았습니다.

**검증 범위:** 사용자 지침에 따라 이번 수정에서는 테스트/컴파일/빌드/벤치마크/CI/실유료 호출을 실행하지 않았습니다. 영상 품질 성능, 캐시 할인률, RSI 후손 성적은 추정치로 채우지 않습니다.


### 2026-11-14: 자율 목표 에이전트의 제품 실행 및 복구 완결 경계

새 통합심사 v3에서는 **자율 목표 에이전트 25점**을 최우선 기준으로 둡니다. [자율 작업 하네스의 실제 호출/영수증/복구 지도](docs/AUTONOMOUS-AGENT-2026-11-14.md)에서 UI → API → Job → 실제 도구 → 독립 검토 → 재계획 → 결과/기억 계승을 추적할 수 있습니다.

- **목표 UX:** `WorkflowsPage` → `workflows/GoalConsole.tsx` → `POST /api/goals`. 실행자·선택적 저비용 Planner/Reviewer·완료조건·최대 실행단계 입력, 단계/검토/재계획/오류 조회, Session 기록 열기와 중단을 지원합니다.
- **검증 단계 복구:** `goal_review::snapshot/replayable` → `jobs::recover` → `goals::run`. 도구가 완료되어 영속화된 검토 단계에서만 검토를 재개하며, 도구를 다시 실행하지 않습니다. 실행 중단·불완전 영수증은 `needs_review`. 이미 저장된 목표 최종 상태와 Job 상태도 재시작 시 일치시킵니다.
- **테스트 완료 조건:** `goal_verification::inferred_checks`는 명시적으로 테스트가 요구된 프로젝트에서 Cargo/Go/Node/Python의 확인 가능한 테스트 명령을 최대 2개 성공 영수증 조건으로 추가합니다. 이 모듈은 테스트를 수행하지 않으며 실제 에이전트 도구 결과를 통해서만 완료가 가능합니다.
- **등록·취소 정합성:** `api/goals.rs`가 `jobId`를 영속화하며 작업 등록 실패를 성공으로 포장하지 않습니다. `api/jobs.rs::cancel`은 실행 전 대기 중인 Goal까지 종료 상태로 전환합니다.

**이번 변경에 없는 것:** 일반 Goal의 DAG 병렬 위임/합성, Git diff 승인·되돌리기 전 구간 UX, 독립 RSI 동결 평가에 의한 일반 Goal 후손 승률 증명. 이번 변경은 테스트/컴파일/CI/벤치마크/실모델 작업을 실행하지 않은 소스 단계입니다.

- **목표 작업의 프로젝트 이해·계측·수정**: `goal_intake.rs`가 저장소의 manifest/최상위 구조를 한정적으로 조사한 뒤 계획자·실행자에게 전달합니다. `POST /api/goals/:id/plan` → `goal_plan.rs`은 실행 중 사용자 수정 계획을 다음 안전한 단계에 반영합니다. `goal_metrics.rs`는 완료 증빙, 모델 토큰, 소요시간, 재계획 횟수를 Goal/Job에 기록합니다. 실제 청구액이나 작업 완수율은 지어내지 않습니다.


### 자율 에이전트의 실제 병렬 조사·Git 변경 확인·복원 (2026-11-14)

- **`delegate_tasks`**: 1~4개 조사 에이전트가 각각 저장된 세션과 작업으로 병렬 실행됩니다. `tools/batch.rs`에서 읽기 전용 도구만 허용하고 `chat/runtime/turn.rs`, `chat/runtime/tool_dispatch.rs`, `tools/dispatch.rs`에서 API/MCP/결정적 실행까지 동일한 제한을 적용합니다. 부모가 개별 `task_result`를 취합해 실제 코드를 수정합니다.
- **변경 내역·되돌리기 UX**: `GoalConsole` → `GoalChanges` → `/api/goals/:id/changes`/`undo` → `goal_changes.rs`. 시작 Git HEAD와 clean 상태를 도구 실행 전에 저장하고, 성공한 쓰기 도구의 파일만 HEAD 기준 diff와 SHA로 대조합니다. 엄격한 조건을 모두 만족한 경우 사용자가 파일 1개의 복원을 직접 승인할 수 있습니다. 동시 외부 편집·처음부터 변경된 저장소·상용 환경 전체 자동 롤백은 지원 범위가 아닙니다.
- CI: 일반 `.github/workflows/ci.yml`의 push 검사로 React 타입 검사·빌드와 Rust 릴리스 테스트를 확인합니다. 정확한 실행 로그는 GitHub Actions를 기준으로 합니다. [자율 에이전트 전체 구조](docs/AUTONOMOUS-AGENT-2026-11-14.md).



### 2026-11-14: 목표별 Git 격리 → 실제 실행 → 검토 → 승인 반영/폐기

`POST /api/goals`의 선택 입력 `isolateWorktree:true`는 파일 수정이 있는 코딩 목표를 원본 checkout과 분리합니다.

1. `goals::run` → `goal_workspace::prepare`: 원본 Git 저장소 최상위·clean 상태를 확인하고 목표 UUID로 별도 Git worktree/브랜치를 만들고 경로·기준 HEAD를 **도구 실행 전** Goal에 영속화합니다. 준비·복구에 실패한 격리 목표는 원본에 쓰지 않고 `needs_review`로 남습니다.
2. `goals::run`의 별도 `scoped_state`: 프로젝트 사전 조사·완료 조건 baseline·실제 실행 `run_quiet_outcome`·`acceptance_gaps`는 격리 작업공간에서 수행합니다. `tools::execute`가 모든 네이티브 호출에서 Goal Session을 재검증하고 `file_*` 절대 경로 도구를 거부합니다. `shell_exec`의 명시적 cwd도 격리 범위로 해석합니다. `gateway::delegate` 및 `jobs::run_prompt`에서 위임받은 에이전트도 같은 Goal/Worktree 컨텍스트를 계승합니다.
3. `/api/goals/:id/changes` → `goal_workspace::changes` → `GoalChanges.tsx`: 격리 공간의 tracked diff, staged diff, 미추적 파일 목록, 게시 가능한 패치 SHA를 원본에 적용하지 않고 비교합니다.
4. `/api/goals/:id/worktree/promote` → `goal_workspace::promote`: **완료 검증 + 사용자 명시적 승인 + 작업 중인 다른 Job 없음 + 기준 HEAD 불변 + 원본 clean + 미추적 파일 0 + 512KB 이하 실제 바이너리 패치 SHA-256 일치 + `git apply --check`** 조건에서만 실제 추적 파일의 수정 내용을 원본으로 적용하고 영수증을 영속화합니다. 자동 커밋이나 강제 병합은 없습니다. 새 파일은 Git에 추가해 diff에 포함한 뒤 반영해야 합니다.
5. `/api/goals/:id/worktree/discard` → `goal_workspace::discard`: 진행 중인 목표/하위 작업에는 거부하며, 명시적 요청이 있을 때만 격리 worktree를 폐기합니다. 이미 원본에 적용된 변경은 삭제하지 않고, 미반영 결과를 폐기한 경우에만 Goal의 완료 판정을 무효화합니다.

**경계:** Git worktree는 소스 파일의 분리이지 운영체제 샌드박스가 아닙니다. 허용된 셸·외부 앱·MCP는 승인 정책을 따르며 임의 셸 명령이 절대경로를 사용할 수 있다는 한계가 있습니다. 서로 다른 코드 작성 서브에이전트의 충돌 자동 병합이나 실모델 작업 완수율까지 증명한 것은 아닙니다. 자체 RSI 후보의 동결 평가·채택 통로와 일반 Goal의 opt-in Git 격리는 별개입니다. [자율 목표 실행 경로 전체](docs/AUTONOMOUS-AGENT-2026-11-14.md).


### 2026-11-14: 저비용 멀티샷 영상·기술 검수·선택 장면 재생성

- **일반 영상 생성**: `POST /api/video/jobs` → `video::enqueue` → `jobs::run_job` → `video::run`. 최종 MP4의 실제 ffprobe 스트림·해상도·화면비·시간·오디오를 `video/quality.rs`에서 검사하고 결과를 저장합니다. ffprobe를 찾지 못하면 검증됐다고 꾸미지 않고 `not_verified`로 표시합니다. 엄격 검수 선택 시 API 비용 발생 **전에** 설치 여부를 확인합니다.
- **LTX-2.5 영화 모드**: `ltxFilm:true`, `confirmPaidMultiShot:true` 요청 → `video/film.rs::spec`에서 유료 2~6개 샷의 장면 프롬프트·길이·모델·해상도를 동결 → 순서대로 유료 생성 → 각 MP4의 SHA-256·API 영수증·ffprobe 결과를 `receipt.json`으로 저장 → 유효한 완료 장면은 재사용 → FFmpeg stream-copy로 납품 MP4 제작. 네트워크·품질 불확실 시 해당 장면 파일을 보존하고 **자동 중복 결제하지 않습니다**. 장편 연결에 FFmpeg가 필요합니다.
- **선택 장면 재제작**: `GET /api/video/jobs/:id/scenes` → 영상 UI의 `FilmRetake`에서 결과 SHA·미완성 뒤쪽 장면 수·예상 신규 유료 호출 수 확인 → 명시적 동의 → `POST /api/video/jobs/:id/retake` → `jobs::enqueue_once`의 결정적 작업 ID → 신규 작업이 이전 정상 장면의 MP4/영수증을 검증 후 복사하고 선택 장면 및 아직 없는 장면만 유료 호출합니다. 원본 유료 영상과 영수증은 보존됩니다. 기존 파일과 영수증이 불일치하면 재구매 없이 사용자 검토가 필요합니다.
- **RSI 연구 입력**: `rsi_scan::report` → `rsi_scan/goal_signals.rs::collect`가 사용자 목표의 반복 실패와 실제 성공 영수증 수를 통계로 수집 → `rsi/system/frontier.rs`에서 실제 소스 개선 목표 연구에 활용합니다. 사용자 자유 텍스트·파일 내용은 전달하지 않습니다. 작업 성공 통계는 **독립 RSI 평가/채택 기록으로 사용하지 않습니다**.
- 기술 검수는 재생 메타데이터 검사이며 영상의 의미·시청 품질·인물/장면 일관성을 증명하지 않습니다. 유료 API 실제 청구액과 유료 모델 RSI 자율 다세대 개선은 아직 실증되지 않았습니다.


### Sub1 2026-11-14: room tool-intent correctness and selection evidence

- `src/engine/chat/runtime/tool_selection.rs::{room_tool_request,select_room_tools}` distinguishes whole-word ASCII actions from incidental substrings (for example `latest`/`test`, `profile`/`file`, `thread`/`read`, `brunch`/`run`). Korean inflected action stems still match, English `files`, `tests`, `continue` and `resume` are supported.
- Code investigation includes `code_search` and bounded workspace reads. Explicit editing can expose `workspace_replace_range` and `workspace_replace_function`; batched `delegate_tasks` joins the delegate tools. Recent tool history boosts the selection only for explicit continuation, not unrelated new turns.
- `src/engine/chat/runtime/turn.rs` persists `execution.toolSelection` with catalog/selected tool counts and serialized schema bytes **before provider-specific budget truncation**. These are deterministic routing diagnostics, **not** proof of charged-token or won-currency savings. Provider usage, cache writes/reads, task success and actual costs must be evaluated separately.
- Regression tests: `tests/support/engine/chat_runtime.tests.rs` covers lexical false positives, positive English actions, fresh-vs-continuation history and source editing exposure. No CI, Rust compiler, paid API or live-provider verification was run for this change.
- Research context (2026): [OpenAI Tool Search](https://developers.openai.com/api/docs/guides/tools-tool-search), [Anthropic Manage Tool Context](https://platform.claude.com/docs/en/agents-and-tools/tool-use/manage-tool-context), [ToolBottleneckBench (Oct 2026)](https://arxiv.org/abs/2610.21098) and [Airbnb Agent Harness Optimizer](https://github.com/airbnb/agent-harness-optimizer). Dominion implements its own deterministic selection here; it does not claim native provider-side deferred search.


### Sub1 follow-up: shared intent matcher and autonomous test gate

- The action-word boundary policy now lives in `src/engine/text_intent.rs`, is registered in `src/engine/mod.rs`, and is reused by group routing and `src/engine/goal_verification.rs::asks_for_tests`. Natural-language words such as `latest`, `testament` or `protest` no longer accidentally turn on `cargo test` completion obligations. Explicit `test`, `tests`, `testing`, and Korean `테스트` retain the test gate.
- Room code intents now cover explicit English `fix bug`/`patch`/`refactor` alongside Korean requests. Source discovery and surgical edits remain bounded by the existing execution approval and worktree policies. `goal_verification.rs` and the shared matcher both have regression tests.


### 2026-11-14 Sub1: integrated connection hub, quiet RSI and memory edits

- **15-menu UI:** Sidebar 4 categories: workspace (7), extensions/connections (2), operations (4), advanced (2). "Connections" is the single sidebar entry for **Channels / MCP & Skills / Capability Market**. Main's original `Connections.tsx` integration was ported; legacy `#/extensions` and `#/capabilities` URLs resolve to the right tab of `#/connections`. Channel, OAuth, MCP, plugin, skills, and catalog APIs are unchanged.
- **No unsolicited RSI alerts:** The root `App.tsx` and `main.tsx` do not mount `ImprovementNotice` or poll for promotional RSI recommendations. The RSI page still displays actual campaign state, errors, approval and receipts when opened. Shared execution refresh broadcasts remain intact.
- **Manual memory editing:** `Memory.tsx` adds a controlled edit/save/cancel UI for owner-authored memories. The `collections::action` endpoint `POST /api/memories/:id/edit` preserves memory scope, rejects learned-record edits, checks the original text (409 conflict), and invalidates stale quotation provenance. `collections.rs` includes regression tests.
- **Audit:** [15-menu UI source/evidence matrix](docs/SUB1-UX-MENU-CONTRACT-2026-11-14.md). Deterministic source contract suite: `node scripts/verify-frontend-navigation.mjs`. CI runs that suite, TypeScript, Vite build and Rust tests; actual browser interactions/provider billing/RSI evolution are separately reported, not assumed.

- **Optional real-browser contract:** `web/tests/connections.browser.mjs` launches a local Vite/Chromium session with deterministic mocked APIs. It actually clicks the 3 tabs, exercises legacy links and the manual-memory edit, then holds an unrelated chat page open for 16 seconds to detect unsolicited RSI dialogs. The GitHub workflow `.github/workflows/sub1-ui-browser-smoke.yml` uses **manual dispatch only** and installs Chromium for the run. This browser check is implemented but not claimed as executed.

### 2026-11-14 Sub1: reviewed autonomous-goal continuation + atomic memory CAS

- **First-class `자율 목표` menu** (`web/src/pages/Goals.tsx`) shares the pre-existing `workflows/GoalConsole.tsx` with Workflows instead of forking the logic. 4 sidebar groups now have **15 visible menus**. The goal screen exposes token-budget admission, owner-requested cancellation and human-reviewed continuation.
- **Durable goal cancel/resume**: `POST /api/goals/:id/cancel` terminates goal + active/queued jobs and invalidates the session cancellation epoch; `POST /api/goals/:id/resume` requires `acknowledgeUnknownEffects:true`, rejects active/terminal jobs, preserves successful evidence and last completed iteration, writes resume lineage and queues a new unique job. The worker starts after persisted iterations with explicit external-state reinspection instead of replaying an in-flight tool turn; existing evidence and acceptance checks remain intact. This does not prove unknown side effects were undone.
- **Cost admission**: planner/reviewer/replanner use session-attributed usage records; an observed token ceiling blocks new helper calls when already exhausted. Provider-reported token counts can lag in-flight calls; no actual KRW billing amounts are inferred.
- **Atomic memory editing**: `Store::edit_manual_memory` uses one SQLite transaction for source=user + active/eligible + expectedText guards, update and audit. Concurrent edits cannot bypass CAS; learned and archived memories are refused. New regression tests cover concurrent writes and provenance.
- **CI browser E2E**: Chrome DevTools smoke is now explicitly opt-in with `DOMINION_BROWSER_E2E=1`; the previous default CI failure came from GitHub runner blocking the Chrome remote-debugging port, after 826 Rust tests passed. Generic unit tests cannot demonstrate real Chrome availability. The optional Playwright smoke remains separate.

### 2026-11-14: 실제 도구 영수증 및 변경 후 테스트 재검증

단계가 긴 코딩 Goal은 테스트가 한번 성공했다는 이유로 이후 소스 변경까지 검증했다고 볼 수 없습니다. `engine/mod.rs::run_quiet_outcome`이 `tool_end` 이벤트와 정확한 DB `execution_operation`을 연결해 `runId/opSeq/startedAt/finishedAt`를 보존하고, `verification.rs::fresh_command_matches`가 마지막 소스 변경 뒤의 성공 검증만 인정합니다. 원본 영수증 없는 이벤트는 증거로 사용하지 않습니다. 코딩 목표의 저예산 도구 선택 정책에서도 소스 검색과 국소 수정 도구를 보존합니다. [실행 경로·검증 및 한계](docs/SUB1-EXECUTION-RECEIPT-CONTINUITY-2026-11-14.md).

### 2026-11-14: LTX-2.5 영상 유료 견적·승인 제어

LTX-2.5 신규 영상 및 일부 장면 재생성은 `POST /api/video/quote` 또는 `/api/video/jobs/:id/scenes`의 **서버 계산 견적**을 프론트에 원화 참고액으로 표시한 뒤, `approvedPaidQuote`·`maxUsdCents`·`quoteDigest`의 승인을 받아야 영상 작업이 영속 큐에 등록됩니다. 가격은 공식 LTX 2.5 Fast/Pro 720p~4K 초당 목록가의 2026-11-14 스냅샷이며 원화는 2026-11-13 참조 환율을 사용합니다. 정확한 실제 청구액이나 영상 시청 품질을 보증하지 않으며, 기존 LTX 장면별 SHA 재사용·FFprobe 검수·미확정 결제 중지 구조를 유지합니다. [실제 과금 경로·테스트](docs/SUB1-VIDEO-PAID-QUOTE-2026-11-14.md).

### 2026-11-14: 자율 목표의 실측 진전 기반 재계획

`engine/goals.rs`는 새 `engine/goals/progress.rs`를 통해 실제 영속 도구 영수증의 새로운 변경·검증 조건 해소를 측정합니다. 유효한 작업이 진행되는 동안에는 정체 카운터가 증가하지 않으며, 실패/반복 호출과 추가적인 수정을 구분합니다. `workspace_replace[_range|_function]` 국소 편집도 수정 증거로 계산합니다. [실행 검증 경로](docs/SUB1-EXECUTION-RECEIPT-CONTINUITY-2026-11-14.md).

### 2026-11-14: 검증된 작업 완료율과 실모델 재현 평가

첫 Goal 등록도 `/api/goals` → `Store::transaction([goal,job])` 단일 SQLite 트랜잭션으로 바뀌어, 프로세스 중단 시 유실된 작업 ID가 있는 대기 목표가 생기지 않습니다. `GET /api/goals/metrics`는 실제 저장된 Goal에서 검증된 완료율/실패 및 보류 분모, 실제 모델 호출·토큰·캐시 보고량을 집계합니다. 미청구 원가는 `null`로 유지합니다. `scripts/sub1-measured-goal-eval.mjs`와 `.github/workflows/sub1-measured-goal-eval.yml`로 공개 저장소 GitHub Actions에서 **실제 Ollama Qwen3 4B**를 격리 실행해 2개의 소형 다중 파일 작업을 비공개(모델 미제공) 검사기로 채점하고 SHA·Goal/Job·토큰 영수증을 아티팩트로 남깁니다. 통과율은 실행 결과가 나왔을 때만 보고하며 대형 SW 공학 작업 성공률로 부풀리지 않습니다. LTX 유료 워커는 실행 시작 직전에 동결 승인 견적을 다시 검사합니다. [재현 평가 경로 및 범위](docs/SUB1-MEASURED-GOAL-2026-11-14.md).

### 2026-11-14 · 서브1 전 제품 연결: 에이전트 능력 → 검증된 워크플로 → 납품

서브1은 기존 워크플로 실행 엔진을 실제 `/api/workflows` 등록·실행·이력·산출물 API로 노출합니다. `Models.tsx`에서는 연결한 제공자/모델을 에이전트에 할당하고, `Agents.tsx`에서는 **설치된 스킬**을 최대 8개까지 지정합니다. 선택한 스킬은 채팅·그룹대화·워크플로 모델 컨텍스트에 우선 주입되고 실제 도구 권한은 기존 승인 정책이 통제합니다. 에이전트 카드에서 2단계 워크플로(실행·검토)를 만들고 `Workflows.tsx`에서 실행하면 작업 ID별로 고정된 모델 설정, 단계별 영속 도구 영수증, 결과물과 토큰 사용량을 저장하고 `DeliverablePanel`에서 확인합니다. 선택적인 결과 파일 SHA 변경 검사에 실패하면 후속 단계는 실행하지 않습니다. `Developer.tsx`는 실제 Git diff를 사용자가 고른 워크플로로 보내 검토 산출물을 만들 수 있습니다. `Home.tsx`는 모델→에이전트→워크플로 준비 동선과 마지막 결과를 표시합니다.

[15개 메뉴별 실제 개선/남은 갭 및 상세 호출 경로](docs/SUB1-WHOLE-PRODUCT-ISSUE9-2026-11-14.md). 테스트·모의 제공자·실모델 결과를 구분합니다. 직전 실제 Qwen3 4B 소형 과제 2개 평가의 독립 오라클 성적은 **0/2**로, 완수율 향상을 주장하지 않습니다.

### 2026-11-14 · 서브1 실제 반복 워크플로와 활동 산출물

`POST /api/automations`에서는 저장된 에이전트 워크플로/일반 에이전트 작업/그룹 회의를 검증해 반복 실행할 수 있습니다. 반복 예약 시각별 동일한 작업 ID를 사용하여 프로세스 재시작이나 갱신 실패가 곧 중복 유료 호출로 이어지지 않도록 했습니다. 워크플로는 예약 실행 순간 모델을 동결하고, 삭제·비활성 제공자는 작업 시작을 차단해 자동화 규칙을 검토 상태로 전환합니다. 활동 메뉴에서 완료된 영속 협업 산출물과 단계별 영수증을 직접 확인합니다. 실제 청구 원가·월별 예산 상한·알림 전송은 별도 미구현 사항입니다. [15개 메뉴 영향과 호출 경로](docs/SUB1-WHOLE-PRODUCT-ISSUE9-2026-11-14.md).

### 2026-11-14 · 공식 오픈소스 Qwen-Agent SDK 직접 실행 선택지

서브1의 기존 로컬 Qwen3 모델 2개 코딩 과제 실측은 **0/2**였습니다. 모델 HTTP 응답은 있었지만 목표 파일은 하나도 바뀌지 않았습니다. GitHub/Hugging Face의 공식 에이전트 SDK 6개를 실제 조사하고, 별도 재구현보다 [QwenLM/Qwen-Agent](https://github.com/QwenLM/Qwen-Agent) (Apache-2.0)의 공식 Tool Parser + Assistant 호출 루프를 도입한 **별도 로컬 코딩 실행 백엔드**를 만들었습니다. 사용자는 **개발자 메뉴**에서 저장된 127.0.0.1 Ollama Provider·작업 지시문·선택적 검증 대상 파일을 지정하고 명시적으로 파일 수정에 동의한 후 실행합니다. `POST /api/jobs`의 `sdk_local` 타입 → Rust 승인/모델/경로 검사 → 중복 방지 Job → 실제 Qwen-Agent Python SDK → 제한된 `list/read/write/replace` 도구 → 변경 SHA 영수증 → Rust 디스크 SHA/검증 조건 확인 → 사용자 결과까지 이어집니다. 모델 API 사용료는 들지 않아도 로컬 PC의 추론 전력·시간 원가는 존재합니다. Python 패키지는 선택 설치: `python3 -m pip install 'qwen-agent>=0.0.26,<0.1'`. 설치하지 않으면 기존 도미니언 기능은 그대로 사용합니다. 이 경로의 **새 실모델 성공률은 아직 미측정**입니다. [SDK 비교·실패 로그 원문·사용자 호출·한계](docs/SUB1-OPEN-AGENT-SDK-2026-11-14.md).
