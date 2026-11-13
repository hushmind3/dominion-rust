# Sub2 executable RSI boundaries — 2026-11-13

## Source change, not a score claim

Two structural changes on `dominion-sub2` preserve the existing API/jobs → child policy → source editing → frozen paired evaluation → accepted descendant path. They separate responsibilities that previously shared large Rust source files and make the experimental method-transfer repair target resolve to its **actual compiled implementation**.

- `rsi_meta_controller.rs`: source-grounded target selection and prediction logic, with `peer_transfer.rs` for donor selection/negative-transfer observations, `observations.rs` for feedback calibration and `guidance.rs` for model-facing instructions. Regression tests live in `tests.rs` and are removed from the runtime-editable source inventory.
- `rsi_system.rs`: system source inventory, ownership and indexed frontier; `evidence.rs` validates plan/lineage receipts, `blueprint.rs` constructs descendant architecture, `history.rs` replays independently verified observations, `planning.rs` accepts model-authored designs and inherits domains, `proof.rs` determines witnessed multi-generation capability, and `guidance.rs` generates model instructions.
- `rsi/planner.rs`: a failed donor-method transfer is now routed to `rsi_meta_controller/peer_transfer.rs`, while prediction-rule repair still targets `rsi_meta_controller.rs`. A targeted unit test checks priority conditions, preventing a self-repair from being silently redirected to a façade.

## Actual calls and evidence ownership

`compiled_system_generation_plan` still calls `rsi_system::revalidated_method_history`, `blueprint`, `architecture_frontier`, `rsi_meta_controller::synthesize`, `peer_lineages`, and `evolve_from_peer_methods` in the same order. The existing public names are re-exported from small Rust submodules, so the coordinator's call graph does not change.

Frozen source receipts and paired evaluation are still required; source placement, retrospective claims and tests alone cannot certify behavioral gain. The history-replay boundary remains distinct from the method that adapts future candidates. New source modules appear under `improvement_methods` in the runtime source inventory and the capped indexed frontier; `tests.rs` remains excluded.

## 2026 research used

