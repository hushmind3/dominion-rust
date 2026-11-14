# dominion-rust

DOMINION을 **Rust 백엔드 + Rust 실행 엔진**으로 다시 구현한 저장소입니다.

- 프론트엔드: 기존 React UI 그대로 (`web/`)
- 백엔드: Rust (axum)
- 실행 엔진: Rust (모델 스트리밍, 추론 토큰, 도구 호출 루프, 승인 게이트)
- 저장소: SQLite (WAL), 레코드는 JSON 문서
- Node.js: 런타임이 아니라 **빌드 도구로만** 사용 (`vite build`)

## 2026-11-14 서브4 출시 제품 통합 업그레이드

자기개선 RSI 외에 **영상 제작, 그룹 대화 비용, 모델 자동 선택, 채팅 사용자 경험**을 같은 Rust 엔진에 연결했습니다.

- **턴키 시네마틱**: Kling 3.0 네이티브 멀티샷 (1회 요청 내 여러 촬영 컷)과 Kling 4.0 최대 30초 1080p 연속 생성 중 선택하고, 유료 클립의 마지막 프레임을 다음 이미지-투-비디오 첫 프레임으로 넘겨 시각적 연속성 강화. 설정 UI에서 연속성 사용 여부를 선택합니다. 이전 유료 클립의 생성 기록이 남은 작업은 자동 재제출하지 않아 중복 과금을 막습니다. 진입점 `Video.tsx` → `api/video.rs` → `engine/video.rs` → `video/{shot_plan,kling4,frame_anchor,runway}.rs`.
- **그룹 대화 적응형 하네스·캐시 계측**: 새 방의 자동 발언 정책을 실제 `algorithms::choose_for_agent("multiagent.economy")`로 선택해 작업에 고정하고, `rooms/economy.rs`와 `rooms/speakers.rs`가 발언 인원·라운드·사용자 지정 저비용/심층 모델을 실행합니다. 완료된 자동 토론에 대한 👍/👎 평가가 선택된 revision의 후속 선택 통계로 이어집니다. 기존 방과 주인의 전원·지목 요청은 보존합니다. 런타임 테스트 2개 모듈을 `tests/unit/`으로 이관하여 `rooms.rs`를 약 1,000줄로 축소했습니다. 그룹 문맥·프롬프트 접두사 최적화에 더해, 제공자가 실제 보고한 캐시 재사용량과 측정 가능한 입력만 별도 집계해 방 UI에 노출합니다. `chat/provider.rs` → `gateway.rs` → `db.rs::session_usage` → `Rooms.tsx`.
- **가격·품질 인식 모델 자동 선택**: 연결 설정에서 원화 기준 입력/출력 100만 토큰 단가와 직접 평가한 0~100 품질 점수를 지정하면, 기존 경량/심층 라우팅이 **가격이 알려진 모델끼리만** 비교합니다. 사용자·에이전트·회의실에서 명시적으로 선택한 모델은 덮어쓰지 않습니다. `Models.tsx` → `api/providers.rs` → `router.rs`.

전체 변경 범위·호출 지도·근거·다음 품질 게이트는 [서브4 제품 전체 통합 업그레이드](docs/SUB4-PRODUCT-UPGRADE-2026-11-14.md) 문서를 참조하세요. **소스 변경과 미실행 테스트 작성까지 수행했으며 이번 수정에서 테스트·컴파일·빌드·CI·유료 추론은 실행하지 않았습니다.**

## 그룹 대화와 내 에이전트

`그룹 대화 · 회의` 화면에서 주인이 대화에 직접 참여하고, 에이전트들이 같은 기록을 읽으며
서로 의견·검토·잡담을 이어갑니다. 예전처럼 에이전트마다 가짜 사용자 메시지를 넣지 않습니다.
전체 토론·순번 발언·멘션 응답을 고르고, 사용자 개입이나 중지로 대화 흐름을 바꿀 수 있습니다.

에이전트 화면의 `기존 페르소나 가져오기`는 사용자의 로컬 9개 프로필에서 SOUL·아이콘만
읽어 DOMINION 프로필로 만듭니다. 원문·이미지는 `runtime/`에 보관하며 기존 모델 설정과
사용자가 수정한 프로필을 보존합니다. 다른 프로젝트의 실행 엔진·토큰·스킬을 가져오지 않습니다.

도구를 허용한 대화방에서는 실제 Rust 파일·셸·HTTP 도구를 사용합니다. 작업 큐·진행·승인·
실행 증거는 Rust와 SQLite에 남으므로 브라우저를 닫아도 실행은 계속됩니다.

읽을 코드부터 빠르게 찾으려면 [RSI 소스 지도](docs/RSI-CODE-MAP.md)와 [현재 서브4 구조](docs/SUB4-RSI-ARCHITECTURE.md)를 확인하세요. 과거 인수인계·임시 벤치마크 결과는 현재 소스 목록에서 정리했습니다.

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

## RSI 소스 폐회로: 후손 실행과 독립 증거 (dominion-sub4, 2026-11-13)

이 경로는 **컴파일된 후손의 결정**과 **독립 평가기 영수증**을 분리한다. 3회 채택 자체는 완성 증거가 아니다.

