# DOMINION · 공식 GitHub Copilot Rust SDK 선택 실행기

실행 엔진 참조: [github/copilot-sdk](https://github.com/github/copilot-sdk), 공식 Rust SDK `=1.0.34`(MIT). 2026-11-14 기준 GitHub 공식 자료와 Rust 소스를 직접 조사해 선택형 호스트 프로세스를 구현했습니다.

## 실제 기능과 적용 범위

`Workflows.tsx` → `workflows::enqueue_run` → `workflows::run` → `agent_sdk::run` → 별도 `dominion-copilot-sdk-runner` → **공식 SDK `Client::start` / `SessionConfig` / `send_and_wait`** → Copilot CLI JSON-RPC 실행.

도미니온 기본 엔진은 변경되지 않습니다. 워크플로의 각 단계에 `harness: copilot-sdk`를 선택할 때만 이 경로를 사용합니다. 애플리케이션이 SDK를 모방해 자체 LLM 채팅 API만 호출하는 방식이 아닙니다.

지원 모드:
- **Ollama 로컬 BYOK:** 기본 `http://127.0.0.1:11434/v1`. 모델은 사용자가 입력하며, 그 모델이 SDK의 도구 호출 요구에 적합한지는 실제 테스트해야 합니다. Ollama 서버는 먼저 실행되어 있어야 합니다.
- **GitHub 계정:** 로그인된 Copilot CLI 계정. 구독·요금제의 사용량 제한이 적용되므로 워크플로 실행 시 별도 모델 비용 동의를 받습니다.
- **기본 권한:** `read_only`가 `deny_all_permissions()`를 설치하여 승인 요청이 필요한 파일 수정·명령을 거절합니다. SDK 런타임 자체가 권한을 요구하지 않는 읽기 작업까지 차단한다는 의미는 아닙니다.
- **고신뢰 작업:** `trusted_full`이 `approve_all_permissions()`를 사용하려면, 사용자 명시 승인 **및** 서버 `DOMINION_COPILOT_TRUSTED_TOOLS=1`이 모두 필요합니다. 이 모드는 운영체제 수준 격리를 제공하지 않습니다. 격리된 테스트 작업공간에서만 사용하세요. 반복 자동화에서는 허용하지 않습니다.
- **복구:** 작업 시작 직전에 `workflowState.inFlight=true`를 영속 저장합니다. SDK가 타임아웃·종료·응답 오류를 반환하면 `needs_review`이며 이전 도구 부작용을 무조건 다시 수행하지 않습니다.
- **계측:** SDK에서 실제 수집된 `assistant.usage` 이벤트의 `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`를 합산합니다. 캐시 토큰은 입력 토큰에 포함되어 있으므로 중복 가산하지 않습니다. `tool.execution_start/complete`도 이벤트 수만 기록하며, 이것을 도미니온의 독립적으로 검증된 `execution_operation`/파일 영수증이라고 속이지 않습니다.
- **미측정:** 실청구 원화, 실모델 작업 성공률, 독립적인 파일 결과 품질, 다른 에이전트 SDK 대비 우위. SDK 모델 제공자 트래픽은 SDK 소유이므로 기존 도미니온 제공자 사용량 원장에 자동으로 합쳐지지 않습니다.

## 설치

이 디렉터리는 도미니온 루트 Cargo 빌드와 **분리된 선택형 Cargo 패키지**입니다. 원래 `cargo test --release`는 SDK 크레이트를 컴파일하지 않습니다.

필요 조건: Rust **1.94 이상**, 선택된 플랫폼의 SDK 필수 개발 라이브러리(리눅스 OpenSSL 3 개발 패키지 등), 선택형 로컬 Ollama 또는 Copilot 계정.

```sh
cargo build --release --manifest-path integrations/copilot-sdk-rust/Cargo.toml
export DOMINION_COPILOT_SDK_RUNNER="$PWD/integrations/copilot-sdk-rust/target/release/dominion-copilot-sdk-runner"
# Ollama 모드 예: 로컬 Ollama가 먼저 실행 중이어야 함
# copilot 모드 예: Copilot CLI에 로그인돼 있어야 함
```

도미니온에서 **워크플로 → 단계 편집 → 에이전트 실행 엔진 → GitHub Copilot SDK**를 선택하고, 로컬/계정 공급자 및 모델 ID를 지정한 뒤 실행합니다. 실시간 모델 목록과 SDK 실행기 설치 상태를 자동 인식하는 UX는 이후 고도화 대상입니다.

고신뢰 도구를 명시적으로 허용하려면 로컬 서버 관리자가 `DOMINION_COPILOT_TRUSTED_TOOLS=1`을 설정하고 실행 화면에서도 한 번 더 승인해야 합니다. 워크플로가 자동화될 때 해당 모드는 거절됩니다. 로그인 토큰과 API 키는 Job의 JSON 본문에 담지 않습니다.

## 검증

- 루트 Rust `src/engine/agent_sdk.rs` 단위 테스트: 기본 비활성, 로컬 모드, 유료 계정 동의, 관리자-사용자 이중 승인, 프로토콜 결과 검증.
- 선택형 외부 실행기 `src/main.rs` 자체 단위 테스트: 캐시 토큰 중복 가산 방지, 이벤트가 없을 때 0토큰으로 위장하지 않기.
- 실제 Copilot/Ollama 요청은 모의 네트워크 테스트로 성공을 가장하지 않습니다. 외부 실행기 크레이트의 별도 빌드와 실모델 작동은 결과가 남아야 검증됐다고 판단합니다.

## SDK 기술 비교 (2026-11-14 공개 코드 조사)

| 프로젝트 | 언어 / 성격 | 도미니온 활용 |
| --- | --- | --- |
| [GitHub Copilot SDK](https://github.com/github/copilot-sdk) | Rust 포함 6개 언어, 완성된 Copilot CLI 실행 하네스, MIT | **이번 선택형 실행기 구현**. 로컬 Ollama BYOK 가능 |
| [Rig](https://github.com/0xPlaygrounds/rig) | Rust 네이티브 LLM 에이전트 구성 요소 | 모델/도구 결합 계층 대안, 자체 하네스 완성도를 자동 보장하지는 않음 |
| [Goose](https://github.com/block/goose) | Rust 기반 실제 코딩 에이전트·MCP | 도구 실행 정책과 실제 코딩 벤치마크의 참조 구현 |
| [OpenHands SDK](https://github.com/OpenHands/software-agent-sdk) | Python SDK + TypeScript/REST Agent Server | 개발용 격리 작업공간·원격 하네스 연동 후보 |
| [Deep Agents](https://github.com/langchain-ai/deepagents) | Python/TypeScript, 장기 작업/하위 에이전트 | 긴 작업 계획·복구의 비교 대상 |
| [OpenAI Agents SDK](https://github.com/openai/openai-agents-python) | Python, 멀티 에이전트 핸드오프·MCP·도구 | 팀 실행과 검증 경로의 외부 비교 대상 |

SDK 오픈소스와 모델 사용량·인증 요금은 다른 문제입니다. 엔진 교체의 이점은 실제 동일 과제, 동일 모델, 동일 횟수, 독립 오라클을 통해 비교해야 합니다.
