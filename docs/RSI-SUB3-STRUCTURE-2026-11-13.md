# 서브3: 재귀 시스템 자기개선의 책임 경계 (2026-11-13)

대상: `dominion-sub3`. 근거: 현재 브랜치 소스의 실제 호출과 필드 흐름. 빌드·테스트·실모델 캠페인을 수행하지 않았다.

## 실행 경로와 경계

1. API에서 시작한 RSI 작업은 `src/engine/rsi_evolve.rs`의 진입점과 `src/engine/rsi/execution/campaign.rs::run_loop`를 거쳐 작업·후보 상태를 기록한다.
2. 감독자는 worktree의 채택 소스를 재빌드하고 SHA 및 `--rsi-worker-identity`를 확인한 후 `--rsi-plan-generation`으로 **그 버전의** 개선 계획을 요청한다.
3. `src/engine/rsi_system.rs`는 변경 가능한 후손의 소스 모델을 제공한다. 세부 역할은 `rsi_system/lineage.rs`(다른 세대·캠페인의 증거), `design.rs`(설계 역할·완결성), `guidance.rs`(개선 지시), `proposal.rs`(실제 소스 수정 대상 선언), `handoff.rs`(후손 계승 검증)로 분리됐다.
4. 감독자가 계획·인덱스·동결 평가 근거를 검증하면 `src/engine/rsi/execution/worker.rs::worker`가 개별 세대를 실행하고 실제 도구·소스 편집 영수증을 남긴다. 독립 페어 평가와 원본 후보 검증을 통과한 커밋만 채택된다.
5. `src/engine/rsi/execution/counterfactual.rs::matched_planner_intervention`은 이전·신규 컴파일러에 같은 소스와 히스토리를 넣어 **계획 행동 차이**를 기록한다. 이것만으로 후손 품질의 원인을 확정하지 않는다.
6. `rsi_system/handoff.rs::handoff_evidence`는 기계적으로 확인한 영수증·방법 계승·하위 과제 성과를 집계한다. `src/engine/rsi_causal_closure.rs::audit`의 4세대 메타→기능→메타→기능 검사를 거쳐야 `demonstrated`가 참이다.

## 변경으로 강화한 경계

`rsi_system/peer_verifier.rs::validate_peer_references`는 기존의 후보 ID·채택 커밋·독립 평가 영수증 검증에 더해 다른 계보에서 가져온 **역할 구성·가설·설계 요약·성과 수치·출처 파일 집합**을 실제 저장된 채택 설계/후보와 대조한다. 이 정보는 후손이 읽는 모델 문맥에 들어가므로 ID만 맞고 설명이 변조된 참조를 허용하지 않는다.

현재 계보 참조의 선택은 `architecture_frontier` 등의 소스 구조를 이용하되, 다른 캠페인의 패치를 직접 채택하지 않는다. 이전 설계는 근거가 있는 참고 정보이며 새 소스 패치의 품질은 별도로 검증한다.

## 재귀 실행기의 실제 소스 범위

모듈 분리로 신규 생성된 `rsi_system/*.rs`도 `rsi_system.rs::COMPONENTS`의 개선 방법 목록, `source_role`의 `improvement_methods` 분류, `rsi_causal_closure.rs::meta_source`의 메타 변경 판정에 연결했다. 즉 새 코드가 **소스 목록 → 후손의 개선 표적 → 채택 후 메타 개선 감사**의 세 경로에서 누락되지 않는다.

`rsi_causal_closure.rs::independently_adopted`는 세대에 복사된 평가 ID·메트릭·증거 SHA가 후보의 독립 A/B 원본과 일치하는지도 검증한다. `handoff.rs`는 방법 계승 검사 중 경험 레코드를 매번 다시 읽지 않고 한 번의 동일 시점 스냅샷을 이용한다.

## 판정 통일: 진입점에서 자동 종료까지의 실제 경로

- 등록: `src/api/rsi.rs`의 RSI 실행 요청은 `rsiSystemEvolution`과 작업 범위를 고정해 durable job을 생성한다.
- 실행: `src/engine/rsi/execution/campaign.rs::run_loop`는 후보를 worktree에서 고정 페어 평가한 후 채택한다. 각 세대의 실제 편집 파일, 후보·평가 ID, 유료 모델 호출, 빌드 및 감독 계획 기록을 보존한다.
- 후손: 채택 커밋의 재컴파일된 `--rsi-plan-generation`과 `--rsi-worker-job`가 다음 목표·도구·프롬프트·소스 변경을 결정한다. `matched_planner_intervention`은 모델 없이 이전/후손 계획 차이를 비교하되, 이 비교 단독으로 성과를 부여하지 않는다.
- 독립 감사: `src/engine/rsi_causal_closure.rs::audit`는 4개의 연속 메타 A→능력 B→메타 A2→능력 B2를 검사한다. 채택된 소스의 SHA·원본 frozen pair 평가·기존 작업의 실제 컴파일 후손 실행 영수증과, A가 제안한 실행 절차를 B가 실제 소스 쓰기에 사용했는지, A2가 B의 피드백을 읽고 새 절차 버전을 냈는지, B2에서 그 새 버전이 사용됐는지를 확인한다. 모든 단계는 독립 홀드아웃에서 실제 실패→성공 사례가 있어야 한다.
- 단일 성공 결정: `src/engine/rsi/proof.rs::recursive_proof_report` → `rsi_system/handoff.rs::handoff_evidence` → `rsi_causal_closure.rs::audit`. `rsi::attach_recursive_proof`가 캠페인·API 표시 필드를 동일하게 채우며, `campaign.rs::proof_goal_reached`, `src/api/rsi.rs::verify`, `campaign_view`, `list`, `campaigns`가 동일 판정을 사용한다. `generationCapabilityProven`은 채택 기반 일반 능력 개선 참고 값일 뿐 전체 RSI 완성이 아니다.
- 메모리: `rsi_system/lineage.rs::lineage_capsule`은 모델 제안 절차 ID·revision과 실제 후손의 실행·성과 피드백을 제한된 계보에 담는다. 재컴파일된 후손의 `worker.rs`가 이를 현재 계획 문맥으로 재주입하고 독립 원본 검증을 거친다.

