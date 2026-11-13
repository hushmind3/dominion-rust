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
