# DOMINION — SWE-bench Verified, 실제 Rust 엔진 평가

**평가 대상:** `src/bin/dominion-swebench.rs`는 도미니온 본체인
`dominion-rust`를 인스턴스마다 새로 띄우고 `/api/goals`에 작업을 맡깁니다.
모델 호출, 계획, 탐색, 도구 실행, 재계획과 검증은 Rust 엔진이 담당합니다.
이 Rust 바이너리는 평가 데이터와 실제 작업에 필요한 실행 환경을 관리합니다.
공식 SWE-bench Python/Docker는 **정답 판정에만** 사용합니다.

## 실행 전 준비

- Linux/macOS, Rust stable, Git, 모델 API(또는 로컬 Ollama), 충분한 디스크 공간
- Python 3 및 `pip install datasets swebench` 설치
- 실제 공식 평가에는 Docker 데몬이 필요합니다.
- 코드를 변경하는 에이전트이므로 평가마다 별도 Git clone과 별도
  `DOMINION_DATA`, 포트, 작업 디렉터리를 만듭니다.
- 모델에 전달하는 입력은 `instance_id`, `repo`,
  `base_commit`, `problem_statement`뿐입니다.
  데이터셋의 gold patch, `test_patch`, 테스트 정답은 전달하지 않습니다.

```bash
cargo build --release --bins
python3 scripts/export_swebench_verified.py \
  --output benchmark/verified-5.tasks.jsonl --limit 5
DOMINION_PROVIDER_KEY=YOUR_API_KEY \
target/release/dominion-swebench \
  --engine target/release/dominion-rust \
  --tasks benchmark/verified-5.tasks.jsonl \
  --output benchmark/verified-run-001 \
  --provider-url https://api.example.com/v1 \
  --provider-kind openai \
  --provider-key-env DOMINION_PROVIDER_KEY \
  --model MY_MODEL_NAME \
  --limit 5 \
  --timeout-seconds 900 \
  --token-budget 150000 \
  --iterations 16 \
  --input-cost-krw-million 3000 \
  --output-cost-krw-million 12000 \
  --evaluate
```

`--input-cost-krw-million`과 `--output-cost-krw-million`은
**예시 요금**입니다. 실제 서비스 제공자의 모델 요금(원화/100만 토큰)을
사용자가 입력해야 합니다. 입력하지 않으면 추정 비용은 `null`이며
요금을 임의 추정하지 않습니다. 모델 공급자 주소도 실제 사용자의 주소로
변경해야 합니다. 로컬 Ollama 등 키가 없는 경우
`--provider-key-env`를 생략할 수 있습니다.

`--evaluate`가 없으면 공식 평가는 실행하지 않습니다.
이때 `predictions.jsonl`은 아직 정답 판정을 받지 않은 예측입니다.
공식 Python 평가기는 새 패치별 고유한 `run_id`로 실행하므로
이전 결과 캐시를 새로운 패치 성공으로 잘못 집계하지 않습니다.
평가 로그만으로 해결률을 주장해서는 안 되고,
공식 harness의 resolved/unresolved 결과를 확인해야 합니다.

## 검증·지표

- `manifest.json`: 입력 파일 SHA-256, 모델, 예산, 작업별 상태,
  수정 패치 SHA-256, 실제 Rust 도구 호출 수, 실행 시간,
  `/api/sessions/:id/usage`의 감사 로그 기반 토큰 사용량
- `predictions.jsonl`: SWE-bench 공식 JSONL 형식
- `progress.json`: 각 작업 직후 저장되는 중간 기록
- `<instance_id>/engine.log`: 해당 독립 Rust 서버 실행 기록
- `official-evaluation.log`: 선택적으로 실행한 공식 Docker 평가 로그
- `official-evaluation-run.json`: 공식 평가 결과 검증 완료 표시
- `official-evaluation-summary.json`: 실제 공식 `results.json`의 해결 ID,
  제출 부분집합 해결률, 도구 호출·토큰·원화 비용 측정 범위

동일 조건 비교는 **동일 인스턴스 ID, 동일 base_commit,
동일 제한 시간/토큰 예산, 독립 체크아웃, 동일 평가기**에서 수행하세요.
모델 자체 품질, 하네스 효과, 작업난도를 분리하려면 교차 배정과
여러 시드를 추가한 반복 실험이 필요합니다. 기존
`src/bin/dominion-benchmark.rs`의 AB/BA 실험과 구분해 분석하세요.

공식 평가기 실행이 끝나면 `logs/evaluation/<run_id>/results.json`의
`submitted_ids`가 제출한 작업 ID와 정확히 일치하는지 검사합니다.
일치하지 않으면 성공률을 기록하지 않습니다. 작업 수가 500개 미만이면
부분집합 해결률로만 표시하며 전체 Verified 점수로 발표하지 않습니다.

이 기능은 SWE-bench Verified에 대한 정식 실행 경로를 제공합니다.
**실제 500문제 실행·공식 평가 결과는 아직 없습니다.**
