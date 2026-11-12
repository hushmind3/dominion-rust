# 서브3 — 심판의 ‘고정 단계·상위 개선 원리 미발명’ 감점에 대응하는 개방형 RSI 개선 프로그램 (2026-11-13)

## 감점 반영 기준과 구현 결손

최신 심판 판정: 챔피온 72점, 서브3 71점, 서브4 69점. 80점 이상 없음. CI 실행 자체는 감점 대상에서 제외한다. 서브3의 강점은 실제 변경 소스·독립 채택·후손 바이너리와 연결된 메타→기능→메타 개선 계보였다. 감점 사유는 고정된 단계에 의존하며 후손이 새로운 개선 원리·메타개선 구조를 일반적으로 발명하는 능력이 부족하다는 점이었다.

서브3가 이전에 만든 `rsi_reflexive.rs`의 3단계 및 5단계 경로만 연장하면 같은 구조의 반복에 머문다. 이번 변경은 이를 **기본/기존 경로**로 유지하고, 대신 모델이 새로운 개선 프로그램의 연결 구조 자체를 저술할 수 있는 상위 계층을 추가한다.

## 새로운 핵심: 모델 저술형 재귀 개선 프로그램 그래프

- `src/engine/rsi_program_graph.rs`: 변경 가능한 Rust 일반 소스. 모델이 새로운 개선 그래프를 `rsi_system_plan.openProgram`에 작성한다.
- 그래프는 정해진 A→B→A 순서가 아니다. `entry`, 임의로 명명된 노드, `nodes[{id, role, domain, path, objective, mechanism}]`, `edges[{from,to,when}]`으로 구성된다.
- 역할·향상 목표·메커니즘·전이 구조는 모델이 발명한다. 신규 구성요소를 만들면 `newComponents`의 실제 실행 연결 증거를 갖춘 후 `architectureFrontier`에도 편입될 수 있다.
- 노드는 새 역할뿐 아니라 `rsi_program_graph.rs`, `rsi_meta_controller.rs`, `rsi_factorial.rs` 등 개선 방법 자체의 소스를 수정하도록 선택할 수 있다. 새로운 모델 저술 설계를 독립 채택한 다음 후손은 기존 그래프 전체를 교체할 수 있다.
- 후손은 현재 소스의 실제 해시를 다시 확인하고, 자신이 직접 컴파일한 `rsi_program_graph::synthesize`로 다음 노드의 출처·역할·목적을 선택한다. 수락된 세대의 동결 평가 결과 `improved/neutral/regressed/adopted`가 조건부 전이의 입력이 된다.
- 수락된 이전 노드에서 출발하는 다음 분기를 선택하므로 더 긴 그래프·분기·재귀 동작이 가능하다. 노드 이름·연결은 미리 정해진 고정 단계 집합에 속할 필요가 없다.

## 실행 연결 및 독립 채택 증거

1. `rsi_system::propose`: `openProgram`을 구조화·검증하고 모델 저술 설계 원문에 포함한다.
2. `rsi_evolve::plan_generation_from_file`: 새로운 후손의 **재컴파일된 실제 코드**에서 인정된 부모 설계·현재 전체 소스 `architectureFrontier`를 읽고 프로그램과 노드를 선택한다. 새로운 독립 채택형 열린 프로그램이 없으면 기존 A→B→A complement/return 단계를 유지한다. 최근에 실제 채택된 모델 저술 프로그램 그래프가 있으면 고정 단계보다 최신 개선 원리를 우선할 수 있다.
3. `rsi_meta_controller::steer_open_program`: 동적 그래프 노드를 메타 개선기의 실행 목표·정책 해시·예측에 결합한다. 기존 고정 reflexive 경로는 후순위로 남는다.
4. `rsi_evolve`의 코디네이터와 worker: 그래프 원문, SHA-256, 노드, 실제 모델 수정 소스 및 부모 채택 커밋, 메타 목표가 동일한지 검증한다. worker가 실제로 선택된 그래프의 목표 파일을 수정할 수 있도록 집중 대상을 교체한다.
5. `generation.openImprovementProgram`, `compiledPlanExecuted.openImprovementRevision`, `metaPolicy.openImprovementRevision`: 계획·실제 재컴파일 후손 실행·그 결과를 각기 분리 기록한다.
6. `rsi_program_graph::handoff`: 이전 메타 프로그램과 다른 새 프로그램의 소스가 **독립 채택되었고**, 그 새 프로그램으로 재컴파일된 **다음 후손의 실제 소스 패치**까지 독립 채택된 경우에만 `executedProgramRewrites`로 집계한다. 단순 모델 제안, 미완료 도구 호출, 바뀐 계획 메타데이터는 성과로 보지 않는다.
7. `rsi_system::handoff_evidence`가 실제 실행된 프로그램 계보와 관찰 결과를 상위 자기모형에서 다시 사용한다. 다음 세대는 이 기록을 증거로 새 메커니즘·그래프를 저술한다.

## 반증 테스트와 기존 코드 수정

- 열린 그래프의 독립 채택, 임의로 새로 만든 역할로의 전이, 다른 평가 결과에 따른 분기.
- 신규 메타 프로그램을 작성한 세대와 실제 실행한 세대가 분리되어 있으며, 후손 실행 SHA가 일치하지 않으면 메타 프로그램 재설계가 **실행 성과로 집계되지 않는** 계약.
- 독립 채택이 없는 부모 설계는 새 그래프를 다음 세대로 전달할 수 없음.
- 종전 `rsi_meta_controller::steer_reflexive`가 `redesign_experiment`와 `execute_revised_experiment`을 배제하던 결함 수정.

코드 내부의 외부 동결 평가기, CI 자격, production 반영 경계는 변경하지 않는다.

## 외부 원리 비교

- HyperAgents (Meta AI, 2026): https://ai.meta.com/research/publications/hyperagents/ — task/meta 개선기를 동일한 수정 가능한 프로그램으로 통합
- GEPA (ICLR 2026)와 공개 구현: https://github.com/gepa-ai/gepa — 실행 trace의 풍부한 피드백과 후보 방법의 merge/mutation, Pareto 검색
- Dream-RSI (2026): https://arxiv.org/abs/2609.14858 — 실제 누적 탐색 궤적을 재사용해 상위 탐색 전략을 고치는 방식
- RSI-Env (2026): https://arxiv.org/abs/2611.01736 — 다음 독립적으로 채택된 후손이 연구·모델·하네스의 개선 상태를 계속 이어받는 방식
- Meta^n (2026): https://arxiv.org/abs/2608.24735 — 입력 경험의 확장에 따라 고정된 레벨을 넘어 재귀적 계층이 나타날 수 있다는 통찰

서브3의 차별점은 **모델 저술형 그래프의 조건부 실행, 검증된 실제 소스/커밋 계승, 후속 프로그램의 자기 재작성과 실행 증거**를 한 경로로 엮는 것이다. 구현한 프로그램은 가변적이지만 상위 source validator와 독립 평가기를 통과해야 한다.

## 테스트·CI·심판 상태

기능 코드와 반증용 Rust 단위 테스트를 추가했다. 컴파일, 단위 테스트 실제 실행, 장기 RSI 완주 및 CI는 수행하지 않았다. 이전 채점 **71점**에서 얼마나 상승했는지는 재채점 후에 알 수 있다. 심판 기준 80점 이상은 CI 자격이고, 실행한 적 없는 성공은 주장하지 않는다.
