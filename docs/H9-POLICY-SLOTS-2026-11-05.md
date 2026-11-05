# H9 정책 슬롯·버전 관리 (2026-11-05)

H9 전체(알고리즘 후보 검색·메타평가·outer gate) 중 **버전 있는 정책 artifact와
실험 기반 활성화**를 먼저 엔진에 연결했다. 기존 전략 텍스트(improvement)와 달리
정책은 슬롯·revision·파라미터를 가진 교체 가능한 artifact다.

## 1. Artifact

`src/engine/policy.rs`

```
adaptation_policy {
  id, slot, name, revision, status(candidate|active|superseded|rolled_back),
  evidenceGrade(E0|E2), parameters, rationale,
  experimentRevision, activatedAt, rolledBackAt
}
```

- 지원 슬롯: `planning` (추가 슬롯은 SLOTS에 명시).
- `register`는 동일 name+parameters면 idempotent.
- `apply_experiment_result`:
  - `independentUnits >= 8` + `improved|preliminary_support` + delta>0 → `active`
  - 같은 슬롯의 이전 active는 `superseded` (revision 보존)
  - `regressed` → `rolled_back`
  - 표본 부족/불확실 → 후보 유지, `lastExperiment`만 기록

## 2. 실제 소비 경로

`run_turn_body`의 계획 결정이 `planning` 슬롯을 읽는다.

- active 정책이 없으면 기존 `needs_plan` 휴리스틱 그대로.
- `parameters.outline=false` → 계획 호출 생략.
- 적용된 정책은 selection snapshot의 `contextItems`에
  `kind=planning_policy, id, revision, parameters`로 고정된다.
- 따라서 같은 실행이 어떤 정책 revision으로 계획되었는지 재현 가능하다.

## 3. API

- `GET /api/policies` (읽기)
- benchmark 전용:
  - `POST /api/benchmark/policies` {slot,name,parameters,rationale}
  - `POST /api/benchmark/policies/experiment` {policyId, experiment...}

일반 서비스에서는 benchmark 엔드포인트가 404다.

## 4. 검증

- `candidates_need_a_powered_experiment_before_activation`
- `activation_supersedes_the_previous_revision_and_regression_rolls_back`
- `unknown_slots_and_non_object_parameters_are_rejected`
- 전체: `test result: ok. 232 passed` (unit), `33 passed` (E2E)와 함께 당시 기록, `8 passed` (bench).

## 5. 남은 것 (H9 본체)

- 정책 후보를 자동 생성하는 proposer/search loop.
- 정책 전용 격리 실험(planning 품질 지표)과 outer gate.
- 알고리즘 코드 패치 artifact의 컴파일·회귀 검증.
- 메타 평가자 자체의 버전 관리와 leakage 방지.
- 현재는 planning 슬롯 1개만 실제 소비된다.

## 6. Candidate proposer (2026-11-05)

`policy::propose_candidates`가 기록된 실패 cluster에서 정책 후보를 자동 생성한다.

- 입력: 최근 execution 500개의 `failureClass` (cancelled 제외)
- 최소 근거: **같은 class 3건** (`PROPOSER_MIN_EVIDENCE`)
- 규칙:
  - `verification_failed` ≥3 → planning `{"outline": true}` (`plan-before-acting`)
  - `limit_exceeded`/`model_error` ≥3 → planning `{"outline": false}` (`skip-extra-planning`)
- rationale에 근거 execution id를 최대 8개 기록.
- 후보는 `register`로 `candidate` 상태가 되고, 동시에 durable
  `experiment_request{kind:"policy", status:"queued", fixture:"planning_evaluation",
  minIndependentUnits:8}`가 생성된다. revision 단위로 idempotent.
- 호출 지점: finalize `Applied` 직후, 엔진 시작 시.

### Outer gate

활성화 경로는 `policy::apply_experiment_result` 하나뿐이다.

- `independentUnits >= 8` + `improved|preliminary_support` + delta>0 → active
- `regressed` → rolled_back
- 표본 부족/불확실 → candidate 유지
- 정책 실험 요청은 procedure runner가 평가할 수 없으므로 claim에서 건너뛰고
  queued로 보존한다(planning 전용 evaluator는 다음 단계).

### 검증

- `proposer_rejects_clusters_below_the_evidence_threshold` (2건 → 후보/요청 없음)
- `proposer_creates_one_candidate_and_policy_experiment_request` (3건 → 후보 1 + 요청 1, 재실행 idempotent)
- `proposed_candidate_stays_inactive_until_eight_units_and_rolls_back`
  (4 units → candidate, 8 units → active, regressed 8 → rolled_back)

전체: `236 unit + 36 E2E + 8 benchmark` 통과.