```text
POST /api/rsi/... → src/engine/rsi_evolve.rs (job 등록·캠페인)
→ src/engine/rsi/execution/campaign.rs (실험 worktree·후손 바이너리 구동)
→ src/engine/rsi/planner/worker_plan.rs (후손 자기 소스로 계획 재구성)
→ src/engine/rsi/execution/worker.rs (provider·모델·편집 정책 선택, 도구 실행)
→ src/engine/rsi/evaluation/worktree.rs (고정 사례 paired 평가)
→ src/engine/rsi/lineage/adoption.rs (유효 후보 채택·새 커밋)
→ 재컴파일 후 새 generation 작업 (방법·기억·전략·소스 계승)
→ src/engine/rsi/proof/closure.rs (원본 candidate, job, paired receipt 재열람)

`src/engine/rsi/proof/source_roles.rs`는 실제 실행되는 RSI 계획·학습·편집·계승
소스를 개선기 방법으로 분류한다. 동결 평가·독립 증명 및 일반 애플리케이션 능력은
방법 소스로 분류하지 않는다. A와 마지막 A는 **방법 소스만 변경**해야 하고,
B는 **방법 소스를 변경하지 않은 별도 능력 개선**이어야 A→B→A로 인정한다.
보호 경로는 `rsi/proof/path_policy.rs`에서 단일 관리한다.
`rsi::safe_path`가 후보 패치를 거부하고 `code_index::is_source`가 자동 수정 대상 인덱스에서
같은 소스를 제외한다. 평가 API의 기존 인라인 테스트는
`tests/unit/engine_rsi_api_tests.rs`로 이동하여 독립 검증 및 프로덕션
소스의 책임 경계를 유지한다. 동결 기준과 별개인 실험용 `intervention_lab.rs`,
`rule_tournament.rs`는 개선·평가 대상으로 유지한다.
네이티브 RSI 변형기가 탐색 정책의 실제 코드를 수정할 수 있도록
`rsi_evolve.rs::search_features`와 `select_domain`의 원래 결합을 유지한다.
```

- `rsi::capability_proven`: 연속 3세대의 원본 paired 평가와 개선기→다른 능력 전이에 관한 **기초 증거**만 확인.
- `rsi::campaign_capability_proven`: 위 조건에 더해 **개선 방법 소스 수정 A → 실제 재빌드된 후손 실행 및 다른 능력 B의 독립 개선 → 후손의 개선 방법 재수정 A**를 원본 레코드로 검증해야 참. 모델의 주장·계획서·세대 요약 필드만으로 인정하지 않는다.
- `rsi::capability_audit`의 `recursiveClosure`: 충족 여부, 원본 candidate ID, 생성·채택 커밋, 후손 바이너리가 선택한 편집·메타 정책 revision을 표시.
- 변경된 모델 가중치·LoRA·파인튜닝을 사용하지 않는다. 실험의 개선은 **소스·경험·정책 선택의 변화**이며 격리 계보 채택과 운영 반영을 구분한다.

**인과 대조 증거:** `rsi/proof/method_control.rs`는 A 개선 전 원본 생성기로
동일한 동결 과제와 프롬프트를 수행한 실제 대조 세대의 후보·평가·모델 사용
감사를 재검증한다. 개선된 B가 같은 모델·충분한 예산의 모든 비교 가능 대조군보다
실패→성공 holdout을 추가로 해결했을 때만 엄격한 폐회로를 인정한다.
대조군이 없으면 `recursiveClosure.methodControl`에 필요한 작업·커밋·과제 지문을
표시하고 RSI 완료는 거짓으로 유지한다. 비무작위 실험의 인과 일반화까지
확정했다는 뜻은 아니다.

**검증 경계:** 실제 신규 3세대 A→B→A 성공 기록은 이 커밋에서 실행·제출하지 않았다. 따라서 전체 지능의 RSI 완성이나 독립 심판의 인정 점수를 주장하지 않는다. 실행 증거가 생기면 `recursiveClosure.proven=true`의 원본 영수증과 이력을 비교해야 한다. 세부사항은 [RSI 완료 기준](docs/rsi-completion.md).

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

## 구현 범위와 후속 완성 과제

Rust 엔진은 GGUF·MLX 연동, ComfyUI/FFmpeg 및 Runway Dev 유료 영상 생성, 채팅·그룹 작업·기억·에이전트를 연결합니다. 아래 항목은 일부 구현 또는 운영 환경 검증이 필요한 기능이며, 실제 미지원 API는 해당 엔드포인트의 JSON 상태로 확인해야 합니다.

- 다운로드 일시정지/재개 (시작·취소·재시작 지원)
- OAuth 실제 동의·토큰 발급 운영 검증 — 사용자의 GitHub/Google 앱 자격 증명 필요
- Slack/Telegram 등 Discord 이외의 메신저 드라이버
- 영상 생성 후 프레임·사운드 자동 품질 검사와 `needs_review` 유료 원격 클립 수동 복구 UI
- 설치 파일 자동 적용 (데스크톱 셸 업데이트 경로)
- 제공자별 실청구액과 채팅 토큰 감사 기록의 대사, 적응형 그룹 대화 정책의 비용·품질 A/B 검증

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

### 실제 A-off 대조 실행 (선택적, 유료 모델 호출)

`GET /api/rsi/campaigns/:id/verify`에서 `recursiveClosure.methodControl`이
보여 주는 `treatedGenerationId`를 사용합니다.
`POST /api/rsi/start`에 `mode:"continuous"`,
`confirmation:"START_RSI_EVOLUTION"`, `controlForGenerationId`를 전달하면
서버가 원본 A·B 영수증으로부터 **A 직전의 실제 생성기 커밋**을 복원합니다.
사용자가 임의 기준 커밋·모델·과제를 입력하지 못하며, B 작업의
`rsiPromptBase / rsiFrozenCases / objective / model`을 그대로 가져옵니다.
기존 worktree 생성기·provider 모델·도구·동결 비교를 1회 실행하고,
대조 후보를 채택·운영 반영하지 않습니다. 메인과 현재 브랜치는 건드리지 않습니다.
모델 호출 비용은 별도로 발생하므로 명시적 시작 요청이 필요합니다.

