# 서브1 · 실사용 오픈소스 에이전트 SDK 통합 (2026-11-14)

## 1. GitHub·허깅페이스 2026년 조사 결과

**SDK 소스 무료 ≠ 모델 추론 무료.** 도미니언에 새 유료 API를 기본값으로 추가하면 고객 원가·비밀키 위험만 생긴다. 아래는 2026-11-14 기준 공개 원저자 프로젝트와 확인한 실행 능력이다. 인용만으로 제품 성능 향상을 주장하지 않는다.

| 공식 프로젝트 | 공개 라이선스 / 언어 | 실제 제공 기능 | 도미니언에서 적용 판단 |
|---|---|---|---|
| [QwenLM/Qwen-Agent](https://github.com/QwenLM/Qwen-Agent) | Apache-2.0 · Python | Qwen3 도구 호출 파서, Assistant 실행 루프, 사용자 도구, MCP | **이번에 실제 실행 백엔드로 연동**. Ollama 127.0.0.1 경유, 파일 조작 도구를 DOMINION 제한 버전으로 정의 |
| [OpenAI Agents SDK](https://github.com/openai/openai-agents-python) 및 [JS](https://openai.github.io/openai-agents-js/) | MIT · Python/TS | 툴·handoff·guardrail·session·trace·서브에이전트 | 병렬 외부 백엔드 후보. 기본 provider 인증/비용·도구 실행 정책과 중복되므로 무작정 전체 Rust 엔진 교체하지 않음 |
| [OpenAI Codex SDK](https://github.com/openai/codex/tree/main/sdk/typescript) | Codex OSS 저장소의 TS SDK | 실제 Codex CLI agent를 임베드, JSONL 실행 이벤트, 파일 변경·세션 연속성 | 고난도 코딩 사용자 선택 백엔드 후보. Codex 실행 비용·로그인 필요, 사용자 예산 승인 없는 자동 사용 금지 |
| [Google ADK 2.0](https://github.com/google/adk-python) | Apache-2.0 · Python/TS/Go | 그래프·동적·협업 워크플로, 플러그인, 평가 | 워크플로 단계 동결·검증 기준과 비교. 기존 Rust 재개/권한을 우회한 중복 엔진 금지 |
| [Deep Agents](https://github.com/langchain-ai/deepagents) | MIT · Python/TS | 도구 실행·계획·하위 에이전트·디스크 문맥·체크포인트 | 장기 도구 세션 관리 후보. 모델/런타임 선택 비용과 장애 복구 추적 후 채택 |
| [Agent Client Protocol Rust SDK](https://docs.rs/agent-client-protocol/latest/agent_client_protocol/) | 공개 Rust 라이브러리 | ACP v1 JSON-RPC, 세션·권한·스트리밍·프로세스 통신 | 추후 Codex/타 ACP 코딩 에이전트 연결 어댑터 후보. 프로토콜 지원 여부 검증 없이 '모든 SDK 즉시 호환' 주장 금지 |

Qwen 모델카드 [Qwen3-4B](https://huggingface.co/Qwen/Qwen3-4B)와 [Qwen3.5-4B](https://huggingface.co/Qwen/Qwen3.5-4B)는 공식적으로 Qwen-Agent의 tool-template/parser를 통한 agentic 사용을 권장한다.

## 2. 실패한 기존 실측 데이터 → 실제 SDK 선택지

[서브1 최초 실모델 평가 GitHub Actions 43583057056](https://github.com/hushmind3/dominion-rust/actions/runs/43583057056)의 **아티팩트 ZIP 원문**을 회수했다.

- 모델 Qwen3 4B Instruct-2507 Q4_K_M, Ollama loopback, 2026-11-14, Git HEAD `3e5eee94`, 2개 고정 소형 다중 파일 코딩 과제.
- 실제 `live-goals.json`: **외부 독립 오라클 0/2, 둘 다 480초 제한시간 초과 취소**, 0개 파일 수정, input/output SHA 모두 동일, 기록된 토큰 0. 이는 모델이 0토큰을 사용했다는 뜻이 아니라 **영수증 확보 실패**다.
- `sub1-ollama.log`: 여러 차례 POST /v1/chat/completions가 HTTP 200으로 돌아옴. 예를 들어 17초·22초·60초·140초 처리도 있었으나 **파일 변경은 없음**. 단순히 모델 설치/HTTP 부팅 성공을 '실제 코딩 성공'으로 인정할 수 없다.
- 실제 원인은 기존 DOMINION 내부 Goal harness + 모델·도구 파서 조합에서 코딩 작업을 끝내지 못한 것. Qwen-Agent 연결은 **대체 실행경로이며 성공률 향상은 새 실모델 비교 측정 전까지 미확인**.

## 3. 새 사용자 경로와 도구 계약

**개발자 메뉴 → 저장된 127.0.0.1 Qwen/Ollama Provider 선택 → 작업 지시문 → 결과 파일 검증 조건(선택) → 파일 수정 동의 → 실제 Job 등록 → SDK 실행 → 파일 영수증 조회.**

`web/src/pages/Developer.tsx`
→ `POST /api/jobs` `{kind:"sdk_local",requestId:UUID,input:{prompt,providerId,model,approvedLocalWrite:true,maxSeconds,maxEdits,successCriteria}}`
→ `api/jobs.rs::create`
→ `sdk_local::authorize` (실제 활성 Provider DB·localhost HTTP URL·명시 승인)
→ `jobs::enqueue_once` (동일 요청 UUID 재전송 중복 방지)
→ `jobs::run_job`
→ `sdk_local::run` (비용 없는 SDK 패키지가 설치된 사용자 로컬 Python)
→ `scripts/agent_sdk/qwen_agent_runner.py` (실제 `qwen_agent.agents.Assistant`, `@register_tool`)
→ **허용 도구** `dominion_list`, `dominion_read`, `dominion_write`, `dominion_replace` (셸/브라우저/HTTP/코드 인터프리터 제공하지 않음)
→ Python SHA-256 변경 영수증
→ Rust `read_receipts` **실제 디스크 SHA 재검증**
→ Rust `verification::check_acceptance`으로 작업 등록 *이전*에 고정한 기계 검증 조건 확인
→ `job.result.fileReceipts`, `job.status` (developer 결과 UI).

결과가 작성되었다는 SDK 텍스트만으로 완료 처리하지 않는다. **유효 파일 변경 영수증 + 명시적 기계 검증 조건 충족** 시만 `completed`; 조건이 없는 수정 또는 시간 초과·외부 프로세스 예외는 `needs_review` 및 디스크 확인 요구. 이미 변경이 있었을 수 있으므로 자동 재시도하지 않는다. API·프론트의 `requestId`는 등록 멱등성이고 원격 과금/작업 자체의 exactly-once 보증은 아니다.

### 제공 범위·자원·비용

- Qwen-Agent Python package는 사용자 환경에 별도 설치: `python3 -m pip install 'qwen-agent>=0.0.26,<0.1'`. 기본 DOMINION 빌드에는 **새 Python 의존성이 설치되지 않는다**. `DOMINION_QWEN_SDK_PYTHON`로 인터프리터를 선택할 수 있다. 정상 실행은 Ollama 로컬 모델 사전 준비 필요.
- REST API로 입력된 modelServer URL은 무시하고 **저장된 Provider DB의 loopback 주소만 사용**한다. 상용 모델 API 키는 SDK worker에 넘기지 않는다. CPU/GPU 사용·기기 전력·저장소는 무료가 아니다. 실측 VRAM·토큰·원화 비용은 결과 없으면 null.
- 명시적 동의를 받은 DOMINION config.workspace에서만 편집한다. 파일 크기·수정 건수·SHA 낙관적 동시성·상대 경로·심볼릭 링크·민감한 경로 제한이 있다. 이 도구 제한은 **OS 자체 프로세스 샌드박스가 아니며** 악성 Python/SDK 패키지에 대한 방어 보증이 아니다. 배포 시 격리 계정/컨테이너 추가가 필요하다.
- 기존 Rust goal runner, 그룹대화, RSI, 영상, MCP, 워크플로, 모델 설정은 변경 없이 병행 사용. 외부 SDK를 설치하지 않아도 기존 DOMINION 정상 기능이 작동한다.

## 4. 실제 테스트와 남은 결과

- `python3 -m unittest discover -s scripts/agent_sdk -p 'test_*.py' -v`: 독립 SDK 도구의 실제 디스크 SHA/원자적 부분 교체·경로 탈출·민감 파일·수정량 테스트, 가짜 SDK 객체로 **공식 Assistant + Tool 호출 계약을 실행한 영수증 경로**. 모의 SDK 테스트이지 Qwen3 실모델 결과는 아니다.
- `src/engine/sdk_local.rs`: 로컬 Provider 인증/승인 거부, 외부 URL 금지, 영수증 SHA 검증 Rust 단위 테스트. DOMINION Rust 전체 테스트와 프론트 빌드는 CI에서 확인.
- **실모델 Qwen-Agent SDK를 켠 2개 과제의 독립 오라클 완수율: 아직 측정하지 않음.** 따라서 기존 0/2 결과를 새 SDK가 이겼다고 말하지 않는다. 다음 비교는 같은 모델 digest, 2개 과제, 최대 시간을 사용하되 planning/review 호출 오버헤드, tool-write 횟수, task 성공률, 총 추론 토큰·전력·실비를 함께 기록해야 한다.
