# 설치와 첫 사용

DOMINION의 첫 화면은 채팅이다. 모델을 연결하고 작업을 맡기면 대화·도구 실행·결과·실패·사용자 수정이 로컬에 남는다. 자동 학습이 켜진 채팅은 관련 기억과 경험을 다음 작업에 재사용한다. 시스템 소스 자기개선은 별도 실행 예산 안에서 이 사용 경험을 근거로 진행한다. 사용량이나 데이터 수만으로 성능 향상을 보장하지 않는다.

## 무엇을 설치해야 하나

| 용도 | 필요한 것 |
|---|---|
| 배포된 데스크톱 앱으로 채팅·도구 사용 | 자신의 OS에 맞는 설치 파일, API 모델 연결 또는 로컬 모델 실행기 |
| 소스에서 실행 / 코드 자기개선 | Git, Rust stable와 Cargo, Node.js **24.12 이상**과 npm, OS C/C++ 빌드 도구, 이 저장소의 소스와 설치된 프론트 의존성 |
| API 모델 | 해당 제공자의 API 키 또는 지원되는 로그인. 앱의 모델 관리에서 등록 |
| 로컬 모델 | 선택한 로컬 실행기와 다운로드한 모델. 예: Ollama 제공자 등록. API 사용료는 없지만 컴퓨터 자원과 전력은 사용 |
| 네이티브 설치 파일 직접 만들기 | 위 개발 환경과 [Tauri의 OS별 사전 요구사항](https://v2.tauri.app/start/prerequisites/) |

Rust와 Node.js는 완성된 데스크톱 앱의 일반 채팅 실행에 필요한 별도 런타임이 아니다. **자기 소스를 수정·빌드·검증하려면 개발 환경이 필요하다.** SQLite는 Rust 바이너리에 포함되므로 별도 DB 서버를 설치하지 않는다. 현재 자동 RSI 평가기는 Node.js로 실행하며 Bash·WSL·Python을 요구하지 않는다.

다운로드: [Git](https://git-scm.com/downloads/), [Rust/rustup](https://rust-lang.org/tools/install/), [Node.js](https://nodejs.org/en/download), [Ollama](https://ollama.com/download).

### macOS

터미널에서 `xcode-select --install`로 Apple Command Line Tools를 설치한다. Git·Node.js·Rust를 설치하고 새 터미널에서 `git --version`, `node --version`, `npm --version`, `cargo --version`을 확인한다. Rust는 rustup 기본 stable 툴체인을 사용한다.

### Windows

Git for Windows, Node.js, rustup을 설치한다. Rust 설치 안내에 따라 Visual Studio Build Tools의 **Desktop development with C++**와 Windows SDK를 설치하고 MSVC 툴체인을 사용한다. 새 PowerShell 또는 개발자 터미널에서 위 네 명령을 확인한다. 설치형 데스크톱 앱은 WebView2 Runtime을 사용한다. C++ 도구와 WebView2의 자세한 항목은 [Tauri 공식 안내](https://v2.tauri.app/start/prerequisites/)를 따른다.

프로젝트와 캐시는 로컬 쓰기 가능한 폴더에 둔다. 회사 보안 정책이나 동기화 프로그램이 실행 파일 교체를 막는 위치는 피한다.

## 소스에서 실행

macOS 터미널과 Windows PowerShell에서 동일하게 실행한다.

```text
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
```

브라우저에서 `http://127.0.0.1:4317`을 연다. 첫 화면은 새 채팅이다.

1. 모델 관리에서 API 또는 로컬 제공자를 연결한다.
2. 채팅에서 모델을 고르고 실제 작업을 맡긴다. 도구 실행과 자동 학습은 설정에서 조절한다.
3. 코드 자기개선은 개발자 설정에 이 소스 폴더를 연결한 뒤 자기개선 화면에서 시작한다. 제공자·모델을 고르고, 필요하면 고급 설정에서 예산과 평가를 선택한다.
4. 반복되는 문제나 충분한 개선 근거가 쌓이고 실행 중인 작업이 없을 때 **자기개선 한 번 해볼까요?**라는 보조 제안이 표시된다. 근거를 보고 사용자가 모델·예산을 정해 시작하며, 나중에를 누르면 하루 동안 다시 권하지 않는다. 대화 오류에서는 **자기개선으로 수정**을 눌러 직접 관련 기록을 연결할 수도 있다. 알림을 여는 것만으로 모델을 호출하지 않는다. 실패한 실행은 모델을 바꿔 같은 목표로 이어갈 수 있다.
5. 화면의 중단 버튼으로 실행을 멈출 수 있다. 실패·연구 기록과 기존 채택 버전은 보존된다.

로컬 야간 설정은 로컬 제공자를 선택하고 유료 전환을 금지한다. 충분한 RAM/VRAM이 필요한지는 선택한 모델 크기에 따라 달라진다. API 제공자의 사용량/청구 정책은 제공자에서 확인한다.

## 호환 범위

| 기능 | macOS | Windows |
|---|---|---|
| Rust 엔진·React 채팅/도구·로컬 경험 저장 | 공통 코드 | 공통 코드 |
| 소스의 격리 수정·재빌드·후손 실행·독립 평가 | 공통 코드 | 공통 코드, MSVC 개발 환경 필요 |
| 검증된 소스를 시작한 원본 브랜치에 반영 | 지원 | 지원 |
| 현재 실행 파일 자동 교체·서비스 재시작 | 해당 소스 빌드의 macOS LaunchAgent 설치에서 지원 | 현재 수동 재빌드·재시작 필요 |
| 네이티브 설치 파일 빌드 설정 | Apple Silicon/Intel | x64 NSIS/MSI |

Windows에서 소스 반영 후 실행 중인 서버를 종료하고 `npm run build --prefix web`과 `cargo run --release -- --home .`로 다시 실행한다. 패키지 앱을 소스 빌드로 덮어쓰지 않는다. OS별 설치 파일 빌드와 일반 CI의 플랫폼 매트릭스는 저장소 워크플로에 있다. 실제 통과한 실행 결과는 해당 커밋의 Actions 기록으로 확인해야 한다.

## 데이터와 검증

기본 소스 실행의 로컬 데이터는 `runtime/` 아래에 있다. 키·개인 대화·실패 원문을 공개 레포나 홍보 자료에 넣지 않는다. 검증된 코드 계보와 익명화된 집계만 공유한다.

[시스템 자기개선 계약과 실증 범위](RSI-MAIN-INTEGRATION-2026-11-12.md), [평가 프로필 확장](../rsi/evaluations/README.md)을 함께 확인한다.
