# DOMINION — 서브4 Rust 실행 하네스

[English](README.md) · [구조](docs/ARCHITECTURE.md) · [Rust SDK](docs/RUST_SDK.md) · [TypeScript SDK](sdk/typescript/README.md)

도미니온 서브4는 **Rust 단일 실행 커널**과 **메인 브랜치 최신 React/Vite 프런트**를 결합한 독립 개발 브랜치입니다. 코드 실행, 에이전트 위임, 모델 호출, 도구 승인, 복구, 실행 증거는 Rust 엔진이 담당합니다.

- **최신 메인 화면:** `main@93dcee8`의 React 파일을 기준으로 동기화했습니다. 9명 방 메시지 전송은 서브4의 비동기 경로로 연결하고, 중복된 업체 탐색·연결 탭은 하나로 통합했습니다. Hugging Face GGUF/MLX 공개 검색·상세 조회·실제 파일 다운로드·작업 재개·설치 목록은 Rust에 연결했습니다. GGUF는 로컬 Ollama 실행기와 기존 Rust 모델 라우트로 연결하며, MLX 네이티브 실행기 설치와 macOS/Windows 패키지 배포는 별도 작업입니다.
- **대화·에이전트·방:** 사용자 메시지와 에이전트 응답을 영속 저장하며, 방에서는 같은 `clientMessageId`의 중복 실행을 방지합니다. 동기·백그라운드 전송의 입력 승인 경로를 통일하고 같은 사용자 턴의 에이전트·기억 반복 조회를 줄였습니다. 채팅 수동 컨텍스트 압축은 원본 대화를 남기고 오래된 기록의 근거 발췌를 저장합니다. 수동 학습은 사용자의 원문 발언만 명시적 출처와 함께 기억으로 저장합니다.
- **모델별 비용:** 실제 제공자의 입력·출력·캐시 토큰을 분리해 기록합니다. 세션 기억과 서버측 계산 재사용은 서로 다른 개념입니다.
- **실행 안전성:** 모델이 작업 완료(`stop`)와 실행 가능한 도구 호출을 동시에 반환하면 Rust 실행기 진입 전에 거부합니다. 이미 유료 추론이 발생했을 수 있어 자동 재호출하지 않습니다.
- **P3 원격 Worker:** 별도 장치의 Rust Harness로 읽기 전용 조사 작업을 위임합니다. 작업별 고정 ID, SQLite 중복 접수 차단, 전용 인증 토큰, 원격 실행 상태·원화 비용 영수증을 사용합니다. 원격 Worker의 파일 수정·재귀 위임은 Rust 정책에서 금지합니다. 기존 UI 메뉴는 변경하지 않았습니다.
- **API 연결:** 메인 UI에서 새로 추가된 기능 가운데 서브4 Rust 어댑터가 없는 기능은 명확한 오류로 응답합니다. 메인 백엔드 전체를 가져온 것은 아닙니다.

[중복 전수 점검·정리 내역](docs/SUB4_DEDUP_AUDIT_2026-11-20.md) · [폐기 기능·설치 잔재 전수 점검](docs/SUB4_RETIRED_CODE_AUDIT_2026-11-20.md) · [프런트·이전 로그인 잔재 정리](docs/SUB4_FRONTEND_AUTH_CLEANUP_2026-11-20.md)

## P3 원격 Worker (기본 비활성화)

원격 실행 장치에는 32자 이상의 `DOMINION_REMOTE_WORKER_ACCEPT_TOKEN`을 설정하고 기존 Rust 서버를 HTTPS로 접근 가능하게 구성합니다. 호출 장치에는 `DOMINION_REMOTE_WORKER_URL=https://원격주소`와 같은 값의 `DOMINION_REMOTE_WORKER_TOKEN`을 설정합니다. `remote_worker_consult`는 기존 Rust 도구 승인·저널을 통과해야 하며, 원격 API는 사용자 전체 권한 토큰 대신 별도의 제한된 토큰을 요구합니다. 네트워크 결과가 불분명하면 자동 재실행하지 않으며 원격 실행 영수증을 확인해야 합니다.

현재 원격 Worker는 **읽기 전용 조사 전용**입니다. 원격 파일 변경·자동 패치 병합·실기기 간 성능 측정·HTTPS 배포 설정은 포함하지 않습니다. 원격 사용량은 원화로 별도 기록하며 로컬 예산에 합산되지 않습니다.

## 실행·점검

Rust stable 및 Node.js 24.12+ 기준:

```bash
git checkout dominion-sub4
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
node --experimental-strip-types --test web/tests/*.test.mjs
cargo check --locked --all-targets
cargo test --locked --lib
```