`계속 발전하기 → 검증 시험`에서 대조군이 부족한 A→B→A 사례를 확인한 뒤
**대조군 1회 실행** 버튼으로 같은 API를 실행할 수 있습니다. 표기된 실행은
독립 모델 비용이 발생하며, 대조 후보의 채택 및 작업 브랜치 반영은 금지됩니다.

대조 캠페인을 종료한 뒤 **원래 RSI 캠페인으로 돌아가기**를 누르면 기존
독립 검증 기록을 다시 읽어 동일한 폐회로 판정을 수행합니다.

대조 입력·모델·원본 작업의 보안·재현 검사는
`src/api/rsi/control_request.rs`에 분리했습니다. 원래 API 단위
검사 코드는 `tests/unit/api_rsi_tests.rs`로 이동했습니다.

실제 대조군의 분야 선택은 개선 전후의 알고리즘이 달라질 수 있는 **결과 변수**이므로
동일한 모델·작업·동결 사례를 사용하는 한 다른 분야를 선택했다는 이유로 제외하지
않습니다. 평가 성과 자체는 같은 동결 사례별 성공·실패 전환으로 비교합니다.

### 자동 후손 대조 실행 — 정상 RSI 작업 경로

`POST /api/rsi/start`의 연속 캠페인에 `rsiAutoMethodControl`이
활성화되면, `execution/campaign.rs`는 A(개선법) 채택 직후 B(다른 능력)가
독립 채택된 원본 영수증을 `rsi_closure::control_anchor`에서 확인하고
`lineage/auto_control.rs::queue_if_needed`로 **대조 작업을 자동 등록**합니다.
사용자에게 B 세대 ID를 입력하도록 요구하지 않습니다.

대조 작업은 `jobs::enqueue_once`로 B 세대당 정확히 한 번 생성되며
기존 `rsi_evolve::run`의 분리 소스 worktree→A 이전 컴파일된 생성기→
같은 provider/model→실제 도구 편집→고정 paired evaluator를 재사용합니다.
`controlRun`은 채택을 무조건 막아 운영 커밋을 오염시키지 않습니다.
대조 평가가 종료된 후, 캠페인 및 API가 같은 `rsi_closure::evidence`
원본 비교를 다시 계산합니다. 원본 캠페인과 대조 캠페인은 개별
작업 및 사용량으로 기록됩니다.

### 대조 결과의 후손 개선법 재투입

`rsi_closure::control_feedback`는 A→B의 저장된 원본 후보·모델 작업·
독립 평가·대조 영수증을 **매번 다시 계산**합니다.
`rsi/lineage/history.rs::revalidated_history`가 이 결과를
`methodControlEvidence`로 재구성합니다.
`planner/worker_plan.rs`는 피드백을 차세대의
`metaPolicy.methodControlFeedback`와 새로운 메타 정책 revision에 포함하고,
`execution/worker.rs`는 같은 증거를 실제 모델·편집 지시 문맥에 전달하여
`rsiMethodControlFeedbackExecuted` 기록을 남깁니다.
대조 작업이 늦게 완료돼도 차기 캠페인의 컴파일 후손은 최신 평가 기록을
재사용합니다. 모델 가중치는 수정하지 않습니다.

### 4세대 재귀 자기개선의 보수적 완성 기준

최종 성공 판정은 단순 3회 채택이 아니라 **A(개선법) → B(새 능력)
→ A2(후손이 B 피드백으로 개선법 재설계) → B2(A2가 만든 후손의 추가
능력 향상)**로 강화했습니다. `rsi/proof/closure.rs::evidence`는
각 4세대의 원본 채택 후보·개별 생성기 커밋·실제 컴파일 후손 작업을
확인하고, **A-off/B와 A2-off/B2의 2개 독립 대조 실험**에서 모두
정확히 일치하는 동결 holdout 능력 향상을 요구합니다.
A2 워커는 B 후보의 원본 결과를 `methodControlFeedback`으로
수신하고, `rsiMethodControlFeedbackExecuted` 실행 기록이 있어야 합니다.
중간 보고서·요약 JSON만으로는 성공 조건을 만족하지 않습니다.
실험은 비무작위이므로 관찰된 조건부 우위와 일반적 인과 증명은
구분합니다.

A2의 원본 B 피드백은 실제 모델 입력 프롬프트의 SHA-256과 함께
`rsiMethodControlFeedbackExecuted`에 보존되며, 완료 검사기가 원래
작업의 입력을 다시 해시합니다. 기존에 이미 통과했던 가드 사례의
보존만으로는 신규 B 능력 개선이라고 인정하지 않습니다.

정확도 포화 시에도 모델 개선이 정지하지 않도록
`rsi/proof/closure.rs::method_gain`은 **같은 동결 개선기 holdout에서
독립적으로 측정된 토큰·문맥·시간·품질 지표 개선**을 수용합니다.
`method_control.rs::matched_quality`는 기존 pass/fail 능력 외에
동일 baseline에서 측정된 실제 `quality` 차이를 검증하며,
서로 다른 기준 성과를 대조군인 것처럼 합치지 않습니다.


다음 실행에서 이어진 A→B 자동 대조도 누락하지 않도록
`rsi_closure::preceding_method`는 같은 캠페인의 직전 채택을 우선
검색하고, 없으면 **검증된 원본 소스 지문·브랜치·baseCommit·
선택 커밋의 동일성이 확인된 이어서 실행한 캠페인**에서만 이전
방법 A의 실제 영수증을 가져옵니다. 다른 작업의 유사 목표를
계보로 섞지 않습니다.

계속 발전하기 화면은 일반 캠페인에서 자동 등록된 **A 이전 대조 작업
ID**를 세대별 기록에 표시하며, 독립 재검증 화면에서는 A→B와
A2→B2의 **2개 대조 결과**를 각각 보여 줍니다. 자동 대조가 없는
예전 계보에서는 수동 대조가 필요할 때에만 실행 버튼을 제공합니다.

