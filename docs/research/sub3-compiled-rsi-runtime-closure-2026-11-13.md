# 서브3 — 실행 경로 검증으로 79점 구조 상한 돌파 준비 (2026-11-13)

## 출발점

심판의 최신 재판정은 챔피온·서브3·서브4 모두 79점이다. 서브3의 남은 80점 진입 조건은 모델 작성 개선 방법이 **원본 브랜치 검사 → 실제 격리된 후손 생성 → 독립 A/B 측정 → 채택 → 새 바이너리 후손 재계획 → 재귀 계승**으로 이어지는 실제 실행 증거다. 정적 구조의 존재나 단순 소스 수정은 개선 성능 증거로 인정하지 않는다.

## 실제 GitHub Actions 오류에서 확인한 결함과 수정

- [기존 CI 43206692311](https://github.com/hushmind3/dominion-rust/actions/runs/43206692311): Ubuntu 742개 중 741개 통과, `rsi_system::integration_contract_tests::multi_component_design_requires_complete_source_mapping_and_complete_patch` 1개 실패. 테스트 fixture가 필요한 `candidateId`를 빠뜨렸는데 이미 도입된 채택 출처 검증은 이를 의도적으로 거부했다. 후보 출처 필드를 채우고 누락·소스 해시 손상 반례를 추가했다. **실제 출처 검증은 완화하지 않았다.**
- [실모델 실행 43317563169](https://github.com/hushmind3/dominion-rust/actions/runs/43317563169): 사전 소스 준비 단계 이후 `cargo build --release`에서 `rsi_evolve.rs` 대형 JSON 매크로의 재귀 한도 초과가 드러났다. `generation_details`의 45개 증거 필드를 보존한 채 4개 소형 객체로 분리했다.
- [실모델 실행 43318027367](https://github.com/hushmind3/dominion-rust/actions/runs/43318027367): 다음 `rsi_system.rs` 최종 인계 증거의 42개 JSON 필드에서 매크로 재귀 한도 초과가 드러났다. 동일 스키마의 4개 객체 결합으로 변경했다.
- [CI 43318549691](https://github.com/hushmind3/dominion-rust/actions/runs/43318549691): 매크로 장애 뒤에 숨겨진 3개 Rust 오류(E0502: 세대 기록 대여/수정 순서, E0277: `Value`/참조 비교, E0382: 학습 원리 문자열 소유권 이동)가 검출됐다. 세대 증거 기록 순서 조정, 비교의 참조 일치, 메커니즘 키 복제로 수정했다. **채택·보상 기준은 변경하지 않았다.**

## 실모델 작업공간 분리 — 장벽이었던 경로

기존 `.github/workflows/sub3-live-model-cycle.yml`은 detached worktree를 만들어 모델 캠페인에 입력했다. 강화된 `rsi_work_branch`가 정확한 `dominion-sub3`만 허용하기 때문에 실제 모델이 도달하기 전에 소스 브랜치 검사가 실패할 수 있었다.

- `scripts/sub3-prepare-owned-source.sh`는 깨끗하고 커밋이 고정된 원본 저장소를 **서로 다른 .git 데이터베이스의 별도 클론**으로 만들고 `dominion-sub3`만 체크아웃한다.
- 원본이 `dominion-sub3`로 체크아웃됐거나 `origin/dominion-sub3`가 고정 커밋을 가리키는 경우만 허용한다. `main`을 임의로 서브3이라 이름만 바꾸는 사례, 다른 SHA, 미저장 소스, 이미 있는 경로를 거부한다.
- 별도 클론의 `origin`은 공식 저장소 주소를 명시적으로 기록하되 네트워크 작업이나 원격 ref 이동은 하지 않는다. 실험 루프는 비공개 분리된 소스에서 계속되고 실제 원본 채택은 기존 심사·브랜치 경계로 제한된다.
- `scripts/sub3-prepare-owned-source.test.mjs`: Git 초기화와 실제 체크아웃을 통해 안전한 클론, 변경된 SHA/더티 소스 차단, main 차단, 원본이 detached여도 정확한 원격 브랜치 팁으로 재현 가능한 경로 등 **4개 Node/Git 테스트를 로컬에서 통과**했다.
- 실모델 워크플로는 무료 모델이 불가해도 위 소스 준비 테스트를 먼저 실행하도록 분리했다. 모델을 실제 호출하지 않은 상태를 성공으로 위장하지 않는다.

## 다음 실행 검증의 판정 항목

1. Linux/macOS에서 최신 Rust 바이너리와 테스트가 실제 빌드되는지 확인.
2. 기존 `accepted_source_is_rebuilt_and_really_executes_the_next_generation` 및 `goal_capability_loop_inherits_research_adopts_and_applies_next_decision`가 변경된 소유 브랜치 경계에서도 통과하는지 확인.
3. 실모델로 생성된 변경이 실제 소스 파일을 바꾸고 독립 평가로 채택된 뒤 새 **컴파일 후손**이 다음 개선을 수행하는지 세대별 증거를 기록.
4. 모델 호출 감사·후손 영수증·채택된 소스 해시가 같은 캠페인 및 재시작 후 계보에서도 이어지는지 검증.

테스트·CI 성공 여부는 **실제 완료된 실행 기록만** 근거로 한다. 이번 변경을 실모델의 반복적인 지능 향상 실증 결과로 해석하지 않는다.