개발 실행: `cargo run -- --home .` 및 별도 터미널에서 `npm run dev --prefix web`. 기본 서버 주소는 `127.0.0.1:4317`(Rust), `127.0.0.1:5173`(Vite)입니다.

## CI

[서브4 프런트·백엔드 개발 CI](.github/workflows/sub4-dev-rust.yml) · [macOS/Windows 수동 점검](.github/workflows/check.yml) · [9명 방 3턴 검증 기준](docs/SUB4_ROOM_FANOUT_MEASUREMENT_2026-11-19.md)

**유의:** 메인과 서브4는 Rust 백엔드 구조가 다릅니다. 프런트의 전체 소스 동기화가 새로 추가된 메인 서버 기능까지 구현됐다는 뜻은 아닙니다. 실행 성공 여부와 기능 호환 범위는 해당 커밋 CI 및 API 테스트 결과로 구분합니다.

## CLI 인증 범위

공식 Codex CLI 기기 인증은 로컬 서버에서만 실행하며 토큰을 웹에 전달하지 않습니다. CLI 로그인 상태가 성공이어도 서브4 Rust 실행 커널의 모델 제공자/API 연결을 자동 생성하지 않습니다. 실제 모델 호출에는 별도로 등록된 Rust 제공자 연결이 필요합니다.

## 도미니언 클라우드 계정 및 모델 중계 (서브4)

`main`의 Cloudflare Worker와 D1 스키마를 독립 복제했습니다. Rust 에이전트·Run·Room 엔진과 로컬 모델/BYOK는 그대로 유지됩니다. 웹 모델 허브의 DOMINION Cloud 카드에서 회원 세션을 받아 `/api/providers`에 선택한 모델을 등록하면 기존 Rust OpenAI 호환 제공자 라우터가 Worker의 `/v1/chat/completions`을 호출합니다. 상위 모델 API 키는 Worker에만 보관합니다.

기본 Worker 주소는 `https://dominion-cloud.dominion-hushmind.workers.dev`이고, 스테이징 주소는 `VITE_DOMINION_CLOUD_URL`로 재설정합니다. Google의 공개 웹 클라이언트 ID는 새 Worker `GET /v1/config`에서 읽고, 구버전 Worker 또는 별도 스테이징은 `VITE_DOMINION_GOOGLE_CLIENT_ID`로 설정할 수 있습니다. Worker의 `ALLOWED_ORIGIN`은 프런트 URL과 정확히 일치해야 합니다. 로그아웃은 Worker 토큰 폐기를 요청하고 로컬 Rust 제공자의 기존 세션 키를 지운 뒤 비활성화합니다.

**배포 구분:** 이 브랜치의 Worker 소스가 메인 운영 Worker에 자동 배포되지는 않습니다. 클라우드 설정을 변경하지 않고도 Rust CI/Worker 모의 D1 테스트는 실행됩니다. Cloudflare 계정의 D1·Workers AI 바인딩과 OAuth 앱 설정을 확인한 다음 별도 배포가 필요합니다. 무료 모델은 공급자·하루 사용량 한도 내에서만 동작하며, 유료 모델은 사전 충전액·본인확인·운영자 요금/예산 구성과 상위 공급자 키가 있어야 합니다. 현재 결제 서비스가 상용으로 완주되었다고 표시하지 않습니다.

## DOMINION Cloud 통합 회원·모델

메인에서 구현한 Cloudflare Worker·D1 회원 계정, Google/카카오/네이버 로그인, 무료 모델 및 TossPayments Pro 코드가 서브4에도 들어 있습니다. 로그인 후 선택한 클라우드 모델을 **서브4 Rust 제공자**로 등록하므로 자체 SDK·실행 커널과 BYOK·로컬 모델을 그대로 사용할 수 있습니다. 모델 공급사 키는 Cloudflare 비밀 설정에 보관합니다.

이번 변경에는 도구 호출 전달, 실제 토큰 사용량 정산, 동시 호출 운영자 예산의 원자적 예약, 프런트 선불 잔액 표시 및 결정론적 회귀 테스트가 포함됩니다. 무료 모델 호출에서 공급자 결과가 불확실한 연결 오류·429·5xx는 사용량 한도를 반환하지 않으며, 자동 대체 모델에 중복 전송하지 않습니다. 명확한 검증 거부에 한해 한도를 환급합니다. Pro 30일 이용권과 유료 API 선불 크레딧은 별도이며 고객 결제·충전·실계정 호출·D1 운영 DB 마이그레이션은 CI에서 수행하지 않습니다.
