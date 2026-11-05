# H7 실제 모델 smoke 결과 — 2026-11-05

대상: `qwen3:4b` (Ollama, `http://127.0.0.1:11434/v1`, temperature 0, thinking false)
실행기: `target/release/dominion-benchmark` (격리 data/workspace/port, `DOMINION_BENCHMARK=1`)
fixture: `bench/harness_report.txt` + `bench/harness_audit.txt`, `workspace_write` 2노드 DAG
oracle: `s7-harness-audit-v1`

## 1. 결과

### 3 pair (6 trials)

| arm | assigned | strict success | compliance | failure | p50 | tokens |
|---|---|---|---|---|---|---|
| baseline (model_guided) | 3 | **0/3 (0.0)** | 0 | 3 | 19,379ms | 9,034 |
| treatment (harness_assisted) | 3 | **3/3 (1.0)** | 3 | 0 | 18,568ms | 9,952 |

- experimentId: `0a130c9c-e72e-4b6e-9d3b-487de2f9adb0`
- effect: delta 1.0, CI [0.0, 1.0], independentUnits 3,
  verdict **inconclusive**, note "Small N is inconclusive; this is not E4 evidence."
- baseline은 `report` 파일만 만들고 `audit` 노드를 실행하지 않아
  strict success(두 파일 + taskOutcome=succeeded + compliance)가 0이었다.
- treatment는 두 노드를 모두 실행했고 compliance도 3/3이었다.

### 1 pair (2 trials, 이전 실행)

- experimentId: `b70ff57a-73b4-41ec-a5c5-8599773dbbc6`
- baseline 0/1, treatment 1/1, verdict inconclusive.

### 관찰

- 1 pair 실행에서 treatment p50이 89초였고, 3 pair 실행에서는 23초/17초/19초였다.
  모델 서버 상태(모델 로딩/캐시)에 따른 변동이 크므로 시간 비교는 아직 의미가 없다.
- 두 실행 모두 `mode: smoke`로 기록되었다.

## 2. 이 결과가 증명하는 것 / 증명하지 않는 것

증명하는 것 (이 fixture 범위):

- 격리 엔진에서 실제 4B 모델로 baseline과 treatment가 paired로 실행되었다.
- treatment 경로에서 DAG 두 노드가 실제로 실행되고, oracle과 compliance가
  통과해야만 strict success가 된다.
- 이 fixture에서 model_guided는 두 번째 노드를 스스로 실행하지 않았다.

증명하지 않는 것:

- 일반 작업 성공률 향상. 단일 fixture, N=3, holdout 없음, smoke 모드다.
- 학습/전이 효과. treatment는 고정 절차 실행 보조이며, 모델이 새로운 작업을
  스스로 개선했다는 증거가 아니다.
- 비용/시간 우위. 표본이 작고 모델 로딩 변동이 크다.
- 다른 모델·다른 과제로의 일반화.

## 3. 다음 측정 조건 (H7)

- fixture를 최소 3종(파일 생성, 조회 후 수정, 실패 복구)으로 늘리고 holdout을 둔다.
- pair 수를 늘려 Clopper-Pearson/Wilson 구간이 0/1을 벗어나게 한다.
- fixture 승인 resolver를 고정하고, 재시작/중단 trial을 `Incomplete`로 분리한다.
- 모델 로딩 워밍업을 trial 전에 수행해 시간 비교를 분리한다.
- 결과는 baseline/treatment의 raw trial JSON과 함께 보존한다.

## 4. H3 gateway 적용 후 재측정 (2026-11-05)

H3 공통 gateway/admission/usage 변경 후 같은 fixture를 다시 실행했다.

| arm | assigned | strict success | compliance | p50 | tokens |
|---|---|---|---|---|---|
| baseline | 3 | 0/3 | 0 | 22,895ms | 9,414 |
| treatment | 3 | 3/3 | 3 | 34,336ms | 11,811 |

- experimentId: `c65f18fa-8a71-4eae-9e98-cfefca0cf0e7`
- verdict **inconclusive**, CI [0.0, 1.0], `mode: smoke`
- `model.usage` audit 12건이 실제로 기록되었다.
- treatment가 더 느렸다(34.3s vs 22.9s). 로컬 모델 admission 직렬화와 실행 단계 수
  차이 때문일 수 있으나, 표본이 작아 원인을 단정하지 않는다.
