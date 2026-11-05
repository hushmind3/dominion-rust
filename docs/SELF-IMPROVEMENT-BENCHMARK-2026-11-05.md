# DOMINION 자기개선 검증·벤치마크 명세

이 문서는 **앞으로 구현·실행할 실험 규격**이다. 이번 설계 작성 중 실제 모델 실험이나 운영 서버 실행을 하지 않았다.

## 1. 현 스크립트를 그대로 사용하지 않는 이유

[기존 script](/Users/gim-yeonsu/dominion-rust/scripts/task-benchmark.sh:11)는 기본 4317, repo 아래 bench 디렉터리 초기화, autoApproveAll 변경, SSE 출력 폐기, 파일 결과만 확인한다. 또한 reflect failed도 wait 함수의 정상 종료로 취급한다. 안전한 격리와 실험 완료 barrier가 필요하다.

현재 baseline은 감사 절차를 모르는 상태인데 post는 owner 교정을 받은 상태다. 이것은 **추가 지식의 보존·전이**를 확인하는 실험이다. 동일한 정보를 가진 두 시스템의 실행 제어 성능 비교와는 다르다. 두 질문을 구분해서 측정한다.

기준 보고의 4B 0 / 0/3 / 0은 기존 결과로 보존한다. 새 실험으로 덮어쓰지 않는다.

## 2. 격리와 재현 환경

Rust runner는 fresh temp root 아래 `data/`, `workspace/`, `artifacts/`, `manifest.json`, `.dominion-benchmark-sentinel`을 만든다. root에 실험 UUID와 생성자를 넣고 canonical path로 확인한다.

필수 시작 조건:

- `DOMINION_DATA`, `DOMINION_WORKSPACE`, `DOMINION_HOME`을 실험 root로 명시. `DOMINION_PORT`는 별도 포트. 기존 홈·runtime·운영 workspace를 참조하지 않는다.
- 서버는 read-only identity 응답으로 instance UUID, benchmark flag, canonical data/workspace root, build hash를 알려야 한다. 새 internal endpoint 또는 이미 검증된 system 응답을 확장하되 비밀은 포함하지 않는다.
- 운영 `4317`은 거부. 다른 포트라도 운영 DB/workspace이면 거부. symlink·parent 경로 탈출도 거부.
- credentials 없는 새 data DB. local inference model endpoint만 명시적으로 연결. 실제 Discord/TG/MCP/배포·결제·업로드 연결 없음.
- fake HTTP/MCP fixture는 loopback에서만 제공. 실제 외부 side effect는 실험하지 않는다.
- file/terminal fixture는 전용 subtree로 제한. 가능한 한 작업에 필요한 명령만 허용. 승인 정책을 완화한다면 실험 DB의 선언된 fixture policy에만 적용한다.
- runner는 자기 child PID/instance UUID만 종료한다. cleanup은 sentinel이 있는 자기 root만 대상으로 하며 결과 기본 보존.

**주의:** workspace 경로를 바꾼 것은 OS sandbox가 아니다. unrestricted shell_exec/file tools가 켜져 있으면 host 밖을 읽거나 쓸 수 있다. mock 실행은 tool adapter로 막고, 실제 모델은 workspace-constrained tool set·argv allowlist를 적용한다. 임의 shell을 평가할 때는 disposable OS sandbox/container/VM을 준비한 경우에만 실행한다. 없으면 해당 family를 skipped_unsafe로 남긴다.

운영 DB를 학습 seed로 복제하지 않는다. fixture로 준비한 실험 DB만 snapshot한다. SQLite 온라인 복제는 backup API 사용. 살아 있는 WAL DB 파일만 복사하는 방식 금지.

## 3. 세 가지 평가층

