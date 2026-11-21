# DOMINION 병렬 자율 개발 · 심판 오케스트레이터

사용자가 메인·서브2·서브3·서브4를 각각 대화로 깨우지 않아도 **하나의 명령으로 4개 독립 개발자와 읽기 전용 심판**을 한 회차 실행합니다. 각 개발자는 자기 브랜치의 실제 코드를 읽고 문제를 골라 수정·검사·커밋합니다. 독립 심판은 변경 코드·검사 기록을 비교하고 다음 회차에 전달할 브랜치별 피드백을 저장합니다.

**실행 위치:** 사용자 컴퓨터의 Git 저장소 + 로그인된 Codex CLI. ChatGPT 대화 탭을 원격으로 제어하지 않습니다. 설치된 OpenCode를 쓰려면 `--agent-command`에 지원되는 비대화형 명령을 지정할 수 있지만, 제공자별 도구 승인과 파일 접근 범위를 별도로 구성해야 합니다.

## 시작

```bash
# 별도 서버/감시 데몬을 만들지 않고 기존 git clone을 참조합니다.
python3 tools/dominion_autopilot.py --repo /Users/gim-yeonsu/dominion-rust --rounds 1

# 원하는 목표를 한 번만 지정하면 4개 개발자 모두 같은 작업을 경쟁적으로 구현
python3 tools/dominion_autopilot.py --repo /Users/gim-yeonsu/dominion-rust --goal "Chat에서 앞선 작업을 정확히 이어서 끝내기" --rounds 1

# 직접 승인한 경우에만 개별 후보 브랜치로 fast-forward push
python3 tools/dominion_autopilot.py --repo /Users/gim-yeonsu/dominion-rust --rounds 3 --publish

# 세션을 중지할 때까지 반복: Ctrl+C로 정지, 중간 기록은 보존
python3 tools/dominion_autopilot.py --repo /Users/gim-yeonsu/dominion-rust --rounds 0 --publish
```

**전제:** Python 3.10+, `git`, 로그인된 `codex` 실행 파일, 로컬 Git clone의 `origin`에 네 대상 브랜치가 있어야 합니다. 클라우드/모델 접근은 실제 Codex 사용자 설정을 사용하므로 호출 비용과 한도를 별도로 관리하세요. `--rounds 0`은 사용자가 명시적으로 켜야 하며 백그라운드 자동 시작 기능은 없습니다. CI 전체 완주를 무조건 기다리지 않습니다.

## 실제 동작

1. 사용자 목표를 `--goal`로 한 번 지정하면 4개 개발자에게 공통 전달합니다. 생략 시 각자가 가장 큰 사용자 불편을 찾아 개선합니다.
2. `origin/main`, `origin/dominion-sub2`, `origin/dominion-sub3`, `origin/dominion-sub4`의 최신 HEAD를 가져옵니다. 기존 감시 체크아웃을 checkout/reset하지 않습니다.
3. 저장소 **밖**의 `dominion-autopilot-state/worktrees/<round>/...`에 브랜치별 **분리된 detached worktree**를 만듭니다. 기본적으로 4개가 동시에 실행됩니다.
4. 개발자는 `codex exec --sandbox workspace-write --ephemeral`로 자기 worktree에서 수정합니다. 실패·미커밋·민감 파일 수정이면 업로드를 차단하고 결과를 남깁니다. `--publish` 없이는 푸시하지 않습니다. 푸시 충돌 시 강제 푸시하지 않습니다.
5. 심판은 별도 읽기 전용 worktree에서 `codex exec --sandbox read-only --ephemeral`로 증거를 검토합니다. CI 성공 자체에는 가점하지 않으며 측정하지 않은 실사용 성능은 추정하지 않습니다.
6. 푸시하지 않은 로컬 커밋도 다음 회차의 시작점으로 이어갑니다. 내용 변경이 없는 회차도 이전 커밋을 버리지 않습니다. 원격 브랜치가 그 사이 서로 다른 방향으로 진행되면 자동으로 버리지 않고 해당 후보를 검토 대기로 둡니다.
7. `latest_feedback.json`에 저장된 브랜치별 심판 의견을 다음 회차 프롬프트에 자동 전달합니다. 사용자에게 4번씩 피드백을 복사하라고 하지 않습니다.
8. `runs/<round>/<branch>/result.json`, `agent.log`, `judge/report.json`, `latest_round.json`을 보관해 상태를 추적할 수 있습니다.

**실행 결과를 '채택'으로 오인하지 않습니다.** 로컬 개발 커밋과 GitHub의 실제 푸시, CI, 유료 실모델 E2E는 서로 다른 단계입니다. 심판은 제품 파일을 수정하지 않으며 후보 간 병합이나 main 채용도 수행하지 않습니다.

## 안전 경계와 한계

- GitHub에서 이 개발 도구를 **메인에 자동 병합하지 않습니다.** 사용자 결정 후 적용할 수 있는 별도 관리 코드입니다.
- 출발 소스 HEAD 및 결과 SHA를 기록합니다. 다른 작업자가 동일 브랜치를 먼저 푸시하면 fast-forward 푸시만 시도하고 실패를 기록합니다.
- 기존 감시 체크아웃에 커밋하지 않은 사용자 변경이 발견되면 해당 브랜치의 자동 푸시를 보류합니다.
- 실수로 커밋된 `.env`, 비밀키, 로컬 SQLite 파일 등은 자동 업로드를 차단하지만 **완전한 비밀 탐지기는 아닙니다**. 자동 업로드를 켜기 전에 신뢰할 수 있는 사용자 컴퓨터에서 시험하세요.
- 파일·실행 로그는 저장소 밖에 보관하며 읽기 전용 심판도 이 경로를 읽습니다. 평가용 데이터와 운영 데이터는 분리해야 합니다.
- 독립 채점의 실제 100점은 **같은 모델·권한·예산·과제·빌드 SHA·산출물 검증**이 필요합니다. 이 도구는 그것이 없는 상태에서 실험 결과를 만들어내지 않습니다.
- 각 회차마다 분리된 worktree를 보존합니다. 감사·보관 후 `git worktree remove`로 개별 정리하세요. 실행 중 worktree를 지우거나 강제 reset하지 마세요.
- 브랜치별 독립 구현을 유지하며 UI 상단 메뉴·서비스 구조를 일괄 변경하지 않습니다.

## 검증

```bash
python3 -m unittest discover -s tests -v
```

로컬 가짜 Git 원격과 비대화형 모의 에이전트로 **4개 병렬 실행, 피드백 다음 회차 연결, 푸시 기본 차단, 4개 브랜치 fast-forward 푸시, 미커밋·민감 파일 자동 푸시 차단, 중복 컨트롤러 잠금, 중단 신호에 따른 작업 종료, 기존 로컬 커밋의 회차 간 보존, 감시 체크아웃 수작업 보호, 공통 목표의 4개 브랜치·심판 전달**을 검증합니다. 이 모의 검사는 실제 Codex API 작업 성공률이나 실모델 비용·지연 시험이 아닙니다.