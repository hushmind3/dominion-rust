# Sub1 — compiled RSI closure evidence (2026-11-13)

## 실행 경로와 증거

- `src/api/rsi.rs::start` → `jobs::enqueue` → `jobs::run_job` → `rsi_evolve::run`.
- `rsi/execution.rs::run_loop`에서 현재 실험 소스의 바이너리를 빌드·검증하고 `--rsi-worker-plan`에서 새 계획을 받아 모델·도구 편집 실행기로 전달합니다.
- `rsi_worktree::compare_at`는 독립 동결 사례의 A/B 결과와 SHA-256 영수증을 남기고, `rsi_evolve::adopt`는 채택 후보를 분리된 실험 worktree에 반영합니다. 다음 세대는 실제 채택 커밋으로 다시 빌드됩니다.
- `rsi_worktree::classify`와 자기 모델 소스 인벤토리가 `rsi/meta_controller/` 및 `rsi/system/` 하위 파일을 개선기 코드로 식별합니다.
- `rsi/proof.rs::recursive_closure_report`는 실제 3세대 A→B→A 변경, 출처 해시, 컴파일된 실행기, 실행된 정책, 독립된 개선 증거를 묶어 `rsi_campaign.recursiveProof`에 남깁니다. 캠페인 API의 모든 조회에서 원본 후보 영수증을 재검증합니다.

## 인정 범위

`closedLoopObserved=true`의 필요 조건: A(메타 소스 수정/채택) → B(새 컴파일된 후손이 다른 능력을 개선) → A(그 후손의 후손이 메타 방법을 다시 수정/채택), 연속 커밋, 실제 바이너리 정체성, 원본 독립 동결 홀드아웃 성과, 교차 능력 향상.

복제한 성공 기록·단순한 3연속 채택·실행되지 않은 계획·가짜 소스 해시로는 통과할 수 없습니다. 이는 관측된 재귀 계보를 증명하기 위한 조건이지, 개선 원인의 완전한 인과 귀속이나 무한한 자기개선 보증이 아닙니다. `causalAblationProven=false`로 표기합니다.

이번 브랜치 수정에서는 cargo·테스트·빌드·CI·벤치마크·실모델 캠페인을 **실행하지 않았습니다**. 추가 실증은 동등 입력의 기존/변경된 개선기 계획 비교와 3세대 연속 실행·독립 홀드아웃 성과 관찰입니다.

## 2026년 근거 자료

- HyperAgents (2026-03, 2026-11 개정): https://arxiv.org/abs/2603.19461
- AIDE² (2026-09): https://arxiv.org/abs/2609.26457
- Predict Before You Modify (2026-09): https://arxiv.org/abs/2610.13991
