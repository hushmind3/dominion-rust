# DOMINION | 수동 재귀적 자기개선(RSI) 운용 명세

기준: 2026-11-09, `dominion-sub`. 기존 Rust 실행 엔진을 보존하면서 알고리즘·하네스·개선기를 모두 개선 대상으로 취급한다.

현재의 버튼 기반 반복 실행과 직접/자동 선택은 [RSI-EVOLUTION-2026-11-09.md](RSI-EVOLUTION-2026-11-09.md)를 참고한다. 아래는 기존 수동 등록·평가 API의 운용 명세이며, 새 실험 계보와 운영 배포는 구분한다.

## 현재 구현

`src/engine/rsi.rs`는 코드 변경 후보를 `algorithm` / `harness` / `improver`로 분류하고, 원본 git SHA·patch SHA-256·실험 계획·생성 모델·부모 후보를 보관한다. `src/api/rsi.rs`는 등록/조회/평가/수동 승인을 연결한다.

코드 패치를 등록하는 것과 적용하는 것은 다르다. API는 소스를 변경하거나 빌드·재시작·Git 병합을 실행하지 않는다. `approved_for_merge`는 운영 승격이 아니라 **수동 검토를 통과한 후보**다. 실제 운영 버전을 바꾸려면 사람이 별도 코드 리뷰 후 커밋·릴리스해야 한다.

## 실행 프로토콜

1. 실패 기록에서 구체적인 문제를 정하고, 원본 40자리 Git SHA·평가 기준·비용 예산을 고정한다.
2. 고성능 LLM 또는 사람이 원인 분석과 독립 가설을 만들고 실제 git unified diff 패치를 작성한다. 변경 파일은 `src/engine/`, `src/api/`, `tests/`, `web/src/`, `src-tauri/src/`로 제한된다.
3. **후보를 실행하기 전** 독립 평가 계획을 작성한다. baseline과 candidate에 동일 oracle·테스트 케이스를 적용한다. 새로운 테스트를 추가할 때는 테스트 전용 `fixturePatch`를 두 변형 모두에 적용한다.
4. `scripts/rsi-lab.sh`를 격리된 개발 환경에서 수동 실행한다. 2개의 Git worktree에서 동일한 릴리스 빌드와 지정된 Rust 테스트를 실행하여 원본·후보별 성공 여부, 로그 증거 및 SHA-256 보고서를 생성한다.
5. 실험 보고서를 `/api/rsi/:id/evaluate`에 등록한다. 최소 8개 독립 paired 사례, 2개 holdout, 새로운 성공 ≥1, 회귀 0, 양쪽 빌드 성공이 필요하다. `improver`는 holdout 도메인 2개 이상이며 그중 개선된 사례가 있어야 한다.
6. 실험 증거를 사람이 열람한 후 `APPROVE`로 수동 병합 승인한다. 이전 채택 개선안을 다음 세대의 `parentCandidateId`로 지정하고 개선기 버전을 추적한다.

## 동결 평가 계획의 형식

```json
{
  "oracleVersion": "room-owner-state-v1",
  "cases": [
    {"id":"correction","domain":"memory","holdout":true,"filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},
    {"id":"long-recall","domain":"memory","holdout":true,"filter":"engine::room_state::tests::a_hundred_unrelated_owner_decisions_do_not_evict_a_relevant_early_fact"}
  ]
}
```

위 형식은 예시이고, **승격하려면 사례를 8개 이상** 준비한다. 기준 커밋에 없는 회귀 테스트를 비교에 사용할 때는 `fixturePatch`로 테스트만 양쪽에 공통 적용한다. 후보에만 테스트를 추가해 얻은 통과율은 동일 조건의 개선 증거가 아니다.

```bash
bash scripts/rsi-lab.sh "$(git rev-parse HEAD)" /path/candidate.patch /path/frozen-cases.json /tmp/rsi-report.json
```

실행 결과: `/tmp/rsi-report.json` (API 제출용)과 `/tmp/rsi-report.json.evidence.txt` (사람이 확인할 로그). 빌드 실패, 테스트 미발견 또는 회귀는 승인 조건을 만족하지 못한다.