- 성공률 차이 0/3 vs 3/3은 이전 측정과 일치하지만 여전히 성능 개선 주장이 아니다.

## 5. real 10-pair 측정 및 승격 (2026-11-05)

H8 폐쇄 루프 연결 후 `BENCH_MODE=real BENCH_PAIRS=10`으로 실행했다.

| arm | runs | strict success | compliance | p50 | tokens |
|---|---|---|---|---|---|
| baseline | 10 | 0/10 | 0 | 19,063ms | 28,846 |
| treatment | 10 | 10/10 | 10 | 25,998ms | 37,854 |

- experimentId: `4c75a29d-e695-430e-a203-a691e9141280`
- effect: delta 1.0, CI [0.141, 1.0], units 10, verdict `preliminary_support`
- runner가 실험 결과를 엔진에 제출해 treatment 전략이 실제로
  `validated / E2 / isolated_experiment`로 승격되었다.
- 상세: `docs/H8-CLOSED-LOOP-2026-11-05.md`.
- 여전히 단일 fixture이며 일반 성능 주장이 아니다.

## 6. 다중 fixture / holdout 지원 (코드)

runner는 이제 `BENCH_FIXTURE`로 작업군을 선택한다.

- `audit_report` (기본): write audit → write report, 2노드 쓰기 DAG.
- `update_config`: read `bench/config_input.txt` → write `bench/config.txt`,
  oracle은 `config.txt == mode=final` 이고 `config_input.txt == mode=draft` 유지.

`BENCH_HOLDOUT=1`이면 manifest에 `evaluationRole=holdout`을 기록한다.
fixture task와 oracle expected는 manifest/snapshot에 함께 저장된다.

아직: holdout 전용 fixture 예약, fixture별 최소 표본 규칙, 작업군 간 전이 평가.

## 7. update_config fixture 실제 측정 (2026-11-05)

### 7.1 첫 실행: 전부 timeout (실패 사례)

초기 fixture는 read-only 입력도 `.txt`였고, task 문장에 입력/출력 경로가 모두
있었다. 엔진의 acceptance 추론이 **입력 파일도 변경되어야 한다**고 판단해
`config_input.txt`가 그대로면 검증이 영원히 실패했다. 모델은 6~7 라운드를
반복했고 300초 client timeout / 600초 engine 제한에 걸렸다.

추가로 모델이 파일 검증에 쓰는 `shell_exec`가 benchmark 정책에 없어
approval 120초 대기가 발생했다.

이 실행의 결론: **not_evaluated**, trial 8개 전부 strict success 0.

### 7.2 수정

1. `verification::infer_criteria`: 읽기 동사(읽/확인/조회/read/check)와 쓰기 동사가
   함께 있고 경로가 2개 이상이면 **마지막 경로만** 출력으로 추론한다.
   회귀 테스트 `reading_an_input_does_not_require_it_to_change`.
2. benchmark 격리 sandbox 정책에 `shell_exec`를 scoped auto-approve로 추가
   (임시 workspace 전용).
3. fixture 입력을 `config_input.cfg`로 바꿔 자동 추론 대상에서 제외(방어적).

### 7.3 재측정 (2 pair, real)

| arm | runs | strict success | compliance | p50 | tokens |
|---|---|---|---|---|---|
| baseline | 2 | 0/2 | 0 | 112,471ms | 15,003 |
| treatment | 2 | 2/2 | 2 | 65,157ms | 7,904 |

- delta 1.0, CI [0.0, 1.0], units 2, verdict `inconclusive`
- runner가 승격을 시도했으나 `units < 8`이라 **승격되지 않음**
  (출력: `promotion: status=adopted evidence=E0`)
- oracle: `config.txt=mode=final`, `config_input.cfg=mode=draft` 유지 확인
- treatment가 baseline보다 빨랐다(65s vs 112s). 표본 2라 단정하지 않는다.

### 7.4 해석 제한