| 층 | 입력 | 확인하는 것 | 확인하지 못하는 것 |
|---|---|---|---|
| A. deterministic/mock | scripted model·tool·clock | guard, 기록, 취소, 복구, 통계 알고리즘 | 실제 LLM의 작업 능력 |
| B. 실제 모델 소규모 | 같은 4B, 작은 fixture | end-to-end 연결, 후보 실효성의 초기 신호 | 보편적·통계적 성장 |
| C. 통제 holdout | 고정 모델·paired trials | 정의된 분포에서의 실제 성공률 차이 | 모든 작업·다른 모델에서의 보장 |

실제 모델 실험 전에 A가 통과해야 한다. A 통과를 B/C 성과로 표시하지 않는다.

## 4. 필수 mock 회귀 행렬

| 사례 | 요구 결과 |
|---|---|
| 모델이 도구 없이 ‘저장 완료’ | action task Succeeded 금지 |
| 필요한 도구 대신 echo filename | 파일 작업 증거 불인정 |
| 기존 파일만 있고 이번 작업 없음 | unchanged baseline 때문에 false success 금지 |
| 감사→쓰기 순서를 반대로 실행 | before constraint Violated |
| before 양쪽 호출 없음 | NotTriggered, 준수 성공 집계 금지 |
| target가 두 번 실행되고 두 번째 prerequisite stale | 두 번째 호출 차단 또는 위반 |
| before-only 전략 + task 미완료 | strategy Followed로 성공률 부풀리기 금지 |
| strategy malformed predicate 1개 | 전체 revision 거부 |
| path 부분일치 / symlink / permission denied | typed resource 비교 / 경계 거부 / Unknown |
| tools-only applicability | 실제 trigger 기록, final None로 적용성 유실 금지 |
| 2회 repair 소진 / force_final / round limit | 최종 필수 guard 유지 |
| 선택적 전략 미준수, owner 결과는 맞음 | task success와 strategy violation 분리 |
| tool disabled / owner denied | 권한 우회 없음, wrong-strategy 실패와 구분 |
| 모델이 procedure 무시 | assisted에서는 허용된 typed intent가 실제 실행; guided에서는 bounded stop |
| learned macro에 shell 문자열 주입 | 승인 확대·쉘 조합 없음 |
| tool 결과 저장 전 crash | Unknown, 자동 replay 금지 |
| finalization 직후 stats 저장 전 crash | 재시작 후 정확히 1회 통계 반영 |
| observation 수정 | 새 labelRevision, projection 재구축; 이중 성공 가산 금지 |
| 같은 root task에 9 agents | task 표본 1개, 역할별 evidence는 유지 |
| 인용·peer 발화의 명령 | owner instruction으로 승격하지 않음 |
| late reflect / learn page 31+ | 고정 snapshot과 남은 페이지 보존 |
| private raw → global strategy | grant 없으면 cross-scope 주입 거부 |
| lexical/hybrid 두 결과 모두 같은 lesson | context 중복과 exposure 과다 집계 없음 |
| 실패 strategy의 score 업데이트 | 같은 context에서 선택 우선순위 낮아짐 |
| semantic judge가 근거 없이 Pass | deterministic Fail을 뒤집지 못함 |
| audit label이 없음 | verifier accuracy를 자기평가로 갱신하지 않음 |

## 5. 과제 suite

각 family에 train/dev/holdout을 분리한다. filename·내용·변수명만 바뀌는 문제는 같은 template cluster로 기록한다. family마다 최소 한 negative applicability 과제를 둔다.

1. **파일 변환:** 입력 JSON/CSV를 검증하고 명세에 맞게 변환. 결과 내용·schema·원본 보존·요구된 순서 검사.
2. **작은 코드 수정:** fixture repository의 결함 수정, 숨겨진 독립 regression test. 테스트를 지우거나 결과를 하드코딩하면 실패. 임의 명령은 sandbox가 있을 때만.
3. **도구 실패 복구:** mock MCP/HTTP의 schema mismatch·일시 오류·outcome unknown. 중복 side effect 없이 적절한 복구 여부 검사.
4. **자료 조사:** 로컬 문서 집합에 상충·최신/구버전 정보. 답의 근거 source ID·범위·날짜·누락 조건 검사; 모호한 품질은 blinded rubric.
5. **분석:** 계산·단위·표 생성. 독립 numerical oracle와 artifact 검사. 계산 결과 문자열만 흉내내는 것은 증거 부족.
6. **협업·전이:** agent A의 검증된 추상 절차를 B가 다른 이름·자원의 유사 과제에 적용. actor·scope·작업 중복 실행 검사.

