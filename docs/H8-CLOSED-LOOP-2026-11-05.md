# H8 폐쇄 루프 — 2026-11-05

이 문서는 실제로 연결된 자율개선 경로와 아직 아닌 것을 구분한다.

## 1. 증거 등급 분리

- 생산 관찰은 `E1_observed`다. `execution_updates`가 `uses >= 5`에 도달해도
  `evidenceGrade=E2`를 주지 않는다. 대신 `status=validated`,
  `validatedBy=production_observation`, `requiresExperiment=true`를 기록한다.
- `E2`는 `improve::apply_experiment_result`만 부여한다. 조건:
  `independentUnits >= 8` + 유리한 verdict + 양수 delta.
- 불리한 실험은 `rolled_back` + `E2` + `experimentRevision`을 기록한다.
- 표본 부족/불확실 실험은 `lastExperiment`만 남기고 승격하지 않는다.

## 2. 실제 실험 → 승격 경로

```
benchmark runner (BENCH_MODE=real)
  → paired trials + oracle/compliance
  → pair_effect(단위=pair, conservative bound)
  → POST /api/benchmark/experiments   (benchmark 전용)
  → improve::apply_experiment_result
       ├─ preliminary_support/improved + units>=8 → validated/E2/isolated_experiment
       ├─ regressed + units>=8 → rolled_back/E2
       └─ 그 외 → lastExperiment만 기록, 승격 없음
```

- `POST /api/benchmark/experiments`는 `DOMINION_BENCHMARK=1`에서만 존재한다.
  일반 서비스에서는 404다.
- runner는 `BENCH_MODE=real`일 때만 이 엔드포인트를 호출한다. smoke는 호출하지
  않는다.
- 실험 revision은 `experimentId + oracleVersion + units + delta`의 stable hash다.

## 3. 실제 4B 실행 증거 (2026-11-05)

조건: `qwen3:4b`, temperature 0, thinking false, 10 pair(20 trials), fixture
`bench/harness_audit.txt` + `bench/harness_report.txt`.

| arm | runs | strict success | compliance | p50 | tokens |
|---|---|---|---|---|---|
| baseline (model_guided) | 10 | **0/10** | 0 | 19,063ms | 28,846 |
| treatment (harness_assisted) | 10 | **10/10** | 10 | 25,998ms | 37,854 |

- experimentId: `4c75a29d-e695-430e-a203-a691e9141280`
- effect: delta 1.0, CI **[0.141, 1.0]**, independentUnits 10,
  verdict **preliminary_support**
- runner 출력: `promotion: status=validated evidence=E2`
- 엔진 DB 확인:

```
status=validated
evidenceGrade=E2
validatedBy=isolated_experiment
requiresExperiment=0
lastExperiment.verdict=preliminary_support
lastExperiment.independentUnits=10
experimentRevision=47fcad5b460bcbcccee0aee154fe91b3
```

- audit: `improvement.experiment_result`에 같은 experimentId/units/verdict가 기록됨.

## 4. 이 증거의 한계

- 단일 fixture다. holdout과 다른 작업 유형이 없다.
- treatment는 **고정 절차 실행 보조**다. 모델이 스스로 새로운 전략을 생성해
  실험한 것이 아니다.
- baseline 0/10은 이 fixture에서 모델이 두 번째 노드를 스스로 실행하지 않았다는
  뜻이며, 일반 작업 능력의 0%가 아니다.
- 비용은 treatment가 더 높다(tokens 37,854 vs 28,846, p50 26.0s vs 19.1s).
- 따라서 "성능 개선" 주장은 fixture 범위의 `preliminary_support`로 제한한다.

## 5. 아직 아닌 것

- 새 전략 후보를 **자동으로 실험에 배정**하는 scheduler (현재는 runner가 수동 실행).
- canary 단계(일부 트래픽만 적용)와 rollback 자동 트리거.
- holdout fixture 및 다중 작업군 전이 평가.
- 후보 생성 프롬프트/알고리즘의 자동 개선(H9).