**보안 구분:** Git worktree는 격리된 실행/보안 샌드박스가 아니다. `cargo test`에서 후보 코드를 실행하므로, 신뢰할 수 없는 패치는 일회용 컨테이너/VM에서 실행한다.

## API

| HTTP | 작업 |
|---|---|
| `POST /api/rsi` | 가설·patch·baseCommit·cases·생성 모델·부모 후보 등록 |
| `GET /api/rsi` / `GET /api/rsi/:id` | 상태·패치·증거·개선 계보 조회 |
| `POST /api/rsi/:id/evaluate` | 수동 실험 보고서의 SHA/사례/holdout/회귀 조건 검사 |
| `POST /api/rsi/:id/decide` | `action=approve/reject`, 승인 시 `confirmation=APPROVE`와 baseCommit·patchSha256 일치 필수 |

예시 후보 등록 내용: `target`, `baseCommit`(Git SHA), `patch`(전체 git diff), `hypothesis`(가설), `cases`(id·domain·holdout), `generatorModel`, `generatorEffort`, `parentCandidateId`. 등록 시 제출한 patch와 평가 시 제출한 해시는 일치해야 한다.

평가 결과의 `reportSha256`는 실험 로그 증거 파일의 해시다. **API는 제출 자료의 진위를 독립적으로 인증하지 않는다.** 이는 수동 실험·수동 코드 리뷰 체계이며 검증된 완전 자동 RSI로 표시하지 않는다.

## 첫 수동 개선 이력

- **문제:** `room_state.rs`가 명시적 확정 상태 24개만 유지하고, 영문 대문자 코드 토큰 일치로만 정정을 대체하며, 최신 12개만 문맥에 주입함.
- **가설:** 많은 무관한 발언 사이에 오래된 사실이 남고 한글 정정을 반영하려면 관련성 기반 검색과 한국어 도메인 정정이 필요함.
- **변경:** `4f0c8ff`에서 256개 상태 유지, 질문 관련성 기반 선택, 암호·배포 담당자 등의 한국어 도메인 정정 처리 도입.
- **관측:** 신규 회귀 테스트 3개 포함 GitHub Actions 성공. 실제 모델 100턴/9명 실험은 아직 재실행하지 않았고 원본과 후보를 같은 동결 fixture로 비교한 E2 결과도 없음.
- **해석:** 이것은 사람에 의한 L2 알고리즘 개선의 첫 구현이며, 개선기 자체의 성능이 개선됐다고 주장할 증거는 아님.

## 재귀적 개선 합격 조건

`selfimprove.search`/`rsi.rs` 등의 개선기 A와 B를 **같은 LLM, 추론 단계, 도구 목록, 토큰 상한, 작업/holdout 세트**에서 비교한다. 최종 점수는 개선기가 바꿀 수 없는 고정된 외부 평가기로 산출한다. B가 A보다 더 나은 후보를 찾아서 실제 후속 작업 성능이 오르거나 동일 성능을 적은 비용으로 달성하는 것이 독립적으로 반복돼야 차세대 개선기로 승격한다.

운용 순서: 기억 회상 → 교정 인과 연결 → 자율 도구 선택 → 하네스 복구 → 개선기 탐색/평가 알고리즘 자체 개선. 전체 과정에서 `main`은 사용자의 승인 없이 수정하지 않는다.

## 적용 확인과 다음 세대

개선기가 승인됐다는 이유만으로 다음 세대로 연결하지 않는다. 승인 후 실제 Git 수동 병합 및 CI·운영 검토를 거치고, `POST /api/rsi/:id/deployment`에서 `commitSha`, `operator`, `confirmation:DEPLOYED`, `ciEvidence`를 기록한 후보만 부모로 허용한다. 이후 후보는 `parentCandidateId` 및 `generatorSourceCommit`(부모가 적용됐다고 보고한 커밋)을 일치시켜야 한다. 이 배포 기록은 **운영자 보고이며 서버가 Git을 독립 조회한 증거는 아니다.**

실험 등록 시 `oracleVersion` 및 모든 `cases[].filter`를 동결한다. 평가 결과의 oracle 버전이 달라지면 거절한다.