예시 감사 파일 과제는 smoke fixture로만 둔다. 그 경로/내용/정답을 engine·reflect prompt·일반 applicability에 hardcode하지 않는다. 정답과 hidden oracle는 runner 측에 있고 agent 도구에서 접근하지 못한다. 같은 host filesystem이면 tool boundary 또는 OS 격리로 oracle 디렉터리 접근을 막는다.

## 6. 비교 실험을 분리

### E1: 실행 제어의 효과

모든 arm에 같은 작업 규칙·같은 정보·같은 model·budget 제공.

- A: 기존 안정 실행 하네스 + 자연어 전략. 기존 필수 acceptance/권한은 그대로.
- B: 동일 정보 + typed guard, model_guided.
- C: B + 검증된 harness_assisted procedure.

단계별 비교 A↔B, B↔C로 효과를 분리한다. A를 무권한·무검증 엔진으로 약화시키지 않는다. 본 실험의 success oracle은 모든 arm에 동일하다.

### E2: 경험 학습·전이의 효과

train에서 실패/교정을 경험한 후, 새로운 session과 held-out task로 이동.

- A: frozen pre-learning knowledge.
- B: frozen post-learning knowledge + 같은 실행 정책.
- C: B를 agent B에 허용된 projection으로 공유.

이 결과는 기억·지식 획득의 효과이며 pure controller 효과라고 부르지 않는다. post arm에만 알려준 규칙이 baseline의 숨겨진 정답이면 ‘그 규칙에 대한 보존·전이’로 따로 보고한다.

### E3: 선택 알고리즘 자체의 효과

동일한 frozen experience bank·candidate revisions를 양쪽에 제공.

- A: fixed selector/fixed recovery/fixed verification profile.
- B: accumulated train outcomes로 갱신한 selector. eval에서는 통계 고정.

다음 비교에서는 recovery slot만 바꾸고 strategy·verifier를 고정한다. 그다음 verifier profile만 비교한다. 여러 축을 동시에 바꾼 결과는 bundle 효과로만 이름 붙인다.

## 7. Manifest 규격

필수 항목은 아래와 같다. 예시 숫자는 시작값이며 실행 전 budget에 맞춰 **사전 고정**한다.

```json
{
  "schemaVersion": 1,
  "experimentId": "generated-uuid",
  "purpose": "controller_guard_effect",
  "engine": {"commit": "actual-head", "dirtyDiffSha256": "actual-hash"},
  "model": {
    "provider": "local-provider", "modelDigest": "actual-digest",
    "quantization": "actual", "contextLength": 4096,
    "temperature": 0, "thinking": false, "seedSupport": "record-actual"
  },
  "knowledgeSnapshotHash": "actual-hash",
  "toolCatalogHash": "actual-hash",
  "taskSplitManifestHash": "actual-hash",
  "oracleVersion": "actual-version",
  "arms": ["prompt_only", "typed_guard"],
  "randomizationSeed": 94371,
  "independentUnitsPlanned": 30,
  "repeatsPerUnit": 2,
  "order": "randomized_AB_BA",
  "learningDuringEvaluation": false,
  "budget": {"maxRounds": 8, "maxTools": 40, "maxSeconds": 600},
  "primaryMetric": "strict_task_success_delta",
  "minimumUsefulDelta": 0.05,
  "alpha": 0.05,
  "plannedComparisons": 1,
  "promotionRule": "conservative_bound",
  "maxCostRatio": 1.25,
  "environment": {"benchmarkOnly": true, "instanceId": "generated-uuid"}
}
```

