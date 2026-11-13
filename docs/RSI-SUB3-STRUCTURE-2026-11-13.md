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

`rsi_system/lineage.rs::validate_peer_references`는 기존의 후보 ID·채택 커밋·독립 평가 영수증 검증에 더해 다른 계보에서 가져온 **역할 구성·가설·설계 요약·성과 수치·출처 파일 집합**을 실제 저장된 채택 설계/후보와 대조한다. 이 정보는 후손이 읽는 모델 문맥에 들어가므로 ID만 맞고 설명이 변조된 참조를 허용하지 않는다.

현재 계보 참조의 선택은 `architecture_frontier` 등의 소스 구조를 이용하되, 다른 캠페인의 패치를 직접 채택하지 않는다. 이전 설계는 근거가 있는 참고 정보이며 새 소스 패치의 품질은 별도로 검증한다.

## 남은 핵심 과제

- 후손 행동 차이와 실제 A/B 개선 사이의 **세대 간 개입 인과성**을 여러 환경·과제로 실증해야 한다. 행동 해시만으로는 충분하지 않다.
- 커밋·아카이브·실행기는 독립 검증과 작업공간 격리를 전제로 한다. OS 수준의 네트워크·프로세스 예산 강제는 현재 소스 검토에서 별도 검증이 필요하다.
- 고정 평가로 채택된 후손이 일반 목적 기능과 그 개선법을 상호 발전시켰는지 실제 유료 모델 캠페인으로 측정해야 한다.

## 2026년 연구 참고

- Zhang et al., *Hyperagents* (2026-03): 실제 자기수정 절차를 함께 진화시켜 메타 개선을 확인하는 설계. https://arxiv.org/abs/2603.19461
- Liu et al., *Mendel Gödel Machine* (2026-08): 다른 계보의 성과를 출처와 함께 비교하는 진화 방식. https://arxiv.org/abs/2608.07645
- Xie et al., *Predict Before You Modify* (2026-09): 예상 개선과 사후 측정 간 피드백을 다시 다음 개선 정책의 입력으로 만드는 방법. https://arxiv.org/abs/2610.13991
- Huang et al., *How Agent Skills Evolve* (2026-11): 실행 계약·중복 관리·객관적 회귀 검증의 필요성을 다룬 대규모 GitHub 분석. https://arxiv.org/abs/2611.04456
