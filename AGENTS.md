# DOMINION Sub1 — 빠른 소스 탐색

**목표:** 모델 가중치를 고정한 API 기반 RSI. 개선 대상은 도미니언의 소스·하네스·알고리즘·기억·도구·경험 학습·개선 방법과 후손의 재개선 능력이다. LoRA, PEFT, 파인튜닝, 모델 재학습, 사용자 PC의 대규모 추론은 작업 범위가 아니다.

## RSI 수정 시 읽는 순서

1. **요청/상태:** `src/api/rsi.rs`, `src/engine/jobs.rs`
2. **실행·계승 중심:** `src/engine/rsi_evolve.rs`는 조정자입니다. 실제 캠페인 반복은 `src/engine/rsi/execution.rs`, 재컴파일 후손의 계획·실행은 `src/engine/rsi/worker.rs`, 계승 방법은 `rsi/method.rs`, 연구 기록은 `rsi/research.rs`, 토큰 예산은 `rsi/budget.rs`, 방법 프로그램은 `rsi/programs.rs`, 관측은 `rsi/telemetry.rs`부터 읽습니다.
3. **후손의 구조 인식·소스 후보:** `src/engine/rsi_system.rs`는 재노출용 진입점입니다. 실구현은 `src/engine/rsi/system/`의 inventory(실제 소스 해시), blueprint(구조도), discovery(연구·코드 분류), frontier(수정 후보), agenda(후손 작업 계획), design(실제 시스템 설계 등록)에서 확인합니다.
4. **개선 방법 자체의 진화:** `src/engine/rsi_meta_controller.rs`는 진입점입니다. 실제 개선기는 `src/engine/rsi/meta_controller/`의 provenance(채택 증거), reciprocal(A→B→A), selection(선택 정책), transfer(계보 전이), feedback(예측 검증·후손 지시)에서 변경합니다. 실모델에게 가리키는 소스 경로도 새 모듈을 사용해야 합니다.
5. **전체 능력의 상호 개선:** `src/engine/rsi_whole_agent.rs`, `src/engine/rsi_causal_ecology.rs`, `src/engine/rsi_recursive_laboratory.rs`
6. **계승되는 개선 원리:** `src/engine/rsi_principle_learner.rs`, `src/engine/rsi_edit_genome.rs`
7. **독립 검증·폐회로:** `src/engine/rsi.rs`의 후보 영수증, `src/engine/rsi_worktree.rs::classify/compare_at`의 동결 A/B, `src/engine/rsi/proof.rs`의 메타→능력→메타 3세대 검증, `src/api/rsi.rs::apply_verified_capability`의 API 원본 영수증 재검증, `src/engine/rsi_archive.rs`의 참고 계보.

기능 변경 전에는 **호출자 → 실제 실행 → 독립 평가 → 채택된 후손의 재실행** 경로를 추적한다. 독립 평가기 `rsi.rs`, `rsi_worktree.rs`, `rsi_metrics.rs`, `rsi_profile.rs`, `rsi/proof.rs`는 후손 후보가 수정하는 경로에서 제외하고 메타개선기·자기 모델은 계속 진화시킨다. 모델이 말한 성공과 독립 검증된 코드 채택을 혼동하지 않는다. 새 메커니즘은 하네스·알고리즘·기억 등 전체 지능으로 전이될 수 있어야 한다.

## 읽기·실행 비용 억제

- 소스는 파일명으로 좁힌 뒤 함수명·호출부를 먼저 검색한다. 채팅 턴 실행은 `src/engine/chat/runtime/turn.rs`, 기존 런타임 계약은 `src/engine/chat/runtime.rs`를 읽는다. 테스트는 `tests/support/engine/`과 `tests/support/e2e/`에서 책임별로 확인한다.
- `docs/DOMINION-HARNESS-MASTER-DESIGN.md`는 설계 배경, `docs/rsi-completion.md`는 평가 기준이다. `benchmark/`와 `docs/research/`는 필요할 때만 실험 근거로 연다.
- 과거 진행 로그와 중복 실모델 실험 스크립트는 활성 트리에서 정리했으며, 이전 버전은 Git 이력으로 확인한다.
- 일반 CI는 `.github/workflows/ci.yml`. 서브1의 모델·계보 실험 워크플로는 **명시적 수동 실행**만 허용한다. 코드를 읽거나 푸시하는 행위만으로 비싼 모델 추론을 시작하지 않는다.
- 모델 학습이 아니라 **시스템 경험·검증된 개선 방법의 축적과 재사용**을 설계한다.

