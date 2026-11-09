# 조사 보고서와 실험 증거

2026-11-08 조사 당시의 보고서·측정 자료를 2026-11-09에 함께 게시했다. 본문에 나오는 실행 상태·커밋·미게시 여부는 **작성 당시의 기록**이다. 이후 구현과 현재 상태는 별도 보고서를 확인한다.

## 보고서

- [대화 작업 문맥 구현과 검증](PARTICIPANT-WORKING-VIEW-2026-11-08.md)
- [토큰 비용 문제 최초 인계 — 미해결](HANDOFF-UNRESOLVED-TOKEN-COST-2026-11-08.md)
- [토큰 비용 재조사 — 미해결](TOKEN-CACHE-DIAGNOSIS-2026-11-08.md)
- [토큰 비용 조사 최종 인계](HARNESS-TOKEN-COST-HANDOFF-2026-11-08.md)

문맥의 연속성 구현과 실제 비용 절감은 구분한다. 아래 자료는 제한된 실험의 관측값이며, 비용 문제가 해결됐다는 증거가 아니다.

## 실험 증거

- [최신 운영 관측 9건](handoff-evidence/production-latest-observed.json)
- [운영 상태를 복사한 격리 재생](handoff-evidence/production-state-replay.json)
- [자동 학습을 켠 격리 재생](handoff-evidence/learning-enabled-baseline.json)
- [캐시 힌트](handoff-evidence/cache-hint-probe.json)
- [캐시 유지 옵션](handoff-evidence/retention-options-probe.json)
- [지속 세션](handoff-evidence/native-session-probe.json)
- [instructions/developer 경계](handoff-evidence/developer-boundary-probe.json)
- [텍스트/native 응답 재생](handoff-evidence/text-native-replay-probe.json)
- [세션 헤더 비교](handoff-evidence/session-header-comparison.json)
- [재조사 집계](handoff-evidence/cache-diagnosis-summary.json)

## 재현 스크립트

- [격리 방·자동 학습 관측](../../scripts/research/room-cache-probe.py)
- [세션 헤더 비교](../../scripts/research/codex-session-header-probe.py)
- [텍스트/native 응답 재생 비교](../../scripts/research/codex-replay-probe.py)

스크립트는 로컬 운영 상태를 읽고 실제 외부 모델을 호출한다. 실행하면 사용량과 비용이 발생할 수 있다. 이번 게시에서는 JSON 파싱과 Python 구문 검사만 수행했고 실험을 다시 실행하지 않았다. 인증정보·대화 원문·운영 데이터베이스는 게시 자료에 포함하지 않는다.

## 후속 보고서

- [브랜치 점검과 개선](../BRANCH-AUDIT-2026-11-09.md)
- [반복 자기개선 구현과 검증](../RSI-EVOLUTION-2026-11-09.md)
