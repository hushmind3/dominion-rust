## 2026-11-13 — 후손 실행·채택·독립 증명의 실제 분해 및 CI

다음은 **현재 커밋의 실제 호출 관계**다. 과거의 679줄 캠페인 구조나 ‘CI 미실행’ 언급은 그 당시 기록이고, 이 개편에서는 GitHub Actions를 실행해 컴파일 오류를 확인·교정한다.

```text
src/api/rsi.rs::start
  → jobs::rsi_evolve → rsi/lineage/campaign.rs → rsi/execution/campaign.rs::run_loop
  → rsi/execution/generation_preparation.rs::prepare_generation_input
     → --rsi-plan-generation
     → rsi/planner/descendant.rs::plan_generation_from_file
     → rsi/meta/inputs.rs::prepare                 (AppState 조회)
     → rsi/meta/composition.rs::compose_pure      (순수 알고리즘)
     → rsi/meta/evolution.rs::compile             (타입 있는 진화 연산자)
     → rsi/meta/transition_policy.rs::evaluate   (실측 전이 조건)
     → rsi/meta/experiment_dispatch.rs::dispatch (실제 선택한 실험 범위)
     → rsi/evaluation/contract.rs::bind          (동결된 경계의 서명)
  → rsi/execution/generation_launch.rs::launch_generation_worker
     → --rsi-worker-job → model/provider → tools/source editor
  → rsi/execution/generation_selection.rs::evaluate_and_adopt_generation
     → rsi_worktree::compare_at → rsi/lineage/adoption.rs::adopt
  → rsi/learning/methods.rs::record_method_feedback
  → rsi/execution/generation_finalization.rs::finalize_generation
     → rsi/proof.rs::recursive_proof_report
     → rsi/proof/{mutual_cycles,open_handoff,principle_replay}.rs
     → rsi_causal_closure.rs::audit / rsi/proof/open_evolution.rs
```

**책임 이동:** `campaign.rs`는 679줄에서 약 255줄로, 초기 1,376줄을 기준으로 보면 약 81%가 감소했다. 호출만 쪼갠 것이 아니라 준비·워커 프로세스/예산·독립 후보 평가/선택·실험 채택이 분리돼 있고, 독립 원본 재생 코드는 `rsi/proof`에 있다. `rsi.rs::safe_path`는 분리한 검증·채택·감독 파일을 후손 변경으로부터 차단한다. 진화 가능한 `generation_preparation.rs`와 메타 계획기/선택기는 계속 일반 소스 최적화 대상이다.

**CI 807개 테스트 실패 대응(2026-11-13):** GitHub Actions의 실제 Rust 릴리스 실행에서 테스트 804개 통과·3개 실패를 확인했다. 후손 작업기가 API provider/model 없이 로컬 네이티브 실행만 하는 경우, 사전에 추정한 모델 프롬프트 토큰을 실제 사용량처럼 처리해 `token_budget`로 종료하던 경로를 `generation_fits_remaining_budget` 및 `worker.rs::fit_worker_prompt`에서 구분했다. API 모델이 지정된 작업의 기존 토큰 예산은 유지된다. 독립 실험 계약 검증도 느슨하게 하지 않고, 기존 후손 제어기 계약 테스트가 새 동결 계약과 소스 트리 해시를 실제로 구성하도록 수정했다. 수정본 통과 여부는 최종 CI 상태를 별도로 확인한다.