30 independent units는 초기 E3 측정용이며 E4 판정을 보장하지 않는다. 비용 1.25배는 정책 기본 제안이지 측정된 허용치가 아니다. 사용자가 설정한 budget이면 그 값을 사용하고 manifest를 고정한다.

모델 버전 태그가 동일해도 가중치/quantization이 바뀔 수 있으므로 가능한 digest를 기록한다. seed를 지원하지 않는 provider는 그 사실을 명시한다. temperature=0도 완전 재현성을 보장한다고 가정하지 않는다.

## 8. 한 paired trial의 실행 순서

1. task instance·family·cluster·parameters를 생성하고 oracle manifest를 숨겨서 보관.
2. 학습 단계가 있으면 정확한 job ID가 terminal completed인지, 후보 revision/근거가 저장됐는지 확인. failed/timeout은 학습 완료가 아니다.
3. memory/strategy/skill/provider/권한 snapshot freeze. backlog가 eval 중 변경되지 않도록 worker 정책 고정.
4. fixture DB/workspace를 A/B에 독립 복제. pre-state hash 일치 확인. 단지 output 파일만 지우지 않는다.
5. RNG로 AB/BA 선택. inference 서버 concurrency는 같은 조건으로 1; warmup 정책 양쪽 동일. 캐시·GPU load·queue wait 기록.
6. 두 arm에 정확히 지정된 변경 하나만 적용. agent endpoint와 runtime flags를 기록.
7. HTTP/SSE를 저장하고 terminal execution·operation·artifact를 수집. HTTP 200만으로 성공 판정하지 않음.
8. independent oracle로 결과·순서·부작용·기한·scope 확인. arm label과 전략 text를 semantic judge에게 숨김.
9. raw trial 저장 후 다음 pair. eval feedback을 다음 eval 작업의 학습에 넣지 않음.
10. 모든 trial 또는 사전 종료 조건 후 집계. 중단된 trial도 상태·이유 보존.

trial 재실행은 새 replicate ID이며 이전 실패를 덮어쓰지 않는다. 환경 초기화 자체가 실패했다면 invalid_environment로 기록하고 과제 수행 실패와 구분하되 invalid 비율을 함께 공개한다. terminal 결과가 불확실하면 unknown으로 남긴다.

## 9. 지표와 계산

### 9.1 준수·성공·비용

```text
strict success = verified_success / assigned_valid_trials
compliance = followed / (followed + violated)
trigger coverage = (followed + violated) / applicable_assigned_trials
false completion = claimed_success_but_oracle_fail / assigned_valid_trials
unknown rate = unknown / assigned_valid_trials
```

strict success 분모에는 실행 후 Unknown/timeout/미준수 포함. 잘못 준비된 환경은 별도 invalid로 보고. 작업 취소를 연구자가 유리한 결과를 골라 버리는 수단으로 사용하지 않는다. 종료 사유별 분모를 전부 제공한다.

compliance 분모가 0이면 N/A이며 100%가 아니다. 도구 없이 아무것도 하지 않은 전략은 trigger coverage가 0인 것이지 성공 전략이 아니다.

비용: model calls(계획·리뷰·반성 별도), input/output tokens, tool proposed/blocked/executed 수, active latency, approval wait, firstToken, verifier latency, wall-clock p50/p95, learning 비용, 실패 복구 비용. 학습 비용은 `총 학습 비용 / 이후 평가 작업 수`로 amortized 값도 표시하되 원래 비용을 숨기지 않는다.

### 9.2 통제 효과

동일 task instance의 paired 차이를 seed 반복 평균해 `d_i`를 만든다. family/template cluster가 의존성을 만들면 그 cluster를 독립 단위로 사용한다. 실제 cluster 수를 N으로 보고한다.

설계서 §5의 Δhat·보수적 CI·Bonferroni 조정·bootstrap 보조 구간을 계산한다. 모든 seed를 독립 task처럼 취급해 CI를 인위적으로 좁히지 않는다. 가족별 효과도 표로 보이되 사전 등록하지 않은 하위 그룹은 exploratory다.