## 구조적 계약

- 하위 모듈의 공개 인터페이스는 상위 조정자에서 명시적으로 연결한다. 내부 증거·평가·실행에 대한 책임은 섞지 않는다.
- 테스트는 원래 호출 계약과 시나리오를 유지하며 이동한다. 상대 경로 `include_str!`는 이동한 실제 소스 위치에 맞춰 갱신한다.
- 새 실행 로직을 다시 `rsi_evolve.rs`나 `chat/runtime.rs`에 몰아넣지 않는다. 기능 변경 시 기존 모듈을 먼저 수정한다.

## 최신 Sub1 실경로

`docs/RSI-SUB1-ARCHITECTURE.md`를 현재 단일 구조 기준으로 읽습니다. 실제 API·캠페인·후손 계획·독립 OFF/ON 실험·A→B→A2→B2 증명 경로가 함수별로 정리되어 있습니다.

`rsi/proof/full_cycle.rs`는 캠페인·API의 공통 성공 판정기입니다. `rsi/proof/method_lineage.rs`는 대조 평가가 증명된 B만 A2의 방법 재수정 지시로 전달합니다. `rsi/method_control.rs`는 별도 실워커로 OFF를 실행하지만 control source를 채택하지 않습니다. `rsi::safe_path`는 proof/worktree 평가기 영역을 후손 편집에서 막습니다. 보고서 상의 주장과 실제 테스트/빌드/실모델 실행을 구분합니다.

**Experiment architecture:** `rsi/experiment_policy.rs` is the *editable*
compiled algorithm that selects experiments and targets. It has its own
non-overlapping `experiment_design` source role, and receives only original
generation outcome observations. `rsi/contracts/experiment.rs` is the
*frozen typed envelope* that binds those choices to actual indexed source
bytes and a bounded available intervention. The campaign and the worker
validate this contract and act on its target/protocol. Improvement methods
and experiment design can improve each other without prescribing the
A→B→A2→B2 proof order as the next action.

### Stable experiment contract independent of planner and proof

The typed `rsi/contracts/experiment.rs` lives under an immutable `rsi/contracts.rs` composition root. `experiment_policy.rs` and `proof.rs` no longer import each other's private implementation. Both compiled descendant and supervisor call the same read-only `verify_experiment_design` through `rsi_evolve::contracts`. `rsi::safe_path` freezes the entire `rsi/contracts/` subtree; evolution modifies the experiment allocation algorithm, never its own source authentication or accepted evaluator.


## 출시 후 Sub1 2026-11-14 파일·실호출 지도

- 전체 엔진/하네스/알고리즘/UX 연결은 [통합 품질 지도](docs/INTEGRATED-QUALITY-2026-11-14.md)에 있습니다. 특정 기능을 수정할 때는 문서의 **실제 진입점→작업→실행→영수증**을 역추적합니다.
- **그룹 비용/모델 티어**: `web/src/pages/Rooms.tsx` → `src/api/rooms.rs` → `src/engine/rooms.rs::effective_rounds` 및 `src/engine/rooms/model_tier.rs::select` → `run_room_quiet` → `chat/room_context.rs` → `chat/continuity.rs` → 실제 provider. 명시적 모델 티어와 stable prefix 캐시의 장단점을 함께 봅니다.
- **영상**: `web/src/pages/Video.tsx` → `src/api/video.rs` → `src/engine/video.rs` → `src/engine/video/{runway,ltx}.rs`. 결과는 job checkpoint/MP4 파일 및 사용자 업로드 단계에서 확인하고, 결제 불확실 작업을 자동 재생성하지 않습니다.
- **테스트 분리**: `src/engine/{memory,algorithms,learning,improve,jobs}/` 및 `src/db/tests.rs`, `src/api/{providers,video}/tests.rs`, `src/engine/rooms/tests.rs`, `src/engine/chat/room_context/tests.rs`. 테스트 모듈을 이동했지만 CI/컴파일/테스트는 이번 작업에서 실행하지 않았습니다.
- **남은 경계**: 높은 복잡도를 가진 `src/engine/chat/runtime/turn.rs`의 단일 실행 함수, provider별 실제 원화 과금계산, 취소/복구/중복결제 영수증 확인, 영상 샷 품질 평가, 시맨틱 캐시 정책은 구조적으로 분리할 대상입니다. 성공 영수증을 지어내지 않습니다.


