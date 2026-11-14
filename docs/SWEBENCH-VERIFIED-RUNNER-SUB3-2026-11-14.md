# DOMINION Sub3 — SWE-bench Verified 실제 실행 드라이버

이 경로는 **공식 문제 JSONL → 독립 Git 워크트리 → 독립 Rust 서버/SQLite → 원래 Dominion goal 엔진 → 실제 Git diff → SWE-bench 공식 제출 JSONL**로 이어진다. 응답 텍스트나 모의 패치를 제출하지 않는다.

## 준비물

- 실제로 빌드된 Dominion Rust 바이너리 (\`target/release/dominion-rust\`)
- SWE-bench Verified 공식 데이터셋에서 내보낸 JSONL (각 레코드에 \`instance_id\`, \`repo\`, \`base_commit\`, \`problem_statement\`)
- 로컬 Git 저장소 클론 (예: \`/bench/repos/django/django\`, \`/bench/repos/psf/requests\`). 매 문제의 \`base_commit\`을 객체 저장소에 보유해야 한다.
- **실제로 연결되는** 모델 제공자 설정 (예: \`/bench/provider.json\`: \`{"name":"Benchmark Provider","kind":"openai","baseUrl":"https://YOUR-API/v1","model":"YOUR-MODEL","toolMode":"native"}\`). API 키는 \`DOMINION_BENCH_API_KEY\` 환경 변수로도 전달할 수 있다. 이 문서에는 비밀 키를 저장하지 않는다.

## 실행

\`\`\`sh
node scripts/swebench-verified.mjs \
  --dataset /bench/verified.jsonl \
  --repos /bench/repos \
  --binary ./target/release/dominion-rust \
  --provider /bench/provider.json \
  --output benchmark/swebench-verified-predictions.jsonl \
  --metrics benchmark/swebench-verified-metrics.jsonl \
  --iterations 12 --tokens 120000 --limit 10 --approve-tools
\`\`\`

실험 중단 뒤 기존 제출 파일이 남아 있으면 \`--resume\`으로 완료된 \`instance_id\`를 건너뛴다. 전체 평가를 수행하려면 \`--limit\`을 생략한다.

\`--approve-tools\`는 **오직 새로 시작한 일회용 Rust 서버의 SQLite 정책**에만 원격 셸·파일 편집 도구 승인 목록을 기록한다. 실행이 가능해지려면 필요한 옵션이다. **Git 워크트리는 소스 충돌 격리이지 호스트 OS 샌드박스가 아니므로**, 비신뢰 저장소를 실행하는 공식 대규모 평가는 독립 VM/컨테이너에서 실행한다.

## 어떤 정보를 기록하는가

예측 파일은 공식 스키마 그대로 \`instance_id\`, \`model_name_or_path\`, \`model_patch\` 세 필드만 기록한다. \`model_patch\`는 정확한 기준 Git 커밋에서 추적한 변경과 새 파일을 포함하는 **실제 \`git diff --binary\`** 결과다. 제출 결과가 0바이트라면 패치가 없었음을 뜻한다. \`test_patch\`, \`hints_text\` 및 공식 정답 테스트는 프롬프트에 들어가지 않는다.

별도 메트릭 파일은 \`goalStatus\`, \`goalVerifiedByNativeHarness\`, \`iterations\`, \`replans\`, \`executedToolCalls\`, \`modelCalls\`, \`totalTokens\`, \`patchBytes\`, \`elapsedMs\`, \`failure\`를 저장한다. \`officialSWEbenchVerdict\`는 의도적으로 **null**이다. 자체 goal 완료나 독립 컴파일 성공만으로 SWE-bench를 통과했다고 주장하지 않는다. 토큰 사용량과 공급자 실청구 원화 금액도 구별한다.

## 공식 평가

공식 SWE-bench Docker 의존성을 독립 환경에 설치한 후, 위 예측 파일을 같은 원본 데이터셋과 정확한 테스트 조건으로 평가한다.

\`\`\`sh
python -m swebench.harness.run_evaluation \
  --dataset_name SWE-bench/SWE-bench_Verified \
  --predictions_path benchmark/swebench-verified-predictions.jsonl \
  --max_workers 4 \
  --run_id dominion-sub3
\`\`\`

완수율, 실패 복구율, 비용/성능 비교는 **공식 평가 결과 + 본 드라이버의 실행 추적**을 같은 \`instance_id\` 기준으로 결합한 뒤 계산해야 한다. 테스트 단위 계약: \`node --test scripts/swebench-verified.test.mjs\`. 이는 오라클을 속이는 단위 테스트가 아니라 **실제 Git 명령으로 수정된 파일과 새 파일이 패치에 포함되는지** 검사한다. 유료 모델/공식 Docker 채점은 자동 CI에서 실행하지 않는다.