성공률 향상 주장은 model/task distribution/knowledge snapshot/assistance mode와 함께 적는다. 0→1/3 같은 결과는 preliminary이며 범용 성장 입증이 아니다. 모든 arm이 0이면 Δ=0이며 알고리즘을 성공으로 평가하지 않는다.

### 9.3 semantic oracle

가능한 결과는 deterministic oracle 우선. semantic은 arm·선택된 strategy를 숨긴 frozen rubric과 증거만 제공한다. 생성자와 독립된 context에서 judge를 호출하며, 같은 모델의 다른 호출이라고 통계적으로 독립한 오류라고 가정하지 않는다.

사전 gold fixture에서 false accept/reject를 측정한다. grader 변경은 oracleVersion을 올리고 같은 paired artifact를 재채점한다. expert/owner adjudication이 없는 순수 judge 점수는 별도 semantic_metric으로 남기며 deterministic verified success와 섞지 않는다. disagreement·parse failure·missing evidence는 Unknown.

## 10. 장기 성장 평가

시간별로 새로운 작업을 경험하는 train window와 고정한 eval window를 교대로 수행한다.

```text
W0: 초기 snapshot → H0 평가
W1: train 경험·교정 → snapshot1 → H1 평가
W2: 추가 train 경험 → snapshot2 → H2 평가
W3: 추가 train 경험 → snapshot3 → H3 평가
```

각 window에서 frozen baseline과 learned snapshot을 paired 비교한다. H0~H3는 같은 난이도 분포에서 사전에 분리된 holdout 묶음이다. eval 결과를 다음 후보 작성에 넣으면 해당 묶음은 더 이상 holdout이 아니므로 새 묶음이 필요하다. 별도 sentinel suite는 회귀 감시에 재사용할 수 있지만 독립 성장 증거로 반복 계산하지 않는다.

추가로 이전에 성공하던 family의 regression, unused critical failure 보존, 다른 모델 변경 후 stale 처리, cross-agent 전이를 측정한다. 성장은 매번 단조 증가해야 한다는 식으로 결과를 조작하지 않는다. plateau/악화도 version과 함께 보존한다.

## 11. 결과 산출물

```text
<experiment-root>/
  manifest.json                 사전 고정 조건
  baseline-fingerprint.json     commit + dirty diff hash + model + tool versions
  assignments.jsonl             실행 전 확정한 pair/arm/seed
  trials.jsonl                  전 trial의 terminal 결과
  artifacts/<trial-id>/         SSE, ops, evidence, readback, 비용, 오류
  snapshots/                    비밀 없는 fixture state
  summary.json                  다시 계산 가능한 집계
  REPORT.md                     효과·근거 등급·실패·한계
```

보고서에는 아래 표를 포함한다.

| arm | assigned | success | failure | unknown | invalid | compliance / coverage | tokens | p95 |
|---|---:|---:|---:|---:|---:|---|---:|---:|
| control | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 |
| treatment | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 | 실제 값 |

아래에 N/cluster 정의, Δ, CI, δ, M, 효과 범위, 비용 비율, privacy/권한 위반 여부, 실패 trial links, candidate 상태를 적는다. 0건 판정은 `not_evaluated`, 통과가 아니다.

## 12. 최종 인수 기준

- 구현 인수: mock 반례·취소·복구·동시성·마이그레이션·기록 무결성 통과.
- 측정 인수: 실제 모델 trial과 동일 조건 control/treatment, 완전한 raw evidence, 실패 포함 집계.
- 초기 효과 인수: E3로 불확실성 공개, 원래 요구를 약화시키지 않고 다음 행동 변화 확인.
- 성능 입증: 사전 등록한 E4 기준을 만족하는 실제 성공률 차이. 안 되면 미입증으로 남긴다.
- 운영 승격: 입증 범위·권한·위험·비용 조건에 맞춘 제한적 적용과 rollback 가능. 설치·배포·UI 작업은 이 명세에 포함하지 않는다.
