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
