# Champion — source-bound forecast/repair closure (2026-11-13)

## 실제 연결된 실행 경로

1. `src/api/rsi.rs` → `src/engine/jobs.rs` → `src/engine/rsi_evolve.rs` → `src/engine/rsi/execution/campaign.rs`: 캠페인과 독립 세대 작업을 내구성 있게 등록한다.
2. `src/engine/rsi/planner/descendant.rs::plan_generation_from_file`: 새로 컴파일된 후손이 `rsi_system::revalidated_history`의 과거 세대·원본 paired 영수증을 다시 조회한다.
3. `src/engine/rsi_meta_controller.rs::synthesize`: `rsi_prequential::calibrate`로 파일/원본 SHA에 맞는 선택 확률을 갱신하고, `rsi_prequential::drift`가 *실측 재검증된* 반복 예측 오류를 추출하면 예측기 소스 자체를 다음 편집 대상으로 올린다.
4. `src/engine/rsi/execution/worker.rs::worker`: 소스 근거를 동일 후손에서 다시 확인하고 실제 모델·도구 편집 프로그램을 실행한다. 계획만 저장한 행위는 채택으로 세지 않는다.
5. `src/engine/rsi_worktree.rs::compare_at` 및 `rsi/execution/campaign.rs`: 분리된 worktree에서 동결 사례 A/B를 수행하고 채택 결과·컴파일된 후손 계보를 저장한다.
6. `src/engine/rsi_system.rs::revalidated_history`: 후보/작업/평가/원본 paired 메트릭을 재조회하고 `pairedReceiptReplayed`, `metaLearning`, `predictionRepairEvidence`를 재구성한다. 기록에 남아 있던 성공 플래그를 학습 근거로 그대로 믿지 않는다.
7. `src/engine/rsi_prequential.rs::repair_execution_evidence`: 예측기 수정이 **계획됨 → 실제 컴파일된 정책이 제어함 → 후보가 목표 소스를 수정·평가함 → 원본 paired 재검증으로 독립 채택됨**을 각각 구분한다. 마지막 단계의 `sourceIndependentlyAdopted`는 `pairedReceiptReplayed`와 원본 후보의 목표 파일 경로/채택 SHA 일치를 요구한다.

## 바뀐 학습 계약

- `trusted_feedback`: 역사 속 `metaLearning` 성공률·Brier 값이 정책 원래 예측값/실제 경로/채택 여부를 다시 계산한 결과와 같을 때만 학습에 사용한다.
- `calibrate`: 정확히 같은 파일·같은 SHA는 전이 가중치 1.0, 같은 파일의 다른 SHA는 0.25, **다른 파일은 0**. 역할명만 같은 파일의 성공이 무관한 소스의 신뢰도를 올리지 못한다.
- owner가 독립 평가상 eligible인 후보를 채택하지 않은 경우, 실패로 훈련하지 않고 검열(censoring)한다.
- `drift`: 최근 최대 6개의 독립 측정 가능한 출처 결속 예측에서 오차를 새로 계산한다. 원본을 재검증하지 않은 `brierError` 숫자로 자가 수정을 예약하지 않는다.
- `measured_target_attempt`: 단순 focusFiles 기록만으로 자기수정 쿨다운이 발동하지 않는다. 원본 paired 재검증과 실제 변경 경로가 모두 필요하다.
- `predictionRepairEvidence`: source edit 실행 증거와 A/B 평가·채택 영수증을 분리해 영속화한다. 단순 계획·후보 파일 작성은 재귀적 향상 성공으로 간주하지 않는다.

## 근거와 설계 선택 (2026년 공개 자료)

- [Predict Before You Modify, 2026-09-26](https://arxiv.org/abs/2610.13991): 예측 → 수정 → 평가 → 예측기 자체의 갱신. DOMINION에서는 Brier 예측 학습에 원본 소스와 영수증 재검증을 추가했다.
- [RRSI, 2026-09-21](https://arxiv.org/abs/2609.24972): 과적합을 피하기 위한 부품 단위 개선·재사용성·측정 제한. DOMINION은 파일별 전이 범위를 제한하고 독립 채택을 분리한다.
- 서브4의 `rsi_prequential.rs`와 `docs/SUB4-PREQUENTIAL-SOURCE-LEARNING-2026-11-13.md`도 구조 비교 자료로 검토했다. 챔피온의 `controllerPlan` / `metaPolicyExecuted` / `rsi_recursive_closure` 실제 계약은 유지하고, 서로 다른 브랜치의 세대 영수증을 섞지 않았다.

## 검증 경계

이번 변경에서는 요구대로 **테스트·컴파일·빌드·벤치마크·CI·유료 모델 호출을 실행하지 않았다.** 기존 테스트에 더해 원본 예측 변조·owner 비채택·다른 파일 전이·실행 없는 자기수정 의도·가짜 focus 쿨다운의 회귀 fixture를 추가했다. 정적 소스 편집만으로 성능 개선이나 실제 A→B→A 완주를 새로 실측했다고 주장하지 않는다. frozen evaluator, 채택 선택권, 모델 가중치는 변경하지 않았다.
