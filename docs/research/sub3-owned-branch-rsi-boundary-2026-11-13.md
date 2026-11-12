# 서브3 — 자기개선 원본 브랜치 소유권 경계 강화 (2026-11-13)

## 계기

2026-11-13 최신 심판 판정은 `dominion-sub3`를 77점(3위)으로 평가했고, `src/engine/rsi_worktree.rs`에서 `main`의 시작·채택을 허용하던 넓은 브랜치 경계에 -2점을 적용했다. 이 문제는 실제 무단 배포 기록이 아니라 실행 경로의 과잉 권한이다.

## 수정한 실제 경로

**소유 브랜치:** `dominion-sub3` 단일 정확한 문자열. `main`, `dominion-sub`(옛 공용 실험 브랜치), `dominion-sub1/2/4`, `dominion-champion`, 이름의 접두부만 일치하는 임의 브랜치는 일반 RSI 실행·비교·승인·채택 원본이 될 수 없다.

- `src/engine/rsi_worktree.rs`: `rsi_work_branch`를 단일 브랜치로 제한. `owns_application_branch`는 원래 작업의 기록된 브랜치가 **현재 소스 브랜치와 정확하게 같아야** 참을 반환한다. 기록이 빠진 레거시 세대는 적용 불허.
- `rsi_worktree::prepare`: 소유한 원본 작업 브랜치에서만 일반 worktree를 만든다. **내부 캠페인 자식**은 분리된 detached lab worktree를 사용할 수 있으며, `rsi_evolve::campaign_source`는 해당 경로가 `data_dir/rsi-labs` 바로 아래의 실제 비공개 작업공간인지, 원본 소스와 서로 다른지, 기존 캠페인과 소유 브랜치가 일치하는지 함께 검사한다. 메인 체크아웃을 가짜 `labDir`로 등록해 실행 검사를 우회할 수 없다.
- `rsi_worktree::apply_to_sub`: 같은 커밋을 가리키는 `main`에도 패치를 적용할 수 없다. 사용자 확인·영수증 확인·기존 소스 상태 확인 뒤에도 마지막 `git apply` 직전에 `git symbolic-ref --quiet HEAD == refs/heads/dominion-sub3`를 다시 검사한다.
- `rsi_worktree::apply_campaign_to_sub`: 현재 브랜치, 캠페인 `sourceBranch`, 실제 `refs/heads/`가 일치하는 기존 검증된 fast-forward 경로를 그대로 유지한다. 변경된 브랜치 정책이 main에도 자동 적용되는 허용을 제거한다.
- `src/api/rsi.rs::start`: **수동 개선과 연속 개선** 모두 시작 전에 원본 소유 브랜치 및 미저장 변경 여부를 검사한다. 종전 `continuous`의 브랜치 검사 우회는 제거했다.
- `src/engine/rsi_evolve.rs::run`: API를 우회해 이미 대기 중이거나 직접 제출된 캠페인이 들어와도 Git 실제 브랜치와 저장된 `rsiSourceBranch`가 모두 `dominion-sub3`인지 확인한다. `campaign_source`의 예외도 원본 캠페인의 기록된 브랜치와 자식 실행 입력이 서브3인지 확인한다.

## 보존한 기능

- 기존 재컴파일 후손은 캠페인의 실험용 detached lab에서 계속 활동할 수 있다.
- 독립 평가·검증 후 도달한 커밋은 오직 서브3 소유 브랜치에만 승격할 수 있으며 메인에 직접 반영되지 않는다.
- 이전 캠페인의 영구 Pareto 아카이브, 원본 평가 증거 재생, 후손 성과 계승은 그대로 유지한다.

## 새 반증 테스트와 검토점

1. `main`과 모든 다른 참가자 브랜치를 소유 브랜치로 판정하지 않음.
2. 소스 브랜치가 기록되지 않았거나 잘못된 이전 작업은 계승·채택하지 않음.
3. 같은 SHA를 공유하는 `main`에 **기존 서브3 대기 작업을 실행하려는 시도**와 **수동 패치를 적용하려는 시도**를 각각 차단.
4. API 수동/연속 캠페인을 `main`에서 시작하면 새 작업이 큐에 생성되지 않음.
5. `dominion-sub3`에서만 깨끗한 소스를 가지고 기존 수동/연속 캠페인을 생성할 수 있음.
6. 재귀 캠페인 실행기가 `rsiSourceBranch`와 실제 Git 브랜치의 정확한 일치를 확인함.
7. 메인 체크아웃을 내부 실험 작업공간으로 잘못 등록했을 때 예외를 거부하고, 정상적인 격리 작업공간은 통과시킴.

**검증 상태:** 위 제어 경로 및 반증 테스트를 GitHub 브랜치에 커밋했다. 현재 작업 환경에 `cargo`와 `rustc`가 없어 컴파일·테스트·CI·실모델 실험을 실행하지 않았다. 이번 변경의 실제 심판 점수는 다음 재채점으로 결정된다.
