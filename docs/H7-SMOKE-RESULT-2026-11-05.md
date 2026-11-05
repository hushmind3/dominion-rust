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
