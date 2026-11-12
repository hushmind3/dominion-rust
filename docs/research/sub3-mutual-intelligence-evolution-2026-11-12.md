# 서브3: 지능 구성요소 간 상호발전·후손 인과 학습 연결 (2026-11-12)

## 문제

단일 파일의 독립 채택률이나 메타 개선기의 예측 오차만 학습하면 **한 지능 요소의 개선이 다른 요소의 개선 능력에 끼친 영향**이 사라진다. 고립된 성공 확률을 최적화하면 하네스→기억→개선기 같은 상호 진화가 단순한 파일 교체 목록으로 축소된다.

2026년 공개 자료 [HyperAgents](https://ai.meta.com/research/publications/hyperagents/), [AEL](https://aclanthology.org/2026.findings-emnlp.1316/), [EvoTool](https://syang2000.github.io/ACL_2026_EvoTool/) 및 [Agentic Harness Engineering](https://arxiv.org/abs/2604.25850)은 변경 가능한 메타 제어, 하네스·기억 정책 진화, 긴 실행 계보의 모듈별 기여도 문제를 다룬다. 본 구현은 논문의 모델이나 숫자를 그대로 복제하지 않고 DOMINION의 기존 독립 평가·수동 생산 반영 경계에 맞게 새로 작성했다.

## 구현 위치와 연결

1. `src/engine/rsi.rs`: 진화 대상의 실제 후보 등록 경로에 `rsi_meta_controller.rs`, `rsi_system.rs`, `rsi_interaction.rs`를 포함했다. 생성한 메타 개선기를 정작 개선 후보로 제출할 수 없던 제한을 제거했다.
2. `src/engine/rsi_evolve.rs`: `replayed_pair_metrics`가 성공한 독립 측정과 실패한 독립 측정을 동일한 출처 검사로 확인한다. `pairedMeasured`와 채택 조건인 `pairedRevalidated`를 분리 기록한다. 컴파일 실패·미측정 발언은 측정 학습에 포함하지 않는다.
3. `src/engine/rsi_interaction.rs`: `influence_graph`는 실제 독립 A/B 계보에서 **변경된 구성요소 역할 → 평가된 구성요소 역할**로 방향성 관측 관계를 생성한다. 한 번에 여러 모듈을 수정한 경우는 **교란된 결합 관측**으로 따로 기록하며 특정 1개 모듈에 독립 공로를 주지 않는다. 모델 작성/네이티브 작성 출처와 무관하게 독립 측정이 있는 실제 소스를 사용한다.
4. `src/engine/rsi_meta_controller.rs`: 후손의 수정 가능한 메타 선택 알고리즘이 개별 파일 성공/실패·후손 개선 생산성·토큰 비용뿐 아니라 **영역 간 독립 측정 향상과 회귀**를 자신의 메타 점수에 사용한다. 실패한 측정값도 성공 확률 보정에 반영한다.
5. `src/engine/rsi_interaction.rs`: `coupled_portfolio`는 개선할 주 대상과 서로 다른 지능 역할의 실제 소스 최대 3개를 현재 SHA-256에 결속해 함께 선택한다. 관측 효과가 없으면 미검증 탐색으로 명시하고, 영역 간 영향이 측정됐다면 그 근거를 연결한다.
6. `src/engine/rsi_evolve.rs`: **새로 빌드한 후손 실행기**에서 influence graph → meta policy → co-design portfolio → source edit focus 순서로 계획한다. 이전 코디네이터는 모델 계획·소스 해시·이력의 출처를 점검하고 재계산하지 않는다. 작업 worker는 동일 포트폴리오를 받은 뒤 `rsi_system_plan(component=system)`으로 복수 구성요소를 실제 재설계하도록 안내한다.
7. `src/engine/rsi_system.rs`: 세대 계승 증거에 `crossSubsystemPairedEffects`, `reciprocalSubsystemPairsObserved`, 영향 그래프 SHA를 남겨 단순 구성요소 수정과 독립 측정된 상호 발전을 별도 표시한다.

## 다음 세대 재귀 구조

```text
Vn: 전체 소스 자기모형과 기존 설계 계보 읽기
  → 성공·실패의 독립 A/B 측정에서 영역 간 영향 관측
  → Vn의 수정 가능한 메타 개선기가 목표·복합 설계 후보 선택
  → Vn의 개선 도구가 여러 모듈의 소스를 실제 변경
  → 기존 외부 감독기가 동결 사례로 검증, 실험상 채택 여부 결정
  → Vn+1이 자기 모듈·메타 개선기·도구 정책을 실행
  → Vn+1이 독립 검증된 이득/손실과 계승 프로그램으로 재설계
  → Vn+2 ... (생산 반영은 사용자 결정)
```

## 의미와 검증 경계

- **독립 A/B 결과의 방향성 관측**을 인과 학습의 기초 자료로 삼는다. 동시 변경으로 인한 기여도 모호성을 숨기지 않는다. 엄격한 원인 식별에는 후속 단일 구성요소 ablation 같은 별도 독립 실험이 필요하다.
- `causalityProven=false`를 유지한다. 설계 제안, 개선 확률, 소스 수정 자체는 성능 향상의 증거로 계산하지 않는다.
- 외부 동결 평가기, 평가 프로필, 생산 반영 권한은 그대로 유지했다.
- 사용자 요구에 따라 이번 구조 작업에서는 **CI·완주 실험·유료 모델 호출을 진행하지 않았다**. 소스 연결·GitHub 푸시 상태이고, Rust 컴파일 및 실제 반복 후손 성능 검증은 아직 별개다.