## 7. Policy experiment evaluator (2026-11-05)

proposer가 만든 `kind=policy` 요청을 이제 isolated runner가 직접 평가한다.

### Claim / complete

- `claim`은 policy 요청도 claim한다. 정책이 없거나 비활성이면 요청을 failed로
  종결하고, 유효하면 `{request, policy{id,slot,name,revision,parameters}}`를 돌려준다.
- `complete`는 요청 kind에 따라 `policy::apply_experiment_result`를 호출한다.
  따라서 outer gate(>=8 units + 유리한 verdict, regressed → rollback)가 그대로 적용된다.

### Runner policy mode

- claim된 fixture(`planning_evaluation`)로 격리 엔진을 띄운다.
- arm별로 sandbox 정책 상태를 바꾼다.
  - `POST /api/benchmark/policies/state {slot, parameters}` → 후보 register + force activate
  - `POST /api/benchmark/policies/state {slot}` → slot 비활성
- baseline은 정책 없음(기본 휴리스틱), treatment는 후보 파라미터.
- policy trial은 procedure compliance를 요구하지 않는다
  (`requiresCompliance=false`), oracle + taskOutcome + 오류 없음으로 판정한다.
- AB/BA 순서를 유지하면서 trial마다 정책을 켜고 끈다.
- 종료 후 production `complete`로 결과를 보내 정책을 활성화/롤백한다.

### 검증

`policy_experiment_activates_a_candidate_through_the_runner` (E2E):
- production에 planning 후보 등록 + `kind=policy` 요청 queued
- 실제 `dominion-benchmark`를 `BENCH_CLAIM_URL`로 실행, 10 pair
- 결과:
  - 요청 `completed`, `independentUnits=10`
  - 정책 `status=active`, `evidenceGrade=E2`, `parameters.outline=true`
  - delta 1.0, verdict `preliminary_support`
- mock provider는 계획 프롬프트/계획 포함 여부에 따라 행동을 달리해 정책 효과를
  결정적으로 재현한다.

### 한계

- runner의 `pair_effect`는 conservative bound 때문에 **실질적으로 10 pair 이상**에서만
  `preliminary_support`가 된다. outer gate의 최소값은 8이지만, 8~9 pair는
  `inconclusive`라 활성화되지 않는다.
- planning 품질 자체(계획의 유용성)를 평가하는 semantic metric은 아직 없다.
  현재 지표는 동일 task의 성공률이다.
- policy trial은 정책 on/off를 trial마다 토글하므로 콜드 스타트 편향이 없다고
  주장하지 않는다.

전체: `236 unit + 37 E2E + 9 benchmark` 통과.

## 8. Semantic planning metric (2026-11-05)

파일 성공률만으로 계획 정책을 평가하지 않는다. `engine/plan_quality.rs`가
계획 텍스트를 구조와 과제 관련성으로 채점한다.

### 점수

- structure (0.55 가중)
  - 번호/불릿 단계 수: 2~6 최고, 1/7~8 감점, 0이면 0
  - 검증 키워드(검증/확인/verify/check/test/assert) 포함 시 +0.2
  - 20자 미만/2,000자 초과 감점
- relevance (0.45 가중)
  - 요청과 계획의 토큰 겹침 비율
  - 요청의 구체 경로/파일명이 계획에 있으면 가중
- `total = 0.55*structure + 0.45*relevance` (0..1, 4자리 반올림)
- `metric = plan_quality_v1`, 결정적이며 모델 비의존.

### 실행 기록

계획이 실제로 사용되면 execution에 다음을 저장한다.

- `plan` (최대 4,000자)
- `planQuality {metric,total,structure,relevance,steps,hasVerification,chars}`
- `planPolicyId`, `planPolicyRevision` (정책이 계획을 유도한 경우)

### Runner 지표

- 각 trial에 `planQuality`, `planSteps` 기록
- arm aggregate에 `planQuality` 평균 추가
- pair effect에 `semanticMetric`, `semanticDelta`, `semanticCi`,
  `semanticUnits` 추가
- policy request completion에 semanticDelta를 포함

### Outer gate

planning 슬롯 활성화 조건:

1. `independentUnits >= 8`
2. `verdict`가 `improved|preliminary_support`, delta > 0
3. **`semanticDelta >= 0.05`** (`MIN_SEMANTIC_DELTA`)

semantic 조건을 못 넘으면 candidate를 유지하고
`semanticVerified=false`, `policy.semantic_gate_rejected` audit를 남긴다.

### 검증

- `plan_quality` 단위 테스트 3개 (구조/관련성/결정성)
- `policy_effect_reports_semantic_plan_delta`
- `planning_activation_requires_both_units_and_semantic_improvement`
  (units 8 + semantic 0 → candidate, units 4 + semantic 0.4 → candidate,
  units 8 + semantic 0.2 → active)