**CI 정책과 수정:** `.github/workflows/ci.yml`은 `push`로 실행되고 macOS/Linux/Windows에서 Rust release 테스트, Node 스크립트 검사, React 타입 검사와 빌드를 수행한다. 실제 CI 로그에서 이전 분해의 미수정 문서 주석·누락 함수 가시성·상대 경로·새 모듈의 `String` 비교·`Digest` 트레이트 누락을 발견해 수정했다. [브랜치별 최신 CI 결과](https://github.com/hushmind3/dominion-rust/actions/workflows/ci.yml?query=branch%3Adominion-sub3) 참조. CI 전체 통과 여부는 GitHub 작업의 최종 상태로만 표현한다.

연구 배경: [GEPA (ICLR 2026)](https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html)는 실패 실행 흔적을 다음 제안에 사용하고, [AIDE² (2026-09)](https://arxiv.org/abs/2609.26457)는 실제 채택된 에이전트 소스와 다음 후손의 독립 성과를 비교한다. DOMINION 구현은 고정 모델 가중치와 동결 평가를 유지하고, RSI 전체 완성이나 온라인 성능 우위는 실측 전까지 주장하지 않는다.

## 2026-11-13 18시대 구조 재판정 대응 — 재귀 실험 정책과 경계

구조 채점 결과: 서브4 49, 챔피온 48, 서브1 46, **서브3 45**. 이전 실행경로 점수 49와는 독립적인 **아키텍처** 평가이며, 추가 코드는 재채점 전이다.

- `src/engine/rsi/meta/transition_policy.rs`: 범용 실험 선택의 *측정 조건*을 타입 계약으로 구성한다. 후손이 승인된 진화 프로토콜의 전이에 `guard: { mode: "all" | "any", clauses: [{ metric, comparison, threshold }] }`를 작성할 수 있다. 임의 소스 코드나 평가 기준을 실행하지 않으며, 측정 누락은 0점/성공으로 간주하지 않는다. `when: "measured"`는 실제 paired 기록과 위 guard를 동시에 요구한다. 새 실험의 순서, 반복, 분기, 자기개선 대상은 정해진 4세대 순서가 아닌 모델 작성 DAG에 의해 선택된다.
- `src/engine/rsi/meta/inputs.rs`: `AppState`와 경험 저장소, 동결 프로필, 비교 가능한 외부 계보의 조회를 소유한다. `meta/composition.rs`의 `compose_pure`는 준비된 메타 입력, 과거 실측 계보, 소스 프런티어만 소비한다. 실험 전략이 수정되더라도 보존소·평가기의 쓰기 권한이 따라오지 않는다.
- `src/engine/rsi/meta/evolution.rs`: 메타 프로토콜 전이의 타입·해시 유효성, 임계값·복합 조건·측정 출처와 선택된 연산자를 **컴파일된 후손에서 결정**한다. 같은 계약을 supervisor와 실제 worker가 재대조하는 기존 경로는 유지된다. 수치 조건은 평가 프로필을 수정하는 것이 아니라 영수증에 있는 관찰을 *다음 실험의 조건*으로 활용한다. 같은 컴파일 커밋에서 실행된 **채택되지 않은 실제 측정 실험**의 결과도 마지막 실험 피드백으로 받아들여 다음 전이를 선택한다. 원본 프로토콜과 채택된 소스의 출처는 그대로 두고, `feedbackGenerationId`를 서명에 묶어 재생 검증한다. 실패한 시도는 계속 실패로 남는다.
- `src/engine/rsi_system.rs`: 위 2개 모듈을 개선 대상 소스 인벤토리에 넣어, 다음에 재컴파일된 후손이 **실험 정책을 해석하는 원리 자체의 Rust 코드를 개선**할 수 있도록 한다.

현재 실증과 코드 검증을 구분한다. 구조 변경은 GitHub에 반영했고, 실제 4세대 RSI 완성·실험 조건의 성능 우위·새 평가 점수는 주장하지 않는다. 테스트 작성·실행, 컴파일, 빌드, CI, 유료 모델 실행을 수행하지 않았다.

## 18시대 구조 재판정(서브3 45점) 이후 실제 설계 변경

### 개선 방법 자체를 개선하는 개방형 메타 아키텍처

- 기준 판정은 **서브3 45점 / 4위**. 이 변경 이후의 신규 채점은 받지 않았다. 4세대 성공 실적을 주장하지 않는다.
- `rsi/meta/evolution.rs`: **Serde typed** 연산자 `Operator`, 조건부 `Transition`, `Protocol`, 컴파일된 `Compiled` 버전 계약. 모델은 `rsi_system_plan.evolutionProtocol`로 반복·분기·자기수정 가능한 개선법·실험법을 설계한다. 고정 A→B→A→B 순서는 더 이상 유일한 성공 구조가 아니다.
- `rsi/meta/composition.rs`: 컴파일된 후손의 모든 실험 정책·경로·개선 원리·메타 제어·다중 구성요소 판단을 하나의 명시적 `MetaComposition` 인터페이스에서 생성한다. 기존 후손 계획기는 이 계약을 소비한다.
- `rsi/meta/policy_dispatch.rs`: 학습된 메타 연산자 → 실제 선택 정책 연결. `rsi/meta/experiment_dispatch.rs`: 연산자의 실험 종류 → 실질적인 도구 쓰기 범위와 대조 조건으로 컴파일. `paired_source`, `isolated_source`, `meta_policy_rewrite`, `method_control`, `factorial_source`(현재 단일 요인 격리), `cross_component`(공동 소스 관찰)가 서로 다른 실행 계약을 만든다. **단일 요인 격리를 완성된 A/B/A+B 요인 실험으로 주장하지 않는다.**
- `rsi/evaluation/contract.rs`: 변경 불가능한 평가·채택 경계. 원본 소스 해시, 독립 평가 프로필, 계획 정책, 메타 프로토콜, 실제 수정 범위의 SHA 서명을 담은 `FrozenExperiment` 계약을 감독자가 생성한다. 워커가 호출 전에 같은 계약을 재구성한다.
- 실제 `tools.rs`는 격리 실험에서 승인 범위 밖의 파일 쓰기·교체를 거부한다. `rsi/execution/campaign.rs`도 실제 후보 patch 경로가 범위를 벗어나면 독립 평가 성공이라도 채택 불가로 처리한다. 실행 범위 검증 결과는 세대 장부에 기록한다.
- `rsi/proof/open_evolution.rs`는 독립 후보 평가 영수증·소스 바이트·실행된 메타 연산자·후손 작업 품질·후속 프로토콜 개정을 다시 읽는다. 기존 `rsi_causal_closure.rs`의 고정 실험 증명은 보존하고, `rsi/proof.rs`에 독립적 개방형 증명 경로를 추가했다. 두 증명 경로 모두 성공을 추정하지 않는다.

### 실제 코드 분해와 소유 경계

```text
src/engine/rsi/
  meta/
    evolution.rs           // 새 개선 연산자와 전이: 모델 저작 가능
    composition.rs         // 실험·메타·학습 목표 결합
    policy_dispatch.rs     // 메타 정책 실제 반영
    experiment_dispatch.rs // 실험 종류별 실제 도구 행동 제약
    controller_evidence.rs // 기존 메타 정책의 실측 신호
    controller_selection.rs// 기존 메타 목표 선택과 탐색 알고리즘
    controller_policy.rs   // 예측·정책 재구성
    controller_feedback.rs // 독립 결과를 다음 세대로 계승
    reflexive_design.rs    // 기존 재귀 실험 계획
  planner/descendant.rs    // 명시적 MetaComposition을 소비
  execution/campaign.rs   // 격리 작업공간과 후보 운영
  evaluation/contract.rs  // 독립 동결 작업계약; 모델 편집 금지
  proof/
    open_evolution.rs      // 다양한 프로토콜의 실제 결과 감사
    reflexive_evidence.rs // 기존 재귀 실험 영수증 검증
    (기존 proof.rs 경로 유지)
```

기존 최상위 `rsi_meta_controller.rs`는 799줄 → 31줄 실행 인터페이스로, `rsi_reflexive.rs`는 ~660줄 → ~190줄 인터페이스/기존 테스트 포함으로 줄였다. 원본 평가기 `rsi.rs`, 신규 독립 `proof/`, `evaluation/contract.rs`는 개선 후보의 수정 대상에서 제외한다.

**아직 완료가 아닌 것:** 재컴파일 후손이 이 열린 프로토콜의 연속 2세대 독립 품질 개선을 실제로 달성한 기록 없음. 실험 방식 종류 중 `factorial_source`는 하나의 요인 격리까지만 연결했으며 3개 대조 변형을 모두 자동 실행하지 않는다. 장기적인 평가자 자체의 업그레이드는 별도 승인·평가 버전 승격 계약이 필요하다. **테스트 작성·실행, 컴파일, 빌드, CI, 모델 호출 없음.**

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
- 이번 추가 개편의 테스트 파일 수정과 신규 테스트 파일은 사용자 지시대로 되돌렸다. 검증 대상은 RSI 실행 소스이며, 테스트·컴파일·빌드는 수행하지 않았다.

## 재판정 이후 추가 보강: 검증기 불변성과 품질 대조

- `src/engine/rsi/planner/domain_selection.rs`는 독립 모듈로 분리된 **실제 변경 가능한** 후손의 탐색 정책·비용 인지 도메인 선택·native 선택 실험 본체다. `rsi_evolve.rs`는 호출용 facade로 축소되었고, `rsi.rs`, `rsi_evolve.rs`, `rsi_system.rs`는 후보 편집에서 제외된다. 진화할 수 있는 알고리즘은 `rsi_system.rs::COMPONENTS`와 `rsi_worktree.rs::classify`에도 등록했다.
- 검증 경계에 `src/engine/rsi_worktree.rs`(독립 페어 측정), `src/engine/rsi_metrics.rs`(측정 기준), `src/engine/rsi_archive.rs`(원본 보관), `src/api/rsi.rs`(권한·시작 옵션) 및 고정 `tests/support/engine/rsi_*`, `tests/support/api/rsi*`, `tests/unit/engine_rsi_*`를 추가했다. 후보가 자기 시험지를 고쳐 성공을 만드는 일을 차단한다.
- 원본 `compare_at` 결과는 후보 전체 evaluation 객체가 아니라 ID/해시/지표를 포함한 요약이다. ON/OFF 재검산에서 후보 원본 평가 `id`, `metrics`, `evidenceSha256`, `source`를 이 요약과 정확히 비교하도록 수정했다. 컨트롤과 실험군의 입력 SHA는 작업기 실행 전 고정된 별도 job 메타필드로 저장한다.
- `rsi_causal_closure.rs`는 frozen 홀드아웃의 실패→통과뿐 아니라 이미 통과한 사례의 `quality`/`discoveryScore` 독립 지표 향상도 확인한다. `method_ablation.rs::paired_quality_delta`는 OFF/ON 동일 평가 케이스의 품질만 대조하며 **응답 속도/토큰 감소는 지능 향상으로 계산하지 않는다.** 남은 실험 시간은 캠페인 단조시계로 계산한다.

### B→A2 방법 재설계의 관측 가능한 근거

1. B 후손에서 모델이 도구로 실제 사용한 절차 ID·revision과 독립 paired evaluation, 동일 소스에서 절차만 끈 대조군을 기록한다.
2. `rsi_system.rs::blueprint`와 `rsi_system/lineage.rs::lineage_capsule`은 평가·절차를 별도 provenance로 보존하고, A2 컴파일 후손은 기존 바이너리의 기억이 아닌 이 저장 기록을 다시 로딩한다.
3. `rsi_system/guidance.rs::improvement_directive`는 B에 실제 향상이 있을 때 `evolve_executed_method_from_feedback`을 선택하고, B의 control/treatment 평가 ID와 실제 홀드아웃 성과를 방법 피드백에 넣는다. 후손 지시는 같은 이름·분야의 새로운 실행 DAG 수정과 후속 실행을 명시한다.
4. `rsi_causal_closure.rs::revised_method_after_feedback`는 A2가 B의 원본 피드백에 연결된 제안인지 검증하고, B2에서 그 새 절차가 실제 채택된 소스를 작성했는지 원본 operation receipt와 독립 ON/OFF 실험으로 재검산한다.
5. 동결 개별 비교에서 단지 빠르고 저렴한 실행은 지능 이득이 아니다. 기존 테스트를 통과하고 답변의 `quality`/`discoveryScore`가 높아진 사례도 원본 지표·동일 기준과 독립 평가 출처가 확인될 때만 품질 향상에 포함한다.

## 최신 49점 판정 이후: 컴파일된 후손의 방법 선택과 재시도 폐회로

2026-11-13 독립 심사: 서브3 **49점 / 1위**, RSI 전체 완성 **0개**. 아래 변경은 판정 이후의 추가 소스 구현이며 새 점수나 실제 4세대 실행 성공을 의미하지 않는다.

실제 연결 흐름:

```text
POST /api/rsi/start (continuous system evolution, methodAblation 기본 on·옵션 off)
  -> campaign.rs::run_loop
  -> built descendant --rsi-plan-generation
     -> rsi_system/lineage.rs::method_transfer_target
        -> last independently adopted META candidate + original proposal session
        -> selected exact executable procedure + frozen target-domain holdout
        -> canonical actual successor task objective from method applicability
     -> rsi/planner/descendant.rs
        -> meta A -> capability B (selected adopted method)
        -> measured B method ON/OFF -> meta A2 (feedback-driven method revision)
        -> accepted A2 -> capability B2 (revised method)
  -> compiled_planning.rs: independently replay transfer and rebind temporary
     resident-supervisor cost hint, preserving original in audit
  -> method_ablation.rs: execute detached method-OFF worker (selected method ID)
  -> worker.rs: replay exact method transfer, select only chosen revision
  -> method_ablation.rs: method-ON worker, independently paired frozen cases
  -> generation_evidence.rs: actual method ID/source/worker receipt
  -> methods.rs: actual procedure operations and adopted source-byte provenance
  -> generation_finalization.rs: persist the complete result
  -> rsi_causal_closure.rs: 4 successive *adopted revisions*, ON/OFF quality,
     method A vs revised method A2, compiled action hashes, real source writes
  -> proof.rs: one unified API/termination/success verdict
```

**실패 회복:** 채택되지 않은 중간 세대는 성공으로 세지 않으며, 마지막 실제 채택 커밋을 유지한다. 이전 바이너리와의 동일 소스 계획 비교(`counterfactual.rs`) 및 폐회로(`rsi_causal_closure.rs`)는 실패 시도가 끼어도 채택된 후손의 정확한 커밋 인계를 확인한다. `guidance.rs`는 B의 방법 ON/OFF 영수증을 A2 재시도 동안 보존하고, 최종 성공 판정은 여전히 새 모델 호출 없이 독립 영수증을 재조회한다.

**실제 행동 변화:** 이전·후손 컴파일 계획의 동작 해시에 상속 방법 ID·revision·실제 작업 목표를 넣었다. 같은 파일을 수정해도 새 방법을 실제 실행하고 독립 채택 바이트에 기여했다면 방법 개선 행동으로 인정한다. 단순 파일 이동·메서드 이름 변경만으로는 재귀 증거가 되지 않는다.

**예산:** RSI 캠페인을 명시적으로 시작하면 ON/OFF 비교를 기본 활성화하되 UI에서 끌 수 있다. 메타 세대에서 실제로 채택된 방법 전이 대상이 없는 동안 대조군 추가 모델 호출은 발생하지 않는다. 실험 토큰은 기존 캠페인 총량에 합산된다.

**남은 실증:** 모델 샘플링 변동성은 단일 ON/OFF 실행에서 분리되지 않는다(`causalAblationProven:false`). 실모델 4세대 폐회로·반복 시드 비교·일반화 점수는 아직 측정하지 않았다. 테스트·컴파일·빌드·CI·유료 모델 실행은 수행하지 않았다.

## 새 실행 구조와 방법 대조실험의 실제 연결

```text
web/src/pages/SelfImprovement.tsx (methodAblation 별도 동의)
  -> src/api/rsi.rs::start (rsiMethodAblation)
  -> src/engine/rsi_evolve.rs::{campaign_execution,compiled_planning,generation_evidence,method_ablation,generation_finalization}
  -> campaign.rs::run_loop
       -> compiled_planning.rs::plan_compiled_descendant (실제 재컴파일된 후손 정책)
       -> method_ablation.rs::run_method_control [ON/OFF 동의 + 해당 분야에 상속 절차가 있는 경우]
            -> rsi_generation control job -> --rsi-worker-job
            -> methods.rs::method_strategies (OFF 분기의 실제 절차 사용 거부)
            -> rsi_worktree.rs::compare_at (동결 평가, 대조군 자동 채택 금지)
            -> rsi_method_ablation 영구 저장 (서버 원본 사전입력 SHA + 후보 평가)
       -> rsi_generation treatment job -> --rsi-worker-job (ON)
       -> generation_evidence.rs::capture_generation_evidence (도구/계보/사용량)
       -> rsi_worktree.rs::compare_at -> campaign.rs::adopt (독립 채택만)
       -> methods.rs::record_method_feedback (실제 방법 실행 기록·채택 파일 기여)
       -> method_ablation.rs::measured_method_ablation (대조군 vs ON 동일 홀드아웃)
       -> generation_feedback.rs::record_generation_feedback (새 탐색식 시도·메타 피드백)
       -> generation_finalization.rs::finalize_generation (영구 저장·공통 증명·다음 정책)
  -> rsi/proof.rs::recursive_proof_report
       -> rsi_system/handoff.rs::handoff_evidence
       -> rsi_causal_closure.rs::audit
            -> A→B→A→B + ON/OFF 독립 실측 + 새 방법 버전의 후손 실행
  -> GET /api/rsi/campaigns/:id/verify 와 캠페인 목록/상세의 동일 결과
```

- **대조실험 자원:** 추가 모델 호출은 `methodAblation` 별도 선택이 있을 때만 실행합니다. 추가 실행은 세대 작업으로 기록하고 `campaign_model_tokens`에 포함해 treatment 시작 전 남은 토큰을 재검산합니다.
- **대조군 영수증:** control job의 `rsiMethodAblationInputSha256`와 treatment job의 동명 필드는 worker가 도중에 입력 정책을 갱신해도 사전실행 동일성 검사를 유지합니다. `rsi_method_ablation` 레코드의 후보 ID/평가와 실제 작업 세션의 procedure 로그를 재조회하며, 구두 주장이나 제안서는 성과로 인정하지 않습니다.
- **품질 차이:** `measured_method_ablation`은 동일 독립 평가 사례의 `baselinePass`를 대조한 다음, OFF 실패·ON 통과인 실제 홀드아웃이 1개 이상이고 역전 회귀가 없을 때만 측정상 이득을 기록합니다. 대조군 작업은 이후 모델 학습 데이터가 아닌 실행 증거로만 활용됩니다.
- **증명 강화:** `rsi_causal_closure.rs::audit`는 이미 채택된 4세대 계보의 실제 방법 사용 2번 외에, 각 기능 세대에서 **저장된 ON/OFF 평가를 다시 재생한 결과**가 함께 일치할 것을 요구합니다. 대조군 옵션을 켜지 않은 캠페인은 이 엄격한 전체 RSI 완성 기준을 만족할 수 없습니다.
- **가중치 고정:** 기반 모델은 동일 API/로컬 inference 경로를 재사용하며 LoRA/파인튜닝/가중치 업데이트는 RSI 실행에 연결하지 않았습니다.

## 남은 핵심 과제

- 이제 같은 소스/모델/고정 평가에서 실제 방법 OFF/ON 두 작업을 실행하는 경로가 연결돼 있다. 다음 단계는 **모델 난수·프롬프트/시간 차이와 공유 상태 효과를 통제한 여러 시드의 반복 실험**, 절차 단독의 인과 기여량 추정, 다른 과제군 일반화 측정이다. 현재의 `causalAblationProven`은 의도적으로 false를 유지한다.
- 커밋·아카이브·실행기는 독립 검증과 작업공간 격리를 전제로 한다. OS 수준의 네트워크·프로세스 예산 강제는 현재 소스 검토에서 별도 검증이 필요하다.
- 고정 평가로 채택된 후손이 일반 목적 기능과 그 개선법을 상호 발전시켰는지 실제 유료 모델 캠페인으로 측정해야 한다.

## 2026년 연구 참고

- Zhang et al., *Hyperagents* (2026-03): 실제 자기수정 절차를 함께 진화시켜 메타 개선을 확인하는 설계. https://arxiv.org/abs/2603.19461
- Liu et al., *Mendel Gödel Machine* (2026-08): 다른 계보의 성과를 출처와 함께 비교하는 진화 방식. https://arxiv.org/abs/2608.07645
- Xie et al., *Predict Before You Modify* (2026-09): 예상 개선과 사후 측정 간 피드백을 다시 다음 개선 정책의 입력으로 만드는 방법. https://arxiv.org/abs/2610.13991
- Huang et al., *How Agent Skills Evolve* (2026-11): 실행 계약·중복 관리·객관적 회귀 검증의 필요성을 다룬 대규모 GitHub 분석. https://arxiv.org/abs/2611.04456

- Srikanth et al., *Recursive self-improvement of AI research agents* (2026-09): 후손이 개선기가 되는 연속 버전과 숨겨진 평가의 경계. https://arxiv.org/abs/2609.26457
- Xue et al., *PAST-Bench* (2026-08): 같은 조건에서 과거 경험을 켜고 끄는 경로별 효과 분리. https://arxiv.org/abs/2608.04003

- Google Research et al., *RRSI* (2026-09-21): 편집 후보 수 제한·다른 분야의 홀드아웃 전이와 탐색 과정의 과적합 완화. https://arxiv.org/abs/2609.24972
- Zhang et al., *Flex-RSI* (2026-11-06): 동일 초기 하네스와 자원 제약을 유지한 실제 개선기 개입 실험을 분리해 측정. https://arxiv.org/abs/2611.08946