### 심판 규칙과 공개 상태

2026-11-13 제공된 판정은 45점·4위, 완성 0개였다. 이후 구조·검증 통합을 수정했으나 재채점·실컴파일·실모델 완료를 주장하지 않는다. 고정 모델의 파라미터 학습, LoRA, 파인튜닝은 이번 RSI 경로에 연결하지 않았다. 교차 세대의 사용·피드백은 기술적으로 추적하지만 개선 절차를 끈 상태의 동일 조건 A/B **인과 제거실험**은 별도로 필요하다.

## 고정 심판과 변경 가능한 후손의 경계

- 수정 가능한 실제 지능: `rsi_system/lineage.rs`(과거 계보와 실제 방법 실행 기억), `rsi_system/guidance.rs`(후손 지시), `rsi/{planner,execution,learning}`(세대별 탐색·실행·개선법), `rsi_meta_controller.rs`(선택 메타정책).
- 후보가 수정할 수 없는 독립 감독: `rsi/proof.rs`(공통 완성 판정), `rsi_causal_closure.rs`(A→B→A→B 실제 방법 계보 감사), `rsi/execution/counterfactual.rs`(이전·후손 재생 비교), `rsi_system/design.rs`(채택 설계 완결성), `rsi_system/proposal.rs`(설계 선언 유효성), `rsi_system/handoff.rs`(고정 계승 증거), `rsi_system/peer_verifier.rs`(다른 캠페인 원본 증명). 이 경계는 `rsi.rs::safe_path`의 실질적 편집 차단에 반영돼 있다.
- `rsi_system.rs`는 변경 가능한 설계 진입점과 모듈 등록을 보존하되 엄격한 증명 함수는 별도 보호 모듈로 위임한다. `rsi_system.rs::COMPONENTS`는 이 보호 모듈을 후손의 편집 가능한 개선법 후보로 소개하지 않는다.
- 새 경계에 대한 회귀 검사는 `tests/support/engine/rsi_system.tests.rs`, `rsi_causal_closure.tests.rs`, `tests/support/api/rsi.tests.rs`에 선언했으며 실행하지 않았다.

## 남은 핵심 과제

- 동일 과제·모델·예산에서 개선 절차를 실행한 조건과 끈 조건을 비교하는 **개선법별 인과 제거실험**이 필요하다. 현재 경로는 검증된 소스 쓰기·계보·홀드아웃 성과의 결합 증거이며, 개별 절차만의 기여량과 장기 일반화는 별도 측정 대상이다.
- 커밋·아카이브·실행기는 독립 검증과 작업공간 격리를 전제로 한다. OS 수준의 네트워크·프로세스 예산 강제는 현재 소스 검토에서 별도 검증이 필요하다.
- 고정 평가로 채택된 후손이 일반 목적 기능과 그 개선법을 상호 발전시켰는지 실제 유료 모델 캠페인으로 측정해야 한다.

## 2026년 연구 참고

- Zhang et al., *Hyperagents* (2026-03): 실제 자기수정 절차를 함께 진화시켜 메타 개선을 확인하는 설계. https://arxiv.org/abs/2603.19461
- Liu et al., *Mendel Gödel Machine* (2026-08): 다른 계보의 성과를 출처와 함께 비교하는 진화 방식. https://arxiv.org/abs/2608.07645
- Xie et al., *Predict Before You Modify* (2026-09): 예상 개선과 사후 측정 간 피드백을 다시 다음 개선 정책의 입력으로 만드는 방법. https://arxiv.org/abs/2610.13991
- Huang et al., *How Agent Skills Evolve* (2026-11): 실행 계약·중복 관리·객관적 회귀 검증의 필요성을 다룬 대규모 GitHub 분석. https://arxiv.org/abs/2611.04456

- Srikanth et al., *Recursive self-improvement of AI research agents* (2026-09): 후손이 개선기가 되는 연속 버전과 숨겨진 평가의 경계. https://arxiv.org/abs/2609.26457
- Xue et al., *PAST-Bench* (2026-08): 같은 조건에서 과거 경험을 켜고 끄는 경로별 효과 분리. https://arxiv.org/abs/2608.04003