### 2026-11-13 — 실험 설계 원리의 재귀 진화

`src/engine/rsi/meta/experiment_evolution.rs`는
`planner/worker_plan.rs`가 **재컴파일된 후손 바이너리에서 실제 호출**하는
실험 전략 유전체입니다. 기존 `3세대마다 요인 실험` 고정 배분 대신,
직전 세대의 독립 재검증된 평가 결과와 작업 영수증에서
실험 수익·새로움·토큰 비용·퇴행 복구 가중치를 **계승·변이·재조합**해
개방 탐색(`explore`), 요인 실험(`factorial`), 역전이 복구(`repair`)를
선택합니다. 선택은 실제 `rsi_intervention_lab`·
`rsi_negative_transfer` 계획의 활성화와 수정 파일 범위를 바꿉니다.

유전체는 Rust `serde`의 엄격한 `Contract` 타입으로 전송되며,
`revision`에 내용 지문을 부여합니다. Coordinator
`execution/campaign.rs`는 실행 가능한 실험과 유전체 선택을 대조한 뒤
`rsiExperimentStrategy`를 전달하고, 후손
`execution/worker.rs`는 같은 계약을 다시 검사한 후
`rsiExperimentExecuted` 영수증을 남깁니다.
`lineage/history.rs`는 원본 작업에서 실행 여부를 다시 읽습니다.
실제 실행·독립 paired 평가가 검증된 기록만 다음 유전체의 부모나
재조합 기증자가 됩니다. `proof/closure.rs`의 독립 채택 증거와
고정 평가기는 **이 실험 선택 유전체로 수정할 수 없습니다.**

이 구조는 4세대 A→B→A2→B2라는 검증 예시와 무관하게 다음 세대의
실험 원리 자체를 업데이트합니다. 메타 정책·편집 유전체·실험
유전체의 계승·재컴파일은 이미 연결되어 있으나, 임의의 새로운
실험 연산자를 동적으로 발명하는 일반 실행기까지 완성된 상태는
아닙니다. 모델 가중치 변경·학습은 없습니다.

2026년 연구의 공통점: GEPA(2026, ICLR)의 실행 흔적을 활용한
반성·파레토 탐색, GEPA 2026년 7월의 복수 최적화기 결합,
Meta-Agent(2026)의 빠른 하네스 진화/느린 개선기 진화,
CoCoEvolve(2026)의 전체 진화 기록을 통한 메타 반성에서
**계층별로 제어하되 성과는 독립 평가하는 구조**를 참고했습니다.
코드를 그대로 복사한 것은 아닙니다.
- https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html
- https://gepa-ai.github.io/gepa/blog/archive/2026/
- https://github.com/canvas-org/meta-agent
- https://arxiv.org/abs/2610.12794


메타 유전체의 보상·실패 학습은 성공한 채택 세대뿐 아니라,
`lineage/history.rs`가 실제 `replayed_pair_metrics`로 증명한
**미채택·중립·퇴행 실험**에도 적용됩니다. 미실행·도구 호출만 있는
가상 기록은 학습하지 않습니다. 실패에서 실험 선택법이 변하지 않는
기존 성공 편향을 줄이기 위한 수정입니다.


### 이어서 실행해도 유지되는 메타 실험 계보

`rsi::lineage_history`는 원래 RSI 성공 검증에 쓰던
동일 브랜치·소스 지문·base commit·후손 선택 커밋 검사 경로를
메타 실험 학습에도 재사용합니다. 이후
`rsi_system::revalidated_history`가 이전 캠페인까지 실제 평가 영수증을
다시 계산해 `meta_experiment_evolution::propose`에 제공합니다.
같은 프로젝트를 재시작해도 실험 전략 유전체가 초기값으로 돌아가지
않으며, 엉뚱한 소스의 캠페인과 계보를 섞지 않습니다. 학습 자료는
실제 모델이 저술한 원본 후보와 동결 paired 평가가 확인된 작업으로
한정합니다.

실행 중 중첩 `videoState`를 얕게 덮어써 유료 원격 작업 ID와 원래 샷 계획이 없어지던 경로는 `video/runway.rs::checkpoint_video`로 고쳤습니다. 후손 소스 지도(`engine/rsi_system.rs`)에는 실제 그룹대화 비용 알고리즘과 영상 제작 모듈을 알고리즘·하네스·도구 역할로 추가해, 이후 개선기의 탐색 대상으로 연결했습니다. 두 변경 모두 소스만 검토했으며 테스트/빌드는 실행하지 않았습니다.

## 🤖 2026-11-14 — 자율 에이전트 25점 기준 통합 업그레이드

`Goals.tsx`의 **자율 목표 실행**에서 주인이 목표와 검증 조건을 입력하면 `/api/goals` → 영속 `goal-job-<id>` → `goals.rs::run`이 계획·실제 도구 실행·독립 검토·실패 재계획·완료 검증을 관리합니다. 사이드바에서 목표 상태, 계획 이력, 검증 조건별 실제 성공/실패, 파일 SHA-256, 도구 기록과 작업 세션을 볼 수 있으며 대기 중 계획 수정, 작업 취소, 외부 결과 확인 후 수동 재개가 연결됩니다.