- strict success는 standing procedure 준수를 포함한다. baseline(model_guided)도
  같은 절차를 주입받지만 DAG를 결정적으로 실행하지 않아 compliance가 0이다.
  즉 이 delta는 **"고정 절차 실행 보조"의 효과**이며 모델의 일반 능력 향상이 아니다.
- fixture마다 task/oracle이 다르므로 두 fixture 결과를 합쳐 일반화하지 않는다.

## 8. Current-HEAD real 4B measurements + full loop (2026-11-05)

### 8.1 측정 중 발견한 회귀

fixture 파라미터화 과정에서 `seed_verification`의 `appliesTo`가
`bench/harness_report.txt`(task 대상)가 아니라 `bench/harness_audit.txt`로
바뀌어 있었다. 그 결과 harness-assisted treatment도 procedure가 적용되지 않아
첫 10-pair 실행에서 양 arm 모두 strict success 0이 나왔다(실행 기록의 모든
operation이 `origin=model`).

수정: `appliesTo`를 task 대상인 `report_path`로 되돌림.
1-pair 재확인에서 treatment success=true, audit/report=true를 확인한 뒤 재측정했다.

### 8.2 audit_report, 10 pair real

- experimentId: `31fff435-b3af-4ca8-8ad0-cf599398f796`
- model: `qwen3:4b`, temperature 0, thinking false
- verdict: **preliminary_support**, CI [0.141, 1.0], units 10

| arm | success | compliance | p50 | tokens |
|---|---|---|---|---|
| baseline | 0/10 | 0 | 22,570ms | 33,136 |
| treatment | **10/10** | 10 | 28,705ms | 42,337 |

- runner 출력: `promotion: status=validated evidence=E2`
- treatment가 더 느리고 토큰도 더 쓴다.

### 8.3 update_config, 4 pair real

- experimentId: `60a652ac-9555-45f5-877a-d3e2e733f4f1`
- verdict: **inconclusive** (units 4)

| arm | success | compliance | p50 | tokens |
|---|---|---|---|---|
| baseline | 0/4 | 0 | 138,594ms | 29,639 |
| treatment | **4/4** | 4 | 94,075ms | 24,330 |

- 표본이 4 pair라 승격되지 않음 (`promotion: status=adopted evidence=E0`).
- 두 fixture 모두 strict success는 procedure compliance를 포함하므로,
  이 delta는 "고정 절차 실행 보조" 효과다.

### 8.4 Full loop: production scheduler → runner → promotion (real model)

격리된 production 엔진(temp data/workspace/port, `DOMINION_BENCHMARK=1`)에
scheduler env를 주고 strategy experiment request를 queued로 생성했다.
수동 runner 호출은 하지 않았다.

- production experimentId: `c951b5a3-adbb-4979-a43f-4ef96334b464`
- request: `completed`
- scheduler run: `status=success`, `exitCode=0`, `releasedClaims=0`
- runner stdout: `request completed: completed promotion=E2`
- strategy: `status=validated`, `evidenceGrade=E2`,
  `validatedBy=isolated_experiment`, `rollout=canary`,
  `lastExperiment.units=10`, verdict `preliminary_support`
- trial evidence: `${TMPDIR}/dominion-s7-eb871934-2887-4a53-9dcc-ec949faa8b56`
  - `trials.jsonl` 20줄, 성공 10
  - `manifest.json`, `summary.json`, `REPORT.md`, `artifacts/` 보존

측정 중 runner가 띄우는 격리 엔진이 부모의 scheduler env를 상속해 **nested
scheduler/runner**를 만드는 문제도 발견했다. runner가 격리 엔진에
`DOMINION_EXPERIMENT_SCHEDULER=0`, `DOMINION_BACKUP_INTERVAL_SECS=0`을
명시하도록 수정했다.

### 8.5 한계

- Full loop에서 실제로 승격된 것은 **strategy**이며 oracle/compliance/success
  gate를 사용한다. semantic evaluator gate는 policy 승격 경로에만 적용된다.
- semantic-gated policy 승격은 deterministic E2E(`policy_experiment_activates_...`)로
  검증되어 있지만, 실제 4B 모델에서 policy 승격까지 도달한 결과는 아직 없다.
- update_config는 4 pair뿐이라 inconclusive다.
- 단일 머신/단일 fixture군이며 holdout 전이 평가는 아직 없다.
