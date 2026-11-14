# DOMINION Sub3 — SWE-bench Verified 실제 실행 드라이버

이 경로는 **공식 문제 JSONL → 독립 Git 워크트리 → 독립 Rust 서버/SQLite → 원래 Dominion goal 엔진 → 실제 Git diff → SWE-bench 공식 제출 JSONL**로 이어진다. 응답 텍스트나 모의 패치를 제출하지 않는다.

## 준비물

- 실제로 빌드된 Dominion Rust 바이너리 (`target/release/dominion-rust`)
- SWE-bench Verified 공식 데이터셋에서 내보낸 JSONL (각 레코드에 `instance_id`, `repo`, `base_commit`, `problem_statement`)
- 로컬 Git 저장소 클론 (예: `/bench/repos/django/django`, `/bench/repos/psf/requests`). 매 문제의 `base_commit`을 객체 저장소에 보유해야 한다.
- **실제로 연결되는** 모델 제공자 설정 (예: `/bench/provider.json`: `{"name":"Benchmark Provider","kind":"openai","baseUrl":"https://YOUR-API/v1","model":"YOUR-MODEL","toolMode":"native"}`). API 키는 `DOMINION_BENCH_API_KEY` 환경 변수로도 전달할 수 있다. 이 문서에는 비밀 키를 저장하지 않는다.

## 실행

```sh
node scripts/swebench-verified.mjs \
  --dataset /bench/verified.jsonl \
  --repos /bench/repos \
  --binary ./target/release/dominion-rust \
  --provider /bench/provider.json \
  --output benchmark/swebench-verified-predictions.jsonl \
  --metrics benchmark/swebench-verified-metrics.jsonl \
  --iterations 12 --tokens 120000 --limit 10 --approve-tools
```

별도 원화 가격표를 계산 근거로 넣으려면 `--pricing /bench/pricing-krw.json`을 추가합니다. 예: `{"promptPerMillionKrw":6000,"completionPerMillionKrw":12000}`. 두 가격은 사용자가 공급자와 계약한 **실제 원화 토큰 단가**를 직접 입력하는 값입니다. 입력하지 않으면 추정 원가를 `null`로 남깁니다. 이 값으로 계산한 `estimatedProviderCostKrw`는 **실청구 영수증이 아니며 캐시 할인은 포함되지 않습니다.**

실험 중단 뒤 기존 제출 파일이 남아 있으면 `--resume`으로 완료된 `instance_id`를 건너뛴다. 전체 평가를 수행하려면 `--limit`을 생략한다.

`--approve-tools`는 **오직 새로 시작한 일회용 Rust 서버의 SQLite 정책**에만 원격 셸·파일 편집 도구 승인 목록을 기록한다. 실행이 가능해지려면 필요한 옵션이다. **Git 워크트리는 소스 충돌 격리이지 호스트 OS 샌드박스가 아니므로**, 비신뢰 저장소를 실행하는 공식 대규모 평가는 독립 VM/컨테이너에서 실행한다.

## 어떤 정보를 기록하는가

예측 파일은 공식 스키마 그대로 `instance_id`, `model_name_or_path`, `model_patch` 세 필드만 기록한다. `model_patch`는 정확한 기준 Git 커밋에서 추적한 변경과 새 파일을 포함하는 **실제 `git diff --binary`** 결과다. 제출 결과가 0바이트라면 패치가 없었음을 뜻한다. `test_patch`, `hints_text` 및 공식 정답 테스트는 프롬프트에 들어가지 않는다.

별도 메트릭 파일은 `goalStatus`, `goalVerifiedByNativeHarness`, `iterations`, `replans`, `executedToolCalls`, `successfulToolCalls`, `failedToolCalls`, `recoveredToolFailures`, `toolFailureRecoveryRate`, `verifiedToolEffects`, `modelCalls`, `promptTokens`, `completionTokens`, `totalTokens`, `estimatedProviderCostKrw`, `patchBytes`, `elapsedMs`, `failure`를 기록한다. 복구율은 **같은 execution에서 먼저 실패한 도구 호출 이후, 동일 대상 파일 또는 동일 도구에 대해 실제 성공한 호출을 관찰한 비율**이다. 실패 호출 표본이 없으면 복구율은 `null`이며 100%로 채우지 않는다. 독립적으로 검증된 파일 변경 개수는 별도로 센다. `officialSWEbenchVerdict`는 의도적으로 **null**이다. 자체 goal 완료나 독립 컴파일 성공만으로 SWE-bench를 통과했다고 주장하지 않는다. 토큰 사용량과 공급자 실청구 원화 금액도 구별한다.

## 공식 평가

공식 SWE-bench Docker 의존성을 독립 환경에 설치한 후, 위 예측 파일을 같은 원본 데이터셋과 정확한 테스트 조건으로 평가한다.

```sh
python -m swebench.harness.run_evaluation \
  --dataset_name SWE-bench/SWE-bench_Verified \
  --predictions_path benchmark/swebench-verified-predictions.jsonl \
  --max_workers 4 \
  --run_id dominion-sub3
```

2026년 공개된 SWE-bench Verified는 문제/정답이 공개된 평가라는 한계가 알려져 있으므로, **동일 조건 비교를 위한 참고 점수**로 사용한다. 프런티어 수행 능력을 평가할 때에는 처음 보는 내부 개발 과제·오염 위험이 낮은 홀드아웃 사례도 별도로 유지한다. 두 평가 점수를 합산하거나 정답 패치를 모델 입력에 노출하지 않는다.

완수율, 실패 복구율, 비용/성능 비교는 **공식 평가 결과 + 본 드라이버의 실행 추적**을 같은 `instance_id` 기준으로 결합한 뒤 계산해야 한다. 테스트 단위 계약: `node --test scripts/swebench-verified.test.mjs`. 이는 오라클을 속이는 단위 테스트가 아니라 **실제 Git 명령으로 수정된 파일과 새 파일이 패치에 포함되는지** 검사한다. 유료 모델/공식 Docker 채점은 자동 CI에서 실행하지 않는다.

## 2026-11-15 · 실패 복구율 근거 강화 (서브3)

`scripts/swebench-verified.mjs::measuredRecovery`는 Rust `verification::successful_evidence`에
준하는 실패 판정을 사용합니다. `status=completed`여도 셸 `exitCode != 0`,
`result.success=false`, `result.isError=true`, 비어 있지 않은 `result.error`,
수정 도구의 `written != true`는 성공이 아닙니다. `interrupted/running/outcomeUnknown`은
제3 범주인 `unknownToolOutcomes`로 따로 세며, 실패 복구율의 분모/분자에 임의로 넣지 않습니다.

`recoveredToolFailures`은 동일 runId의 뒤따른 실행 순서로만 인정합니다.
실패한 코드 편집에는 **같은 대상의 성공한 쓰기**가 필요하고,
실패한 셸 명령에는 **동일한 명령·작업 폴더의 종료코드 0**이 필요합니다.
실패 후 파일을 읽는 행위나 무관한 빌드 성공은 복구가 아닙니다.
단위 회귀는 `scripts/swebench-verified.test.mjs`에 있습니다.
별개로 독립 Docker evaluator만 공식 SWE-bench 해결률을 판정합니다.

참조: SWE-bench 2026 평가 CLI는 `swebench eval verified -p predictions.jsonl
--run-id <unique-run-id>`이며, 서로 다른 패치 비교에 동일 run-id를 재사용하면 캐시가
잘못된 결과를 반환할 수 있습니다. https://github.com/SWE-bench/SWE-bench/blob/main/README.md