- **최종 완료 게이트**: `goals/report.rs`는 최종 저장 시점에 수락 조건을 다시 검사합니다. 단순 모델 완료 선언이나 이미 바뀐 파일의 과거 상태로 성공하지 않습니다. `finalReport`는 실제 파일 지문, 도구 성공 기록 및 미해결 조건을 포함합니다.
- **서브에이전트 공동 검증**: `verification.rs`는 `delegate_task`/`task_result`의 완료된 자식 도구 영수증을 상위 목표의 증거에 연결합니다. 쓰기 호출은 실제 `written:true`일 때만 행동으로 인정합니다.
- **장기 복구**: 이전 `needs_review`·실패 작업의 부작용을 자동 재실행하지 않습니다. 주인이 확인 후 `/api/goals/:id/resume`를 호출하면 원본 목표·세션·성공 증거·계획을 남긴 새 영속 작업으로 이어갑니다.
- **모델 비용 관리**: 1개 부모 세션에서 위임을 무제한 생성하지 않도록 작업 수와 활성 수를 제한합니다. 기본 15만 토큰의 목표별 실행 예산을 설정하고, 실제 세션 사용량 및 제공자 보고 프롬프트 캐시 재사용량을 목표 보고서와 UI에 기록합니다. 이 제한은 다음 실행 턴 진입 시 확인하며 계획·검토 호출을 포함한 실제 결제 상한은 아닙니다. 2026년 Codex·Claude Code식 에이전트 목표를 향한 실제 실행 경로이며, 모델 자체를 학습하거나 평가 결과를 위조하지 않습니다.

상세 **API·프론트·도구·목표 평가·후손 연결 지도**: [서브4 자율 에이전트 25점 업그레이드](docs/SUB4-AUTONOMOUS-AGENT-2026-11-14.md). 이번 변경은 **코드 커밋 및 테스트 소스 작성**까지 수행했고, 컴파일·테스트·CI·실제 유료 실행은 하지 않았습니다. 기존 심사 점수 50점에 대한 재채점은 별도입니다.

### 서브4: 코딩 에이전트 Git 변경 기록 · 위임 취소 · 영상 클립 검수 (2026-11-14)

- 목표 실행 `goals/git.rs`는 실행 전 Git HEAD/작업트리 상태를 동결하고 최종 변경 파일·bounded diff 미리보기를 `finalReport.gitChanges`에 기록한다. 실제 변경 attribution은 `execution_operation` 증거로 별도 판단한다. `Goals.tsx`에서 볼 수 있다.
- `state.rs::cancel_delegated_descendants`는 목표나 세션을 중단할 때 실행 중인 하위 위임 에이전트까지 영속 취소 epoch와 job 중단을 전파한다.
- 유료 Runway/Kling 영상 생성은 기본 설정 `runwayQualityGate=true`에서 FFmpeg 옆의 `ffprobe` 존재를 **과금 전** 확인하고, 내려받은 클립의 영상 스트림·길이·화면비·최소 해상도·요청된 오디오를 점검한다. 불량 클립 후속 과금을 막아 `needs_review`로 보존한다. 병합 완료 영상도 최종 기술검수를 수행하고 UI에 측정치를 표시한다. 이는 내용적·미학적 품질을 자동 판정하는 기능이 아니다. FFprobe가 없으면 검수를 명시 해제해야 기존 경로를 사용할 수 있다.
- `src/engine/rsi_system.rs::COMPONENTS`가 새 실행 경로(목표 검증·Git 결과·기술적 영상 QA)를 후손 코드 역할 지도에 포함한다. 원본 모델 가중치 학습이나 실모델 RSI 성과의 허위 영수증은 없다.


### 자율 코딩 목표 · Git 작업 격리·검토·적용 (서브4, 2026-11-14)

`isolateWorktree:true`인 목표는 원본 저장소가 아닌 별도 Git worktree에서 실행됩니다. 실제 도구의 기준 경로(`chat/runtime/tool_execution.rs`), 서브에이전트 상속(`goal_git::workspace_for_session`), 목표 파일 검증(`verification::*_in`)과 완료 보고(`goals/report.rs`)에 연결했습니다. `/api/goals/:id/changes`는 변경 파일·새 파일 내용·검토 해시를 보여주며, 사용자가 해당 해시로 명시적으로 승인한 경우만 `/api/goals/:id/apply`가 원본 Git HEAD가 그대로일 때 fast-forward 합니다. `/discard`는 미반영 worktree만 폐기합니다. 실행 Git 경로가 설정되지 않았거나 격리 준비에 실패하면 원본 checkout으로 자동 복귀하지 않습니다.

관련 구조·한계·근거: [docs/SUB4-ISOLATED-AGENT-2026-11-14.md](docs/SUB4-ISOLATED-AGENT-2026-11-14.md). Git 격리는 OS 보안 샌드박스가 아니며, 승인된 임의 명령과 외부 서비스 호출은 별도 정책을 따릅니다.

### 서브4 v3 (2026-11-14) · 멀티샷·병렬 조사·운영 기반 RSI 학습

ComfyUI 영상에 `cinematic:true` 옵션이 연결됐습니다. `video/cinematic.rs`의 장면별 storyboard/작업/ffprobe/검증된 클립 재사용/기술 불량 샷 최대 1회 재생성 → FFmpeg 1080p 편집 및 최종 기술 검수로 이어집니다. `video/comfy.rs`를 분리해 원격 제출·이력 조회·다운로드를 담당합니다. 결과 UI는 개별 장면 기술 검수와 재시도·재사용 기록을 제공합니다. 시각적 미학·실제 GPU 비용을 측정한 것은 아닙니다.

`tools/batch.rs::delegate_batch`는 최대 4개 읽기 전용 병렬 조사 에이전트를 실제 작업 큐에 생성합니다. 모델 도구 목록과 실행기·MCP 경로에서 쓰기 권한을 봉쇄하며, 부모 목표의 Git worktree 상속과 결과 수집을 유지합니다.

`goals::finish` → `goal_feedback::record` → `rsi/planner/focus.rs::generation_context`는 실제 목표 성공/실패 및 검증 정보를 다음 자기개선 모델의 조사 입력으로 전달합니다. 이 운영 관찰은 독립 paired RSI 평가·채택 성공과 분리합니다. 전체 호출 경로와 제한 사항은 [SUB4-V3-VIDEO-RSI-AGENT-2026-11-14](docs/SUB4-V3-VIDEO-RSI-AGENT-2026-11-14.md)를 참조하세요.