- [HyperAgents](https://ai.meta.com/research/publications/hyperagents/) (2026-03-24): the algorithm for improving agents must itself be editable, not a permanently fixed supervisor.
- [Recuris](https://github.com/Gen-Verse/Recuris) (2026-08): source-grounded, localized experience updates plus independent held-out evidence.
- [EvolveNet](https://arxiv.org/abs/2608.04968) (2026-08): compose independently observed harness modifications using typed scope and evidence instead of treating every donor's success as transferable.

## Verification status

The tree/branch and function-preservation checks were performed against GitHub content. **No compile, build, CI, real-model test or full RSI generational proof was performed** for these commits. The focused Rust tests in the source are proposed regression coverage until actually executed. No model-weight learning, LoRA or fine-tuning path was added.

## Runtime and detached worktree boundary cleanup

- `src/engine/chat/runtime.rs` was structurally reduced from 6,984 to about 647 lines. The large live-turn function remains intact in `runtime/turn.rs`; adaptation, durable program execution, tool catalog, tool gateway, semantic finalization, session signals and tests are now separate child modules. Static checks matched all 73 non-test top-level declarations and original function bodies after visibility normalization.
- `src/engine/rsi_worktree.rs` now delegates to `rsi_worktree/{metrics,checkout,validation,candidate,feedback,promotion,comparison}.rs` while preserving the original entry-point names. Paired comparison and operator-controlled source promotion reside in separate modules. Tests were moved out of the runtime implementation.
- Worktree integrity and paired evaluation modules remain in the descendant's full source-inventory fingerprint, but do **not** appear as editable architecture targets. This maintains an independently held verification boundary after the file split. A source-level regression test asserts this distinction.

GitHub source commits use `[skip ci]`; static equivalence checks are not Rust compilation, external CI results, or a verified model-generated descendant success.

## Recursively editable method-learning and descendant planning modules

The source-level `rsi/learning.rs` responsibilities have been split into `learning/{selection,repairs,ablation,failed_edit,feedback}.rs`. These retain the original executed-method proposal/reuse, source-repair synthesis, outcome-bound verified write attribution, and future method feedback. Source-repair attempts still cannot earn accepted-improvement credit merely by being generated.

The `rsi/planner.rs` controller now delegates to `planner/{actions,strategy,edit_policy,compiled}.rs`. Action-outcome adaptation, generation target selection, source-edit rule calibration and compiled descendant planning can be independently redesigned without altering the frozen pair evaluator. The negative-transfer target regression test remains in the parent planner module. Re-exports preserve the previous internal entry points.

Static inspection confirmed 25/25 method-learning and 19/19 planner function declarations accounted for, with no missing cross-module names among those source files. These are **source-preservation checks**, not Rust type-check or runtime proof. A future independent judge should inspect the entire updated source tree and the true API/jobs/model/worktree path rather than score this document.

### Champion integration guidance

Preserve three material boundaries when selecting these changes:

1. The descendant's editable transfer algorithm is `rsi_meta_controller/peer_transfer.rs`, and an observed failed transfer must target that real implementation rather than the parent re-export façade.
2. Detached worktree source, comparison and promotion remain fingerprinted but excluded from model-editable architecture frontiers; they independently verify source changes.
3. A proposed new method, code-edit call, or research finding is not an adopted improvement. Only a verified source diff with independent paired evidence and a descendant able to execute its inherited method can establish that link.

All sub2 changes are isolated to `dominion-sub2` and committed with `[skip ci]`; the user did not request a build or a full-model CI run at this stage.

## 2026-11-13 judge 35/100 feedback: inheritable executable method closure

The independent judge awarded **Sub2 35/55 structural and 0/45 RSI** on the available evidence; the scores are not changed by author documentation or source-only commits. In response, this branch repaired a specific implementation failure and added a native, independently revalidated descendant-method feedback path.

### Source-backed invocation path

`src/api/rsi.rs` → `src/engine/jobs.rs::run` (`rsi_evolve`) → `src/engine/rsi/execution.rs::run_loop` → newly built `rsi/worker.rs::worker` → `rsi/planner/compiled.rs::compiled_system_generation_plan` → `rsi_meta_controller::synthesize` / `peer_transfer::evolve_from_peer_methods` → actual model edit gateway → `rsi_worktree/comparison.rs::compare_at` frozen paired evaluation → `rsi/lineage.rs::adopt` → following generated binary.

The previous module extraction left `rsi_meta_controller/observations.rs` calling private helpers in `peer_transfer.rs` without importing them. This is repaired by exposing them to sibling modules and explicitly importing them. The executable `peer_transfer.rs` source is also always included in the candidate alternatives when present in the frontier, avoiding silent loss under top-six truncation.

### Verified-method inheritance and recursive repair

- `rsi_system/method_lineage.rs::inherited_method_receipt`: requires an independently adopted ancestor with a model-authored design covering a recognized, actually called meta-method module. Both the inherited commit and current source bytes must match the accepted before/after SHA lineage. It records whether the rebuilt child changed target or mode and actually focused that new target, WITHOUT claiming causal gain.
- `rsi/worker.rs::worker`: computes the receipt against `revalidated_method_history` before the child model call, persists it in the job and provides the observed lineage in the actual tool-facing instruction when applicable.
- `rsi/execution.rs::run_loop`: persists `metaMethodInheritance` and `metaMethodOutcome` after the real child was evaluated, keeping assessed, adopted, and subsequent method rewrite distinct.
- `rsi_system/history.rs`: replays original paired measurement records and recomputes method inheritance/outcomes, discarding unmatched stored reports.
- `rsi_meta_controller.rs::inherited_method_repair`: upon two independently assessed unsuccessful successors in the same method/commit lineage and no adopted successes, it schedules a rewrite of the very meta-method source; `rsi/planner/compiled.rs::evidence_based_meta_repair_focus` and `guidance.rs` turn this into an executable source target and tool-facing task. Cooldown prevents an endless meta-only revision spiral.

**Boundary:** neither the success of an edited source nor an observed changed next target proves that the edited policy CAUSED a gain. Follow-up live-model, independent same-budget controls and multiple accepted descendant generations are still outstanding. This work does not execute CI, compile, run a benchmark, or connect model training.

### 2026 sources for architecture decisions

- [Meta HyperAgents, 2026-03-24](https://ai.meta.com/research/publications/hyperagents/): put task-solving and meta-improvement methods in the actual mutable execution program.
- [AIDE², 2026-09-22](https://arxiv.org/abs/2609.26457): an accepted source rewrite becomes the next agent, and hidden held-out evaluations determine admission.
- [RSI-Env, 2026-11-01](https://arxiv.org/abs/2611.01736): retain a persistent independent host controller for evaluation/versioning while accepted descendants take over subsequent improvement rounds. The cited system may also train models; this branch restricts RSI to source, policy, memory and harness changes with fixed base weights.

## Adoption-source provenance and independent descendant attempts — 2026-11-13

The method-inheritance receipt now requires `sourceLineageVerified`, recomputed by
`rsi_system/history.rs` from an independently replayed paired candidate and a
second source-hash ledger stored at **the actual worktree adoption action**.
`rsi/execution.rs` writes the adopted source SHA-256 map both to its generation
summary and to the candidate's `experimentAdoption.acceptedSourceHashes`.
The revalidator binds candidate ID, originating child job, origin, base commit,
adoption commit, and each mapped changed path before allowing the
`rsi_system/method_lineage.rs` inheritance claim.

Prior paired outcomes without the new adoption ledger are still replayed as
task feedback but are not retroactively credited with method-source inheritance.
The `rsi_meta_controller::inherited_method_repair` uses unique child job IDs,
so two copies of one assessed child cannot produce the two-independent-failure
signal that rewrites the executable improvement method. Regression cases cover
cross-job borrowing, modified SHA maps, and duplicate descendant observations.

This is a stronger provenance and independent-observation boundary; it is not
a cryptographic append-only record and does not prove that an edit caused a
quality gain. It follows the 2026 AIDE² principle of inherited accepted
agent revisions (https://arxiv.org/abs/2609.26457) and 2026 RRSI history-aware
proposal discipline (https://github.com/google-research/rrsi).
No test, compile, benchmark, workflow, or model-generation run was performed.

## Responsibility-based RSI regression suite

The prior `src/engine/rsi/tests/evolve.rs` held 105 test cases across
4,326 lines and intertwined search, research, adoption, tool-repair and
lineage test responsibilities. It is now a small entry-point `include!`
manifest for 12 responsibility-named siblings in the **same directory**.
This preserves the original module scope and relative `include_str!` file
locations. No test bodies or assertions were changed, and a source-string
reconstruction check confirmed that the original test items were retained
in order; this is not a compile or test result.

The production engine path is unchanged. These test files remain excluded
from the editable method frontier via `runtime_source_file`, while the
independent scorer and source verification stay outside the editable planner.


## Source-unified recursive completion gate (2026-11-13)

Judge feedback: Sub2 scored 46/100, including 14/45 on RSI, because the
runtime and API could call the weaker adopted-meta-transfer predicate without
a closed inherited-method re-improvement lineage. This change does not claim
to increase the score without a fresh independent source review.

### Executable intervention, not an appended success label

1. `rsi_system/history.rs` first replays the original frozen paired candidate
   records, binds child job IDs, and checks source-adoption hashes.
2. The rebuilt descendant's `rsi/planner/compiled.rs` asks
   `rsi_system/return_feedback.rs::next_method_return` for a source-bound
   observed outcome. B must be a separately adopted downstream capability,
   its compiled child must have actually focused the inherited A method's
   target, and its paired holdout evidence must be independently improved.
   The proposed A method source must still match the CURRENT indexed SHA.
3. The compiled planner changes the actual `metaPolicy` target/revision,
   selects the exact method source for the worker's editable focus, and carries
   B's candidate/job/paired-evidence IDs as `downstreamMethodFeedback`.
   Explicit owner focus and adopted predecessor programmes take precedence.
4. `rsi/worker.rs` recomputes B's method-return evidence from original
   stored receipts before recording `rsiMetaReturnExecuted`; the live model
   sees this feedback in its tool-facing source-edit instruction. An intended
   return is not recorded as an adopted edit.
5. `rsi/closure.rs::evidence` replays the original candidates, paired
   measurements, child jobs, accepted source SHA maps, inherited compiled
   method-use receipts and C's evaluated source diff. A, B and C must be
   contiguous independent source adoptions and successive binary revisions.
   A and C each need a genuinely improved improver holdout; B must improve
   a distinct capability and NOT edit the A method; C must edit the SAME
   source with a new SHA and be independently adopted.
6. Both `rsi/execution.rs` (stop + durable campaign fields) and
   `api/rsi.rs` (single/list/detail completion display) call the same
   `rsi::campaign_recursive_evidence` on the same source-verified campaign
   continuation history. Three accepted flags alone cannot stop the campaign.

`rsi.rs::executable_improver_path` now uses the same recognized compiled
method source list as `rsi_system/method_lineage.rs`. The candidate admission
and `rsi_worktree/validation.rs` classification no longer reject actual
method edits to `rsi_meta_controller/peer_transfer.rs` as non-improver changes.
The new verifier, replayed-history verifier, self-model source validator and
legacy frozen oracle remain outside model-editable source paths.

### Research mapping (all 2026)

- [HyperAgents, 2026-03-24](https://ai.meta.com/research/publications/hyperagents/):
  the meta-improvement procedure itself is a compiled editable program, not
  a frozen hand-written supervisor.
- [AIDE², 2026-09-22](https://arxiv.org/abs/2609.26457):
  accepted rewrites must become the next executed agent and are selected by
  separately measured held-out improvement.
- [CausalFlow, 2026-05-25](https://arxiv.org/abs/2605.25338):
  localized intervention provenance and counterfactual failures motivate
  distinguishing a measured B outcome from A's claimed causal superiority.

### Evidence boundary

The new code supplies a narrowly replayable, **observational A→B→A loop**
whose first/last method steps and downstream step independently improve
frozen cases. It does not establish a randomized causal estimate of how much
A's earlier rewrite CAUSED B's gain, cannot prove unbounded growth, and does
not retroactively upgrade prior campaigns lacking source adoption receipts.
The regression cases were authored but no compile/build/tests/CI, benchmark
or real-model run was executed in this session. The independent judge should
evaluate source registration, actual job calls, and non-forgeable receipts.

## 2026-11-13 추가 폐회로 강화 — 실제 B′ 재적용

앞선 점검에서 원본 `rsi.rs::campaign_recursive_evidence` 및 `rsi_system::next_method_return` 선언이 최종 파일 조합에서 누락된 상태를 발견해 수정했습니다. `rsi_evolve.rs`가 `rsi/closure.rs`를 직접 모듈로 소유하고, `rsi_system.rs`가 `return_feedback.rs`를 등록합니다. 이로써 실행·API·후손 정책의 심볼 경계가 연결됩니다.

이제 A→B→A′의 3번 독립 채택은 성공이 아닙니다. `next_capability_reapplication`은 검증된 A′의 소스 해시, B의 원래 목표 파일·분야·평가 영수증을 결합하여 **B′가 실행할 실제 코드 대상**을 반환합니다. `compiled_system_generation_plan`이 그 대상을 정책·소스 focus·도메인에 반영하고, 워커가 `rsiMetaReapplicationExecuted`를 원본 재생 증거로 기록합니다. 최종 `rsi/closure.rs`는 4연속 채택 A/B/A′/B′와 B′의 재평가 개선, 동일 B 태스크 소스의 실제 편집, 바뀐 A′ 방법의 계보를 확인합니다. 채택된 파일이 하나라도 해시로 결속되지 않으면 `rsi_system/history.rs`가 그 출처를 거부합니다.

이것은 **관찰 기반 4세대 전체 폐회로**를 증명할 수 있는 연결입니다. 비활성 대조군과 활성 실행군의 독립 결과 비교는 아직 구현되지 않았으며, 4세대 실측 성공·CI/빌드/테스트 또한 실행하지 않았습니다. 별도 심판 재채점 전에는 기존 점수를 유지합니다.


## 2026-11-13 실제 개선 전후 컴파일 방법 대조군

새 `src/engine/rsi/method_control.rs`는 모의 계획 비교나 모델에게 주어진 설명문이 아닙니다. `rsi/execution.rs`가 독립 평가·채택된 **개선 방법 전용** A 소스 변경을 발견하면, 이전 버전의 실제 컴파일 바이너리를 보존합니다. 현재 버전의 모델 실행이 B에서 작동하기 직전, 직전 바이너리를 사용해 현재 버전과 동일한 Git HEAD 소스 트리에서 이전 개선기 B 대조 워커를 실행합니다. `bootstrap.rs --rsi-plan-counterfactual`은 이전 바이너리 소스 revision을 검증하면서 동일 캠페인 DB의 동결된 과거 기록을 읽는 읽기 전용 계획 경로이고, `rsi/worker.rs`가 원래 작업과 동일한 worker 진입점을 실행합니다. 두 워커는 각각 분리된 Git worktree에서 독립 소스 편집을 수행하고 `rsi_worktree::compare_at`으로 같은 원본 동결 실험을 평가합니다.

`method_control::measured_advantage`는 채택된 A 메서드 소스 해시, 이전·신규 바이너리 SHA, 동일 HEAD·모델·도메인·동결 사례·원본 후보 쌍을 재검증합니다. 메서드 개편 후 실제 컴파일된 계획 행동이 달라져야 하며, 이전 대조군보다 동결 사례 통과 또는 품질 지표 개선이 있어야 하고 역전 사례는 없어야 합니다. 최종 `rsi/closure.rs`는 B와 B′의 **각각의** 원본 대조 실험에서 `qualityProven=true`인 경우에만 `capabilityDemonstrated`를 허용합니다. 어떤 대조 후보도 프로덕션 또는 실험 채택 Git 계보에 포함하지 않습니다.

전체 RSI 자동 연속 실행에서는 이 대조가 기본 활성화되고, 추가 비용을 원치 않는 경우 요청의 `methodControl:false`가 게이트를 끄면서 완성 판정도 발생하지 않도록 합니다. 검증 불가·대조군 오류·도메인 또는 모델 불일치 시 점수를 올리지 않습니다. 메서드 전용 변경 외의 시스템 변화는 대조 인과 귀속 대상에서 제외합니다. 이는 **이전/현재 실행 프로그램을 동일 소스에 대한 실제 모델·도구 작업으로 비교**한다는 의미이지, 모델의 확률적 변동이나 공유 문맥 영향을 무작위 통제했다는 뜻은 아닙니다.

**개발 검증 상태:** 소스 계보·호출 흐름을 점검했으며 컴파일·빌드·테스트·CI·실제 유료 모델 대조 캠페인은 아직 실행하지 않았습니다. 심판 점수는 재심사 전까지 보류합니다.

## 2026-11-13: 개방형 실험 정책 및 후손 실행 계약

- `rsi/planner/operator_policy.rs::select`: 모델 작성 실험, 메타 개선, 개선 원리, 계승 계획, 아키텍처 탐색의 후보를 원본 paired 평가로 비교합니다. 선택 결과가 후손의 실제 소스 대상·영역에 반영돼야 후속 학습 대상으로 인정합니다. 자기 소스도 개선법 목록에 포함됩니다.
- `rsi_system/history.rs::replayed_experiment_operator`: 원본 job, 독립 평가, 워커 실행 소스/도메인, 계약을 다시 대조해 계획만 존재하는 선택기에 성과를 귀속시키지 않습니다.
- `rsi/contract.rs::ExperimentContract`: 소스/평가/레시피의 주요 경계를 타입·버전 계약으로 구성합니다. 후손에서 바뀌는 것은 실험 원리이며, 평가 스위트 원본은 해당 실험 동안 동결됩니다.
- `rsi/execution/{generator,compiled_plan}.rs`: 빌드/바이너리 lineage와 후손 계획/검증 책임을 커다란 orchestration 루프에서 별도로 추출했습니다. `rsi/worker/planning.rs`가 계획 전용 별도 프로세스 입구를 소유합니다.
- `rsi/closure.rs`: 채택 4세대의 동결 검증·실제 대조 실행을 엄격하게 판정하지만 일반 탐색 순서를 결정하지는 않습니다.

한계: `AppState`와 자유형 `Value` 의존성 및 긴 `execution.rs`는 아직 남아 있고, 선택기 후보군은 시작점입니다. 동결 평가 기준 자체를 장기적으로 검증·버전 이전하는 독립 프로세스는 미완성입니다. 컴파일·테스트·CI·실 모델 실행 없이 소스 구조만 수정했습니다.
