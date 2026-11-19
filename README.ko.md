# DOMINION — 서브4 Rust 실행 하네스

[English](README.md) · [구조](docs/ARCHITECTURE.md) · [Rust SDK](docs/RUST_SDK.md) · [TypeScript SDK](sdk/typescript/README.md)

도미니온 서브4는 **Rust 단일 실행 커널**과 **메인 브랜치 최신 React/Vite 프런트**를 결합한 독립 개발 브랜치입니다. 코드 실행, 에이전트 위임, 모델 호출, 도구 승인, 복구, 실행 증거는 Rust 엔진이 담당합니다.

- **최신 메인 화면:** `main@93dcee8`의 React 파일을 기준으로 동기화했습니다. 9명 방의 장시간 502 문제를 피하기 위해 메시지 전송 API만 서브4의 비동기 경로로 연결합니다.
- **머니 기능 제거:** 돈벌이 탐색 메뉴, Rust 기능 모듈, SDK 메서드, API, 수집 스케줄러, 관련 테스트를 삭제합니다. 기존 사용자 데이터베이스 `money.sqlite` 파일은 자동 삭제하지 않습니다.
- **대화·에이전트·방:** 사용자 메시지와 에이전트 응답을 영속 저장하며, 방에서는 같은 `clientMessageId`의 중복 실행을 방지합니다.
- **모델별 비용:** 실제 제공자의 입력·출력·캐시 토큰을 분리해 기록합니다. 세션 기억과 서버측 계산 재사용은 서로 다른 개념입니다.
- **API 연결:** 메인 UI에서 새로 추가된 기능 가운데 서브4 Rust 어댑터가 없는 기능은 명확한 오류로 응답합니다. 메인 백엔드 전체를 가져온 것은 아닙니다.

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