#### 실행 중 사용자 계획 수정 (서브4)

`/api/goals/:id/steer`는 예상 plan revision을 동봉한 사용자의 새 계획을 기록한다. `goal_steering::pending`이 다음 executor 턴에서만 적용하고, 기존 계획으로 완료 처리되지 않도록 `goals::run`·`finish`에서 미반영 revision을 검사한다. `Goals.tsx`는 최신/반영 계획 상태를 표시한다. 완료 판정과 권한/검증 조건은 불변이다. 세부 경로: [서브4 v3 문서](docs/SUB4-V3-VIDEO-RSI-AGENT-2026-11-14.md).


### Goal-wide helper token accounting (2026-11-14)

`GoalControl::complete` now uses `llm::complete_for_session` for planner, reviewer and replanner. Their provider-reported usage (or an explicitly marked estimate when absent) is audited under the same goal session as executor turns. Before **each helper call**, the shared `goal.tokenBudget` is checked against `db::session_usage`; `0` remains unlimited. Final reports expose the scope and explicitly mark the limit as **not an exact billing cap** (an in-flight call may overrun, and missing usage cannot be estimated reliably). See [goal execution architecture](docs/SUB4-AUTONOMOUS-AGENT-2026-11-14.md). A mock HTTP regression is included but was not run in this connector-only edit environment.


### Isolated goal worktree lifetime across delegated descendants (2026-11-14)

`goal_git::rollback` and `goal_git::apply` now share ancestry-aware active child job detection. A still-running grandchild or cyclic/deep unresolved session ancestry blocks worktree removal/integration. See [isolated agent lifecycle](docs/SUB4-ISOLATED-AGENT-2026-11-14.md). Regression cases are committed; runtime CI remains unverified.


### 2026-11-14 서브4 연결채널 통합 / 자기개선 비방해형 표시

**연결 채널** 아래 채널 연결 · MCP/스킬 · 역량 마켓 3개 탭을 제공한다(`web/src/pages/Connections.tsx`). 기존 설치·검색·삭제·계정 연결 API는 `ExtensionsPage`, `CapabilitiesPage`에서 그대로 실행되고 `#/extensions`, `#/capabilities` 구형 경로도 연결 채널의 해당 탭에서 열린다. 서브4 독립 **자율 목표**, **기억** 메뉴는 유지한다.

**자기개선**(`SelfImprovement.tsx`)은 작업 상태를 페이지 내부에 표시하고 RSI 상태 조회 실패로 `role=alert`를 반복 발송하지 않는다. 완료/중단 상태의 반복 polling을 중지하고 활성 실행만 5초 주기로 확인한다. 사용자가 요청한 검증/중단/적용 오류·결과는 화면과 서버 작업 기록에서 유지한다. 정적 연결 확인까지 수행; 실제 브라우저 E2E와 빌드 검증은 별도다.


### 2026-11-14 기억 직접 편집 (서브4)

`web/src/pages/Memory.tsx`의 사용자가 직접 작성한 활성 기억은 카드에서 수정하고 저장할 수 있다. `POST /api/memories/:id/edit` → `api/collections.rs::action` → `Store::patch_if`로 이전 텍스트를 비교해 원자적으로 수정한다. scope/agentId/검증/출처와 저장 ID를 유지하고 변경 시각을 기록한다. 학습 추출 기억은 원본 증거를 유지하기 위해 UI에서 직접 편집을 제공하지 않으며, 서버도 해당 수정 요청을 거부한다. 저장 후 클라이언트 기억 목록을 재조회해 새 검색에 반영한다. 백엔드 회귀 테스트에 충돌·출처 격리·scope 보존을 기록했다(실행/CI는 별도).


### 서브4 프론트 전체 메뉴 · API · 엔진 검증 지도

2026-11-14 심판 이슈 #8을 반영한 **4개 카테고리 / 15개 메뉴(자율 목표 포함)**, 연결 채널 3개 탭, 기억 편집·RSI 알람 조정, 화면→API→엔진→산출물·복구 경로, 실제 실행 전/후 검증 구분은 [SUB4-FRONTEND-COVERAGE-2026-11-14.md](docs/SUB4-FRONTEND-COVERAGE-2026-11-14.md)에 기록했다. 해당 지도는 정적 소스 검토와 12개 JS assertion 통과를 기준으로 작성했으며 브라우저 E2E·Rust/TS 빌드는 미실행이다.


### 2026-11-14 서브4 유료 영상 요금 추정·승인·재시도 원장

`VideoPage` → `POST /api/video/quote` → `video::paid_quote` → `runway::quote`로 공개 요금표 기준 **원화 견적·클립 수·모델·오디오**를 계산한다. `POST /api/video/jobs`는 사용자 승인과 최대 추정 크레딧을 검증하며 실제 유료 원격 요청 직전에 가격을 다시 확인한다. 작업의 시도 횟수·잠재 과금 노출은 영속 체크포인트에 남기고 **실제 청구 비용과 구별**한다. 외부 과금/환불 검증은 아직 미실시다. 상세한 공식 가격 근거·회귀 검사는 [SUB4-VIDEO-COST-2026-11-14.md](docs/SUB4-VIDEO-COST-2026-11-14.md)를 참고한다.


### 2026-11-14 · 서브4 원자적 목표 생성과 재개

