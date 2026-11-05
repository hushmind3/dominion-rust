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