## v3.0 자율 목표 에이전트 — 2026-11-14

- 새 기준의 실제 UI/API→Job→작업→증거→검토→복구→계승 경로는 [AUTONOMOUS-AGENT-2026-11-14.md](docs/AUTONOMOUS-AGENT-2026-11-14.md)를 읽습니다.
- UI: `web/src/pages/workflows/GoalConsole.tsx`를 `WorkflowsPage`에서 렌더링합니다. 목표 POST와 중단, 독립 계획·검토, 단계별 기록을 연결했습니다.
- 엔진: `api/goals.rs::create_goal` → `jobs::enqueue/next_queued/run_job` → `goals.rs::run` → `goal_control::complete` 및 `run_quiet_outcome` → `chat/runtime/turn.rs` → 도구 결과 수집 → `verification::acceptance_gaps` → 실패 수정/재계획 → `episode::set_root_outcome` / `learning`.
- 크래시 복구: `goal_review.rs::snapshot/replayable`는 **도구 완료 후** 영속화된 검토만 재시도할 수 있게 합니다. 실행 중 작업, 레거시 체크포인트는 실행 결과를 알 수 없으므로 자동 재실행하지 않습니다. `jobs::recover`는 목표 최종 결정이 이미 저장된 경우 Job을 그 결정에 맞춰 정합화합니다.
- `goal_verification.rs::inferred_checks`는 명시적인 테스트 요청에 대해서만 검증 조건을 요구합니다. 실제 검증 명령이 성공하지 않은 경우 목표를 완료 처리하지 않습니다. 이 모듈은 도구 자체를 호출하지 않습니다.
- 관련 테스트 소스: `engine/goals/tests.rs`, `engine/jobs/tests.rs`, `engine/goal_review.rs`, `engine/goal_verification.rs`. 기존 inline 테스트를 이동하였지만 실행하지 않았습니다.

- **추가 자율 목표 핵심**: `goal_intake.rs::inspect`(manifest와 bounded project inventory), `goal_plan.rs::apply_owner_override`(사용자 계획을 다음 안전한 턴 시작에 CAS 소비), `goal_metrics.rs::final_report`(실제 증거와 audit 토큰 계측). `POST /api/goals/:id/plan`은 사용자의 목표 완료 검증 조건이나 도구 권한을 수정하지 않습니다. `GoalConsole.tsx`는 해당 조작·관측의 실제 UI입니다.


## 새 자율 작업 기능 연결 원칙

- 병렬 하위 조사: `tools.rs::definitions` → `delegate_tasks` → `tools/batch.rs::delegate_batch` → `tools/gateway.rs::delegate` → 고유 자식 세션/Job → `jobs::start_now` → `task_result`. 읽기 전용 child를 소스 편집 가능한 child와 혼동하지 않습니다. `delegation_allows`는 네이티브 도구, 모델 도구 디스패치, 결정적 실행과 MCP 경로를 모두 막습니다. 직렬 부모 검증/수정이 필요합니다.
- Git diff·Undo: `goal_changes.rs::baseline`을 도구 실행 전 저장하고, `preview/undo`는 실제 성공한 `written:true` 도구 증거가 있는 파일에만 접근합니다. 작업 시작 시 clean/HEAD 불일치·도구 기록이 없는 파일·60KB 초과·동시 진행 중 작업은 자동 복원을 허용하지 않습니다. 전체 `git reset`이나 타인의 변경 소거로 해결하지 않습니다.
- 새 소스 구조: `src/engine/tools/batch.rs`, `src/engine/goal_changes.rs`, `web/src/pages/workflows/GoalChanges.tsx`. 사용자 목표 진입점과 기능 경계는 `docs/AUTONOMOUS-AGENT-2026-11-14.md` 참고.