- E2E `policy_experiment_activates_a_candidate_through_the_runner`:
  runner가 계획 포함 여부에 따라 다르게 동작하는 mock으로 10 pair 실행,
  request completed, `semanticVerified=true`, semanticDelta >= 0.05,
  정책 active E2.

### 한계

- 휴리스틱 점수이며 계획의 실제 실행 가능성/정확성을 의미하지 않는다.
- 한국어 토큰화는 공백/구두점 기반이라 조사가 붙은 토큰은 일치하지 않을 수 있다.
- semantic CI는 아직 게이트에 사용하지 않고 delta만 사용한다.
- 메타 평가자(평가자 자체의 품질) 분리는 아직이다.

전체: `240 unit + 38 E2E + 10 benchmark` 통과.

## 9. Evaluator-independent meta-evaluation (2026-11-05)

### 버전 분리

- main evaluator: `plan-quality-main` v1, metric `plan_quality_v1`
  (단어 토큰 + 번호 단계)
- independent evaluator: `plan-quality-independent` v1,
  metric `plan_quality_independent_v1`
  (문자 bigram + 문장/라인 구조)
- 두 점수 모두 execution에 저장된다:
  `planQuality`, `planQualityIndependent`.

### Runner → production 재계산

runner는 policy trial마다 raw `plan`(최대 4,000자),
`planQuality`, `planQualityIndependent`를 기록하고 completion에 다음을 보낸다.

- `semanticEvaluator {id,version}`
- `independentEvaluator {id,version}`
- `task`
- `plans [{pair,arm,plan}]`

production `complete`는 runner가 주장한 semanticDelta를 그대로 믿지 않는다.

1. evaluator id/version이 현재 서버 버전과 다르면 승격 거부
2. `plans`가 없으면 승격 거부
3. main/independent evaluator로 **직접 재계산**
4. 둘 중 하나라도 `MIN_SEMANTIC_DELTA(0.05)` 미만이면 승격 거부
5. 통과 시 재계산 값으로 policy outer gate 호출

`policy::apply_experiment_result`도 planning 슬롯에서
`semanticDelta >= 0.05` **그리고** `independentSemanticDelta >= 0.05`를 요구한다.

거부되어도 request는 `completed`로 남고 `promotionError`에 사유가 기록된다.
candidate는 active가 되지 않는다.

### 검증

- `the_independent_evaluator_shares_the_direction_but_not_the_algorithm`
  (main과 metric/evaluator가 다름, 좋은 계획 > 모호한 계획)
- `planning_activation_rejects_an_independent_evaluator_disagreement`
  (main 0.4 / independent 0 → candidate, 둘 다 0.2 → active)
- `policy_effect_reports_semantic_plan_delta`
- E2E `policy_activation_is_rejected_when_the_independent_evaluator_disagrees`
  - evaluator version 999 → request completed, promotionError=version, candidate 유지
  - 올바른 버전 + 양 arm 동일 계획 → independent delta 0 → `semantic gate rejected`
  - 올바른 버전 + 개선 계획 → active, `independentSemanticDelta >= 0.05`
- 기존 runner 정책 E2E도 production 재계산을 통과해 active.

전체: `244 unit + 39 E2E + 14 benchmark` 통과.

### 한계

- independent evaluator도 heuristic이다. 외부 인간/별도 모델 평가자는 아니다.
- evaluator 버전은 코드 상수이며 배포 버전 관리/서명은 아직 없다.
- semantic CI는 여전히 게이트에 쓰지 않는다.

## 10. Policy search (2026-11-05)

### 후보 공간

proposer는 failure cluster마다 하나가 아니라 **여러 revision**을 제안한다.

- `verification_failed`:
  - `plan-before-acting` `{outline:true}`
  - `plan-only-long-tasks` `{outline:true, minChars:200}`
  - `skip-extra-planning` `{outline:false}`
- `limit_exceeded`/`model_error`:
  - `skip-extra-planning` `{outline:false}`
  - `plan-only-long-tasks` `{outline:true, minChars:400}`

각 revision은 같은 `searchId`로 묶이고, **revision마다 실험 요청 1개**가 생성된다.
`register`/요청 생성은 idempotent이며 두 번째 proposer 실행은 새 항목을 만들지 않는다.

### 정책 소비 확장

`policy::outline_decision(parameters, input_chars)`:

- `outline=false` → 항상 계획 없음
- `outline=true` + `minChars` → 입력 길이가 임계값 이상일 때만 계획
- 파라미터가 없으면 기존 `needs_plan` 휴리스틱 유지

### Search selection

각 실험 완료 시 `policy::select_best(searchId)`가 실행된다.