`POST /api/goals`가 목표를 저장한 다음 작업 등록에 실패하면 `status=queued`인 유령 목표가 남던 위험을 제거했다. 사전 생성한 `goalId`와 결정적인 `jobId`를 SQLite `Store::transaction` 한 번으로 저장한다. `POST /api/goals/:id/resume` 역시 `Store::transaction_if_status`로 상태 CAS와 새 작업 등록을 묶어 동시 재개 충돌이나 기록 없는 작업을 차단한다. 두 경우 모두 **트랜잭션 성공 이후에만** SSE 이벤트·감사 기록을 발행한다. 테스트에 트랜잭션 롤백과 동시 재개를 추가했다. UI/API 응답, 기존 중단/격리/토큰 예산의 계약은 유지된다. 외부 실모델 완주 여부는 별도 검증 대상이다.


### 2026-11-14 · 서브4 재계획 호출 비용 절감·실진전 증거

`src/engine/goals/progress.rs`는 리뷰어 말·실행자 서술을 진전으로 계산하지 않는다. 매 턴 전후의 검증 조건(`acceptance_gaps_in`)과 실제 성공한 변경 도구의 **새 인수 영수증**을 비교한다. 검증 조건을 충족했거나 새로운 성공 변경이 있는 턴은 `stalled=0`, 동일 명령 반복·읽기·실패만 반복한 턴은 정체 횟수를 증가시킨다. 신규 성공 도구가 있어도 같은 턴에 도구 오류가 있으면 검증 조건이 실제로 줄지 않은 한 정체로 계산한다. 모델 재계획은 **연속 정체가 `replanAfter`에 도달한 경우**에만 호출한다. 작업 체크포인트·goal 리뷰 이력·진전 이벤트에는 새 성공 작업 수, 해소된 검증 조건 수, 실패 수를 남긴다. Rust 모듈 단위 회귀 테스트가 있으며 실모델 완주율 측정과 구분된다.


### 2026-11-14 · 서브4 목표의 DB 영수증·검증 신선도 강화

`run_quiet_outcome`은 스트림의 `tool_end`를 곧바로 완료 증거로 받아들이지 않는다. `operationId`를 통해 동일 세션·호출·도구 이름으로 완료된 `execution_operation` 저장 행을 찾아야 성공 증거가 되고, SQLite 영수증의 `runId/opSeq/startedAt/finishedAt` 순서를 유지한다. 실패·저장 누락·다른 세션·변조된 스트림 이벤트는 제외한다. 목표의 `command_succeeded` 검증은 성공 후 실제 소스 수정이 있었으면 과거 테스트를 불인정하고 재검증을 요구한다. 여러 실행 간 타임스탬프와 동일 실행의 `opSeq`를 대조하며 순서 자료가 없으면 보수적으로 실패 처리한다. 모의 영수증·실행 순서 회귀 테스트를 추가했으며 실모델 완주 실적을 의미하지 않는다.


### 2026-11-14 · 목표 도구 부작용 미확정 시 턴 성공 여부와 무관하게 일시정지

각 실행 턴 직전에 `execution_operation` 영수증 ID를 보관하고, 턴 이후 **새로 발생한** 목표/하위 세션의 `running`, `interrupted`, `outcomeUnknown`, `effectState=needs_review|unknown`를 검색한다. 신호가 있으면 모델 응답이 성공했더라도 즉시 `needs_review`로 전환해 후속 도구를 실행하지 않고 원인 영수증 ID를 목표에 저장한다. 기존에 소유자가 확인한 과거 미확정 영수증이 정상적인 읽기 전용 재개를 영원히 방해하지 않도록 **신규 ID만 선별**하고, 다른 채팅·그룹대화 세션의 영수증은 제외한다. 성공한 도구 영수증은 검토 상태에 유지한다. 실제 외부 API의 정확한 결과는 운영자가 별도 확인해야 하며 자동 재전송되지 않는다.


### 2026-11-14 · 서브4 목표 실행 성과 원장

사용자가 요구한 **실제 완수율·토큰 비용을 측정할 수 있는 경로**를 목표 화면에 설치했다.

- `GET /api/goals/metrics` → `api/goals.rs::goal_metrics` → `engine/goals/metrics.rs::snapshot` → `Store::try_all("goal"|"job")` + 고정 audit rowid 범위의 `Store::model_usage_for_session_in_window`. API는 읽기 전용이며 어떤 LLM·영상 유료 API도 호출하지 않는다.
- 내부 성공은 목표 `status=completed` **그리고** `finalReport.accepted=true` **그리고** 동일 목표와 연결된 `job.kind=goal/status=completed`가 있어야 한다. 검사 없는 완료·실패한 작업은 성공으로 세지 않는다.
- 상태가 완료·실패·취소·검토 대기인 목표의 성공 비율을 분모와 함께 표시한다. 대기·실행 중 목표는 별도 구분한다. 성공 0건이면 완료 1건당 토큰은 `null`이다.
- 기록된 전체 목표 세션의 모델 호출·토큰·제공자 보고 캐시 사용량·추정 호출 수·재계획 수를 합산한다. **실패와 진행 중 목표에 지출한 토큰도 포함**해 성공한 결과만 선택적으로 세지 않는다.
- 목표 세션·사용량이 누락되거나 같은 세션을 여러 목표가 공유하면 `completeAttribution=false` 및 1건당 토큰 `null`로 표시한다. 공급자 청구 원화 비용은 제공되지 않았으므로 `actualBilledKrw=null`이며 절감률/가성비를 임의 산출하지 않는다.
- 프론트 `GoalsPage`는 15초 간격으로 실측 원장을 새로고침하되 독립 벤치마크 실적이 아닌 **로컬 운영 관측**이라고 명시한다.
- 계약 회귀: 목표·작업·검증 세 조건, 진행 중 분모 제외, 실패/추정 토큰 포함, 캐시 보고 분리, 측정 누락 시 1건당 비용 비표시. 제품 사용 전/후 성능 비교에는 동등한 작업·모델·예산의 재실행과 외부 독립 검증, 실청구 영수증이 여전히 필요하다.

