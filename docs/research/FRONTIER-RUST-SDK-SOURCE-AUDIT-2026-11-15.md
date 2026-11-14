# Dominion: 2026-11-15 프런티어 Rust SDK 소스 비교와 구현 추적

범위: 공개된 **실제 코드·도구 인터페이스**를 비교하고 자체 하네스에 반영한 구현을 추적한다. 모델 자체의 IQ, 특정 회사의 서비스 성과, 프롬프트 문구를 독립 엔진의 성능으로 혼동하지 않는다.

## 1. 확인한 미국·중국 오픈소스 실행 구조

| 프로젝트 | 분석한 실제 소스/인터페이스 | 채택할 강점 | 한계 및 Dominion의 독립 설계 |
|---|---|---|---|
| 미국 **OpenAI Codex** | [Rust sandboxing.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/sandboxing.rs): `Approvable`, `ToolRuntime`, `Sandboxable`, 승인·샌드박스의 분리된 실행 책임 | 모델 호출과 파일/셸 실행의 권한 경계, 실행 컨텍스트·이벤트·검증 가능한 작업 수명 | Codex SDK나 Copilot을 백엔드로 심지 않는다. `src/engine/chat/runtime`, `src/engine/tools.rs`, `src/engine/jobs.rs`가 실행 주도권을 유지한다. |
| 미국 **OpenHands Software Agent SDK** | [agent/base.py](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/agent/base.py): agent context, condenser, LLM, MCP/도구 추상화 경계 | 장기 문맥 압축/회수와 모델·도구 의존성 분리 | 별도 Python 프로세스에 작업을 재위임하지 않는다. 기존 Rust context/compaction/skills/MCP를 유지하고 성능을 증거로 비교한다. |
| 중국 **Qwen Code** | [agent tool 계약](https://github.com/QwenLM/qwen-code/blob/main/docs/developers/tools/task.md): named subagent/fork, `fork_turns`/ `fork_tools`, 결과 회수와 백그라운드 실행 | 위임 작업의 문맥 선택·도구 정책 축소·공통 prefix 캐시 기회 | 모든 하위 에이전트에 전체 대화를 복제하면 비용/오염이 커진다. 부모/자식 Git worktree 공유 시 쓰기 충돌 가능. Dominion은 역할별 독립 세션·권한 축소 및 원자적 동시 정원을 우선 구현한다. |
| 미국 **SWE-bench Verified** | [공식 평가 저장소](https://github.com/SWE-bench/SWE-bench): 원본 커밋, 참조 패치 미노출, 분리된 Docker 평가 | 모델 자기보고와 외부 hidden-test 해결률 분리 | Dominion 스크립트는 패치·실도구 영수증을 추출하는 드라이버다. 전체 500문항 공식 pass@1 실측 수치를 아직 보유하지 않는다. |

Codex의 2026-08-19 [open harness 설명](https://developers.openai.com/blog/codex-as-a-platform)도 세션, 이벤트, 실행기와 제품 UI의 독립성을 뒷받침한다. 이는 학습 자료이지 Dominion 런타임 의존성이 아니다.

## 2. 이번 소스 변경과 실제 호출 경로

### 병렬 위임 경쟁 조건 해결

이전: `tools::delegate`가 `store.all("job")` 활성 수를 세고 → 자식 `session.put` → `jobs::enqueue`를 별도로 수행했다. 동시에 여러 부모 실행이 들어오면 제한 6을 초과하거나 오류 시 고아 세션을 남길 수 있었다.

현재: `tools::delegate → jobs::enqueue_delegation → Store::reserve_delegation`. SQLite 트랜잭션 안에서 동일 부모의 활성 위임 job을 확인하고, 제한 안이면 자식 `session`과 `job`을 함께 기록한다. 정원 초과 시 둘 다 생성되지 않으며 작업 생성 실패 시 롤백한다. 완료된 위임 작업은 슬롯을 돌려준다.

검증 코드: `tests/support/engine/tools.tests.rs::concurrent_delegate_reservation_is_atomic_and_enforces_hard_budget`의 32개 동시 예약·최대 6개·고아 세션 부재·완료 슬롯 재사용·쓰기 오류 롤백.

### 실패 복구율 과대계상 방지

`scripts/swebench-verified.mjs::measuredRecovery`가 `exitCode != 0` 성공 취급, 실패한 파일 편집 뒤 읽기만 수행한 경우의 '복구' 취급, 종료 여부 불명확한 호출의 성공/실패 혼합을 배제한다. 동일 `runId`의 이후 작업이 **같은 경로로 실제 재편집**하거나 **동일 명령을 exitCode 0으로 다시 통과**한 경우만 복구로 계산한다. 중단/미확정은 `unknownToolOutcomes`로 따로 센다.

Node 회귀: `scripts/swebench-verified.test.mjs`의 nonzero shell, no-op write, interrupted, read-only false recovery.

### 그룹대화 중단 실행 결과의 프로토콜 복구

`src/engine/chat/history_repair.rs::replay`는 누락된 도구 결과에 `effectUnknown=true` 및 `requiresInspection=true`를 표시하여 provider의 tool-call 쌍을 맞추고 원본 SQLite 기록은 보존한다. 이 사양을 따르지 않아 실패하던 `src/engine/chat/tests.rs::room_history_keeps_speakers_distinct_and_drops_orphan_tool_results`의 기대값을 실제 미확정 프로토콜로 갱신했다.

## 3. RSI 자기개선 연구를 실행 코드에 반영

2026-10-27의 [GEPA-Curator 논문](https://arxiv.org/abs/2610.37651)은 실패 사례가 모두 유용한 학습 신호는 아니며 작업 큐레이션과 과거 성공 궤적 회수가 중요하다고 설명한다. 종속 라이브러리로 GEPA를 탑재하지 않고 연구의 **독립 근거 선별 원리**를 Dominion 자기개선에 연결한다.

기존 보호 계층:
`rsi_principle_learning::independent_trial` → `rsi/evaluation/contract.rs` → `rsi/proof.rs` → `rsi_worktree.rs`의 동결 평가·격리 소스·독립 채택 검증.
`rsi/learning/methods.rs`는 실제 실행 후속 조건을 통과한 절차의 후속 재사용을 관리하며 가중치 수정/LoRA는 사용하지 않는다.

**새 코드:** `src/engine/rsi/meta/controller_evidence.rs::generalization_evidence`에서 같은 `candidateId + evaluationProfileSha256` 조합은 동일 평가 자료 1개로 취급하고, 하나의 평가 안에서 동일 holdout `id+domain` 반복도 1회만 기록한다. 중복 실패로 인한 탐색 패널티 과장을 막지만, 독립 심판의 채택 여부·실험 합격을 조작하지 않는다.

회귀: `tests/support/engine/rsi_meta_controller.tests.rs::duplicated_archived_frozen_pair_does_not_amplify_rsi_penalty`.

## 4. 공개 SDK 및 제품 경계

`sdk/js/index.js`의 `DominionClient`는 REST/SSE만 호출하며 독자적인 LLM 정책과 실행 루프를 만들지 않는다. `sdk/js/node.js`의 `DominionRuntime.start`는 실제 Rust 바이너리를 소유한다. UI(`web/`)와 SDK가 같은 Rust 하네스를 사용한다.

```text
외부 TypeScript 앱 ──┐
React UI ───────────┼→ Rust Axum API → Rust goals/chat/runtime → models/tools/MCP
CLI/Node 런처 ──────┘                 ↘ SQLite checkpoints/receipts
                                                ↘ 격리 Git worktree + 독립 RSI 평가/후손 실행
```

## 5. 검증/남은 과제

개발 코드·테스트가 커밋되었다는 사실과 GitHub CI의 실제 결과는 구분한다. 모델이 서로 다른 파일을 안전하게 병렬 수정하는 충돌 제어, 위임 중 부모 취소의 하위 프로세스 전파, 실모델 SWE-bench Verified 500개 수행, 회수율·복구율·총 청구 원가에 대한 외부 비교는 계속 검증해야 한다. 원화 가격은 청구 데이터가 없는 한 참조 견적이므로 실제 지출과 구별한다.
