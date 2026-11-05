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