- 격리 Git 목표에서 최종 검증이 통과해도 `gitWorktree.applied=true`가 아니라면 납품 성과로 세지 않는다. `internalAcceptedGoals`, `deliveredToSourceGoals`, `acceptedAwaitingGitApply`를 별도 제시한다. 아직 실행하지 않은 `queued` 목표는 사용량 로그 부재가 정상이므로 비용 누락 경고에서 제외한다.


### 2026-11-14 · 전 제품 통합 #9: 에이전트 → 목표/워크플로 → 실제 작업 원장

- `Agents.tsx`의 각 에이전트에서 **자율 작업 실행**을 제공한다. `POST /api/agents/:id/goal`은 저장된 에이전트만 허용하고 서버에서 실행자를 고정한 뒤 기존 `goals::create_goal`의 원자적 목표·작업 트랜잭션을 재사용한다. 작업·예산·Git 격리 설정은 사용자에게 노출하며 Goal 화면에서 실제 실행/복구/검토가 이어진다.
- `GET /api/agents/portfolio`는 `agent`·`goal`·`job`·`workflow` 영속 기록에서 에이전트별 목표 수·실제 완료 검증·원본 반영/비격리 결과·실행/검토 대기·워크플로 참여 단계를 추적한다. `Store::session_usage` 기반의 에이전트 소유 목표 사용량을 별도 표시한다.
- 여러 모델이 공유하는 워크플로 세션의 토큰은 개별 에이전트 비용으로 억지 배분하지 않는다. UI에 모델 독립 성능, 실제 원화 청구액으로 해석할 수 없는 내부 실행 자료임을 명시한다.


### 2026-11-14 · 전 제품 통합 #9: 역할별 공동 산출물·작업 검증

- `WorkflowsPage` 단계마다 **실제 도구 영수증 필수**를 선택할 수 있다. 실행기 `engine/workflows.rs`는 `run_quiet_outcome`을 재사용해 세션의 완료된 SQLite `execution_operation` 영수증으로만 실행/변경 증거를 계산하며 모델 서술을 증거로 취급하지 않는다.
- 필수 단계에서 성공 행동 영수증이 없거나 미확정 외부 작업이 남으면 `needs_review`로 중단하고 **다음 에이전트를 호출하지 않는다**. 실행 중인 단계 체크포인트를 남겨 자동 재실행을 방지한다. 표준 읽기/조사 단계는 필수 옵션 없이 기존 실행 경로를 사용한다.
- 매 단계 `stepId`, 담당 `agentId`, 응답 메시지 ID, 성공 도구 ID, 변경/실행 영수증 목록, 실패 도구를 함께 영속화한다. 최종 결과는 전체 단계의 공동 산출물과 동일 세션 사용량을 반환한다. 워크플로 UI에서 실제 단계 결과를 펼쳐 볼 수 있다.
- `{{step:1}}` 같은 1기반 특정 단계 결과 참조는 1회 치환하며 지난 단계의 전체 문맥을 매번 반복 붙이지 않는다. 재귀 치환·타 단계 임의 확장은 없다.
- 실행 결과는 내부 영수증이며 공급자 유료 청구액, 독립 실모델 평가/완수 증거가 아니다.


### 2026-11-14 · 전 제품 통합 #9: 워크플로 → 자동화 → 결과

- `OperationsPage`의 자동화 생성/편집에 `workflow` 유형을 추가했다. 등록 워크플로 ID와 `{{input}}` 입력을 선택해 예약 후 실제 `engine/jobs::schedule_automations`가 기존 워크플로 작업 엔진으로 전달한다. 작업 상태·출력은 자동화 목록 및 작업 큐에서 이어진다.
- 예약은 `automation.nextAt` **CAS + 실행 job 생성**을 동일 SQLite 트랜잭션으로 커밋한다. 예약 시각이 달라진 다른 스케줄러는 중복 작업을 만들지 않는다. 안정된 `automation:{id}:{dueAt}` 작업 ID로 프로세스 재시작 직후 같은 예약을 다시 등록하지 않는다. 앞선 작업이 `queued/running`이면 겹치는 새 작업을 시작하지 않는다.
- `collections::save`는 자동화의 workflowId가 실제 등록됐고 입력이 길이 제한에 맞는지 서버 검증한다. 프론트만 존재하는 가짜 자동화가 아니다. 관련 원자성·중복 방지 회귀 테스트 추가.


### 2026-11-14 · 전 제품 통합 #9: 홈 설정→작업→복구, 개발자 격리 코딩

- `HomePage`는 실제 연결된 모델/에이전트/워크플로/예약/Goal 상태에서 다음 작업의 진입점을 결정한다. 준비되지 않은 모델 연결 → 에이전트 생성 → 워크플로 구성 → 자동화 등록으로 이어지고, 미확정 목표는 최우선으로 검토 화면에 보낸다. 내부 검증과 Git 원본 반영 결과·모델 기록 토큰도 운영 집계 API에서 보여 준다.
- `DeveloperPage`는 소스 검사/테스트/빌드와 별도로 **검증 기준이 있는 에이전트 코딩 작업**을 시작한다. `POST /api/dev/agent-goal`이 설정된 개발 소스 경로와 엔진 작업공간이 같은 깨끗한 Git HEAD인지 **서버에서 재검증**하며, 불일치 시 다른 저장소로 쓰는 일을 차단한다. 등록된 에이전트를 지정하고 `command_succeeded` 검증이 필요한 격리 Goal을 생성한 후 기존 도구·검증·작업공간 변경 승인 경로로 넘긴다. 개발자 화면만 바뀐 것이 아니라 코드 수행 엔진과 연결된 진입점이다.