- eligible: `units>=8`, verdict `improved|preliminary_support`,
  main semanticDelta>=0.05, independent semanticDelta>=0.05
- score = **min(main, independent)** (보수적)
- 최고 score revision만 active
- 같은 search의 다른 active revision은 `superseded`
- 부적격 candidate는 candidate로 남는다(활성화하지 않음)
- `policy_search {id, selectedPolicyId, selectedScore, candidates[]}` 기록
- API: `GET /api/policy-searches`, benchmark 전용 `POST /api/benchmark/policy-search`

### 검증

- proposer 테스트: cluster 1개 → 후보 3 + 요청 3, 같은 searchId, 재실행 0
- `outline_thresholds_control_planning`
- `search_selection_keeps_the_best_independent_score_only`
  (min 점수 기준 선택, 열등 active superseded, 부적격 candidate 유지)
- E2E `policy_search_selects_the_best_revision_through_the_scheduler`
  - production scheduler + runner로 후보 2개(`outline:true`, `outline:true,minChars:1000`)를
    실제 10-pair 실험
  - 둘 다 request completed
  - 계획을 만든 `outline-all`만 active로 선택
  - 짧은 작업에서 계획하지 않는 `outline-only-long`은 active 아님
  - `policy_search`에 후보 2개와 selectedPolicyId 기록

전체: `246 unit + 40 E2E + 14 benchmark` 통과.

### 한계

- 후보 공간은 고정 heuristic 2~3개다. beam search나 파라미터 연속 탐색은 아직.
- search는 failure class 단위이며, 검색 간 전역 최적 선택은 하지 않는다.
- selection은 이미 평가된 revision만 비교한다.

## 11. Algorithm patch artifacts (2026-11-05)

정책 파라미터 변경을 **버전 있는 patch artifact**로 분리했다.

### Registry

`policy_patch`:

```
{id, slot, targetPolicyId, baseRevision, baseParameters, body, rationale,
 status candidate|active|superseded|rolled_back|rejected,
 evidenceGrade, requiresExperiment,
 lastExperiment{units,verdict,semanticDelta,independentSemanticDelta},
 appliedPolicyRevision, rolledBackAt}
```

- 등록 조건: slot 일치, 대상 정책이 `active`, body.parameters 객체.
- id는 `(slot,targetPolicyId,baseRevision,body)` stable hash로 idempotent.
- `baseParameters`를 함께 저장해 rollback 시 원복할 수 있다.

### Regression check (base compatibility)

activation 시점에 대상 정책이

- `baseRevision`과 같은 revision이고
- 여전히 `active` 상태

여야 한다. 다른 revision으로 바뀌었거나 다른 정책이 active가 되었으면
patch를 `rejected`로 종결하고 `policy.patch_base_mismatch` audit를 남긴다.
revision이 다른 정책에 patch를 조용히 적용하지 않는다.

### Activation gate

- `independentUnits >= 8`
- verdict `improved|preliminary_support`, delta > 0
- main semanticDelta >= 0.05 **그리고** independent semanticDelta >= 0.05
통과 시:
- body.parameters를 base parameters에 merge
- 정책 revision을 새로 계산하고 `appliedPatchId`를 기록
- 같은 slot의 다른 active patch는 `superseded`
- patch `active`, `appliedPolicyRevision` 기록

`regressed`(units>=8)이면 patch를 `rolled_back`으로 바꾸고 정책 parameters를
`baseParameters`로 복원한 뒤 새 revision을 만든다.

### API

- `GET /api/policy-patches`
- benchmark 전용:
  - `POST /api/benchmark/policy-patches`
  - `POST /api/benchmark/policy-patches/experiment`

### 검증

- `a_patch_needs_both_evaluators_and_eight_units`
  (4 units → candidate, independent 0 → candidate, 둘 다 통과 → active + minChars 적용)
- `a_patch_against_a_stale_base_revision_is_rejected`
- `a_regressed_patch_restores_the_base_parameters`
- E2E `policy_patches_activate_rollback_and_reject_stale_bases`
  - 등록 → 4 units 거부 → 8 units + 두 evaluator → active, 정책 minChars=120
  - regressed → rolled_back, 정책 outline=true/minChars 없음
  - 다른 revision이 active가 된 뒤 남은 patch → **rejected**

전체: `249 unit + 41 E2E + 14 benchmark` 통과.

### 한계

- patch body는 현재 정책 **parameters**만 바꾼다. Rust 알고리즘 코드 patch를
  컴파일·검증하는 artifact는 아직 아니다.
- runner가 patch 실험을 자동 생성/수행하지 않는다(현재는 benchmark API로 결과 적용).
- patch 서명, 배포 버전 관리, 의존성 검사는 아직.
