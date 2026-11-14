# DOMINION 자체 Rust 엔진

2026-11-15 공식 벤치마크 비교기: scripts/swebench-official-compare.mjs
는 실행기 manifest.json / records.jsonl / predictions.jsonl 과 Docker
logs/evaluation/RUN-ID/run.json 및 각 report.json 을 대조합니다.
코드 수정 실제 patch SHA256, 작업 집합, Git 베이스 SHA, 제공자 모델,
토큰·시간 예산, 도구 승인 및 격리 상태를 확인하고 paired 승패를 계산합니다.
미채점 비공백 패치는 채점에 포함하지 않고 오류로 반환하며, 측정되지
않은 토큰과 제공자 청구액은 추측하지 않습니다.


## 2026-11-15 공동 목표 원가 상한

`tools/gateway.rs::delegate`가 부여한 `parentSessionId`·`delegated`
관계를 `Store::session_tree_usage`의 재귀 SQLite CTE로 읽습니다.
`goal_control.rs::GoalControl::complete`는 계획·검토·재계획에,
`chat/runtime/turn.rs::goal_token_budget_error`는 모든 부모/위임
모델 라운드와 하네스 시작 전 공통 상한을 적용합니다. 검사 실패 시
추가 호출을 막고, 미리 시작된 병렬 요청의 발생 비용은 소급 취소할
수 없습니다. `goal_metrics.rs::final_report`는 전체 위임 토큰과
실제 보고된 캐시 토큰을 사용하며 사용자 청구 금액은 추정하지 않습니다.
별도 SQLite fixture에서 부모·자식·손자·독립 세션, 캐시, 실패 및
상한 0(무제한)을 확인합니다.


## 2026-11-15 RSI 예측의 관측 분리

`rsi/meta_controller/provenance.rs::paired_outcome_observed`는 실제
독립 평가 verdict·eligible, 최소 8개 동결 사례 및 원본 평가 증거
SHA-256이 있거나 성공한 독립 채택 영수증을
지닌 세대만 채택 확률 추정의 표본으로 인정합니다.
`rsi/meta_controller/selection.rs::synthesize`는 확인된 paired
표본만 Beta(1,1) 사전분포와 합산하고, 빌드 실패/소스 후보 없음처럼
평가되지 못한 세대는 계속 실행 가능성 패널티·탐색 비용에만 반영합니다.
`pairedAssessedTrials`, `unassessedAttempts`를 후손이 보는 예측에
제공합니다. 동결 A/B 검증·채택 규칙은 바꾸지 않습니다.


## 2026-11-15 평가 데이터 경계

`scripts/swebench-dominion.mjs::runCase` →
`scripts/lib/swebench-source.mjs::prepareSealedSource` → 일시 Git
worktree → 독립 Git init / root commit → tree SHA·reference·object
alternates 확인 → 일시 worktree 제거 → 네이티브 Rust GoalRunner →
goal별 작업 worktree → 독립 sealed SHA 기준 실제 패치 추출.
공식 평가용 정답 패치와 미래 원본 Git 객체를 실행기에서 접근할 수
없게 합니다. 외부 네트워크/캐시는 격리 VM에서 별도로 제한합니다.


## 2026-11-15 네이티브 RSI: 후손 자기개선 경험과 공개 SDK

실제 실행 경로: rsi_meta_controller::synthesize →
rsi/meta_controller/trajectory.rs::source_matched_alternatives →
rsi/meta_controller/selection.rs → rsi/meta_controller/feedback.rs::instruction →
후손 Rust 워커/도구 실행 → 분리 worktree → 동결 paired 평가 →
승인된 후손 커밋 → 재컴파일.

동일한 원본 Git 커밋에서 동일한 모델 작성 역할과 경로를 바꾼
후속 변경만 경험 대안으로 선택하며, acceptedSourceHashes와
pairedRevalidated, eligible 평가 결과를 요구합니다.
후손에게 최대 3개의 검증된 대안을 전달하고,
독립적인 인과효과 증명과 원래 동결 평가 기준은 변경하지 않습니다.
새 모듈은 자기개선 가능한 실제 소스 대상으로 인벤토리에 연결됩니다.
모델 가중치 학습/LoRA는 이 경로에 없습니다.

Rust SDK와 TypeScript SDK는 모두 네이티브
POST /api/rsi/start (mode continuous, confirmation START_RSI_EVOLUTION)
→ GET /api/rsi/campaigns/:id → 명시적
POST /api/rsi/campaigns/:id/apply 계약을 제공합니다.
예산 제한은 요청 최상위 maxAttempts, tokenBudget,
timeBudgetSeconds이고 src/engine/rsi_evolve.rs::limits에서 검증합니다.
읽기 전용 모니터링은 기존 실행을 재시작하지 않습니다.


실행 경로는 `src/engine`의 Rust 모듈입니다. OpenClaw/Hermes 런타임, 봇 프레임워크,
외부 메모리 서비스, 해당 프로젝트의 소스 코드를 포함하거나 실행하지 않습니다.
모델은 OpenAI 호환 HTTP API로 호출하고, Discord는 공식 REST v10으로 연결합니다.
이번 변경은 Cargo 의존성을 추가하지 않습니다. `web/`의 Node 도구는 UI 빌드에 사용합니다.

## 기억과 학습

`memory.rs`가 기억의 소유권과 검색을 결정하고, `learning.rs`가 검증된 원문을 저장합니다.

1. 턴 완료 시 메시지 마지막 ID·에이전트·제공자·모델을 고정하여 학습 작업을 저장합니다.
2. 에이전트별 학습 잠금과 체크포인트를 확인합니다. 이미 처리한 범위는 모델을 호출하지 않습니다.
3. 새 메시지를 최대 30개씩 처리하며, 모델에는 JSON 추출만 요청합니다.
4. 후보가 가리키는 원문 메시지 ID와 정확한 인용을 검증합니다. 사용자 발언 또는
   성공한 실행 기록과 연결된 도구 결과만 허용합니다. assistant 발언·실패한 명령·봇 발언은 배제합니다.
5. 저장하는 기억의 본문은 검증한 원문 인용입니다. 모델이 만든 사실 요약으로 대체하지 않습니다.
6. 동일한 정정 키에 새 근거가 생기면 이전 기록은 `superseded`로 보존합니다.
7. 저장을 마친 뒤 체크포인트를 갱신합니다. 잘못된 JSON·저장 실패는 성공 처리하지 않습니다.

자동 기억과 모델 도구가 저장하는 기억에는 엔진이 `agentId`, `sessionId`, `partitionKey`를 붙입니다.
모델이 `scope: global`을 요청해도 경계를 넓힐 수 없습니다. Discord 경계는 연결·서버·채널을
포함합니다. 로컬의 기존 수동 공유 기억은 로컬에서 유지하지만 채널에 자동 공개하지 않습니다.
범위 정보가 없는 과거 자동 기억은 원래 세션에서만 읽습니다.

검색은 자체 Rust 문자 정규화·단어·한국어 2글자 조합·관련성 점수를 사용합니다.
벡터 임베딩이나 외부 검색 서버를 요구하지 않습니다. 의미가 전혀 다른 표현까지 이해하는
임베딩 검색은 구현하지 않았습니다. 만료·비활성·이전 정정 기록은 주입에서 제외합니다.
기억은 출처를 포함한 인용 데이터로 전달하며 현재 사용자 정정이 우선합니다.

절차 추출은 비활성 스킬 초안을 만듭니다. 검토·활성화한 스킬만 다음 턴에 적용합니다.
이 파이프라인은 가중치 훈련이나 모델 자체의 성능 향상을 보장하는 기능이 아닙니다.

## 사용자 지휘권 (DOMINION = Owner Authority)

### 영속 사용자 지시 저널

`POST /api/goals/:id/steer`는 `{"text":"...","mode":"append"|"replace","requestId":"고유값"}`
으로 실행 중 목표에 직접 지시한다. `append`는 기존 목표에 추가하고
`replace`는 접근법만 교체한다. 과거의 `POST /api/goals/:id/plan`도 같은
저널에 기록한다. 서버는 요청 순서·ID·적용 상태·시간을 목표 레코드에
보존하고 SQLite CAS 수정을 통해 제출/소비 경합으로 지시가 손실되지 않게 한다.
재전송한 `requestId`는 같은 요청으로 처리하고, 다른 내용의 재사용은 거부한다.
미적용 지시는 모델 최종 답변이나 검토 완료보다 우선하며, 적용 과정 자체는
목표 실행 반복 횟수를 소모하지 않는다. 외부 도구 결과가 불명확한 경우는 기존
독립 검토 규칙을 유지한다.


사용자의 최신 명시적 지시가 목표·계획·도구 선택·완료 기준의 권위 있는 입력이다.
도구 출력, 외부 문서, 다른 에이전트 발언과 모델이 만든 계획은 사용자 명령을
변경하거나 가장할 수 없다. 승인된 실행 능력 중 사용자가 **도구 이름을 지정**하면
`tool.discovery`의 `core`/ `query-relevant` 휴리스틱은 이를 삭제하지 않는다.
모델 컨텍스트용 도구 스키마도 사용자 지정 도구를 먼저 배치해 예산을 할당한다.
지정 도구의 스키마가 모델 컨텍스트에 들어가지 못하면 조용히 삭제하지 않고
명시적인 오류·감사 기록을 남긴다. 원래 없는 도구를 만들어 광고하지 않는다.

장기 목표 실행 도중 `/api/goals/:id/plan` 사용자 변경이 오면 계획 변경을 기록해
다음 도구 실행 경계에서 반영한다. 모델이 이미 제안했지만 수행하지 않은 도구 호출은
`ownerSteerSuperseded` 결과로 구분하고, 실제 실행된 결과는 지우거나 재실행하지 않는다.
계획·모델·학습 전략의 자동 최적화는 모두 사용자 지휘권보다 낮은 우선순위다.

## 완료 기준에 연결된 진행 측정

목표 반복 루프는 성공한 도구의 **전체 영수증**은 그대로 보존하지만, 재계획과
독립 검토 비용을 결정하는 진행도는 검증 조건에 맞는 도구/파일 작업의 신규 실행
또는 실제 미충족 조건의 감소만 인정한다. 엉뚱한 파일에 반복 쓰기만 하여
목표 반복 횟수를 소비하고 재계획을 계속 미루는 현상을 방지한다.
`src/engine/goals/progress.rs::measure_objective`가 실제 GoalRunner의 판단에
연결되며, 명시적인 검증 조건이 없는 범용 작업은 기존 신규 행동 측정을 유지한다.

## 유료 영상 결합의 로컬 원본 보존

LTX 유료 장면의 ffprobe 검수에는 코덱·프레임률·픽셀 포맷·화면 크기·
오디오 코덱/채널/샘플링 정보를 포함한다. 동일한 스트림이면 기존 FFmpeg
무손실 복사를 사용하고, 다른 스트림이나 검증 불가능한 파일은
**유료 원본을 바꾸지 않은 채** FFmpeg 로컬 복제본으로 프레임률·
화면 크기·H.264/AAC를 맞춘다. 전송 실패 시에도 재호출 대신
로컬 정규화를 1회 시도하고, 결과에는 `concatMode`,
`localNormalizedClips`, `newPaidCallsForNormalization=0`을 기록한다.
이는 기술적 결합 기능이지 영화적 연속성의 주관적 품질 보증이 아니다.

## 영상 멀티샷 연속성

영상 작업 등록 시 사용자 입력 `filmAnchor`(인물·의상·배경 기준)을
모든 유료 장면 프롬프트에 고정한다. 장면별 직접 작성한 내용과도 합성하며,
각 장면에서 동일한 연속성 기준 해시를 남긴다.
이는 생성 지시 일관성에 대한 기능이며 픽셀 수준의 실제 연속성 검증과 구분한다.
유료 호출 횟수 견적·확정 승인·완료 장면 재사용은 그대로 작동한다.

## 코딩 작업의 재사용 가능한 소스 지도

일반적인 자율 코딩 목표도 RSI에서 검증된 콘텐츠 해시 기반 코드 인덱스를
재사용한다. 실행 전 비동기 블로킹 풀에서 `src/engine/code_index.rs`를
증분 갱신하고 실제 파일명·심볼 위치·대상 후보의 제한된 지도를
계획 담당과 실행 담당 모델에 전달한다. 프로젝트 트리 전체를 매 모델 턴에
재전송하지 않고 인덱스 갱신 실패는 없는 출처 지도로 위장하지 않는다.
일반 대화와 문서 작성 작업에는 인덱스를 강제로 만들지 않는다.

## RSI: 실패 경험 기반 수정과 holdout 독립성

`rsi::focus::generation_context`에서 평가 자료를 재사용할 때,
동결 평가 보고서에 holdout 표지가 없다는 점을 고려해 **후보에 동결된
평가 계획(`rsi_candidate.cases`)의 ID와 교차 검증**한다. 실제 모델에
노출하는 구체적인 실패 사례는 holdout=false인 학습 사례뿐이다.
동일 저장소 파티션·동일 사용자 목표·동일 생성기 커밋에서 평가됐으나
채택되지 않은 후보는 최대 3개의 실패 교훈으로 계승한다.
각 교훈은 실측된 paired 영수증과 연결되며 숨겨진 정답이나 raw holdout
입력을 프롬프트에 주입하지 않는다. 기존 독립 paired 평가와 채택 조건은
변경하지 않는다.

## RSI: Pareto + 다양성 기반 탐색

`src/engine/rsi_archive.rs`는 동결된 동일 A/B 평가와 영수증을 통과한
중립 후보만 아카이브에 보존한다. 비지배 Pareto 후보 중 다음 검색
부모를 고를 때 **실제 소스 하위 시스템 / 변경 범위 / 패치 크기**
셀별 후손 실행 횟수를 먼저 비교하고, 같은 셀에서는 개별 부모의
후손 실행 횟수를 사용한다. 성능 검증을 다양성으로 대체하지 않으며
`featureCell`, `frontierCells` 등 탐색 선택 근거를 결과에 기록한다.
OpenEvolve·EvoAgentX MAP-Elites의 탐색 다양성을 Rust 자체 엔진에 적용한
경량 기능이며, 동결 평가 결과가 확인된 후보의 부모 선택에만 적용된다.

## RSI: 후손 소스 버전에 일치하는 학습 기록

`src/engine/rsi/execution.rs`는 모델 프롬프트의 경험 부분을 부모 입력이 아닌
후손의 `child_input`으로 구성한다. `rsiRepairLessonReceipt`에
후손 실제 커밋, 유효한 이전 실패 개수, 컨텍스트 예산 적용 후 실제 전달된
교훈 개수와 출처 세대 ID를 기록하고 `rsi_generation.repairLessonReceipt`로 계승한다.
예산 때문에 경험이 제외되었다면 전달된 것처럼 기록하지 않는다.

## 비가중치 RSI 검증 계보의 파레토 학습 (2026)

`src/engine/rsi/focus.rs::verified_reflection_frontier`는 기존 RSI의 검증된
세대 결과를 성공 주장과 분리한다. 독립 쌍대 평가 `eligible=true`,
`verdict=improved`, `pairedRevalidated=true`, `accepted=true`,
회귀 0건, 실제 개선된 홀드아웃 1건 이상인 세대만 사용한다.

영역마다 **홀드아웃 개선 수 최대화 × 관측 토큰 비용 최소화**의
비지배 계보를 유지하고, 최대 6개를 다음 후손 모델의
`generation_context`로 전달한다. 비용이 없는 관측은 0원/0토큰으로
취급하지 않으며, 동결 홀드아웃 사례·정답·실행 흔적은 전달하지 않는다.
따라서 하나의 최신 성공 계보만 답습하지 않고 검증된 다른 장점을
실제 소스 변경 가설에 활용할 수 있다. 이는 결과 기억/조건부 제안이며
새 세대의 채택·성공은 독립 평가를 다시 통과해야만 인정한다.

참조한 실제 구현: `gepa-ai/gepa/src/gepa/strategies/candidate_selector.py`
(파레토 후보 선택), `gepa-ai/gepa/src/gepa/proposer/reflective_mutation/reflection_lm.py`
(평가 피드백으로 후보 갱신), `facebookresearch/HyperAgents/generate_loop.py`
(후손 평가·계보 개선). 2026년 Meta-Harness의 과거 후보 실행 흔적 보존,
RSIAgent의 광범위 탐색→심층 검증도 추가 개발 방향이다. 이번 변경이
이들 프로젝트의 전체 알고리즘을 복제했다는 뜻은 아니다.

## 병렬 도구 실행 예산

`execution.ordering`의 읽기 우선과 `tool.combination=parallel-reads`는
모델이 제안한 서로 독립적인 읽기 작업의 순서·실행 시간을 최적화한다.
병렬화를 선택해도 도구 예산과 제안 예산을 미리 **원자적으로 예약**한다.
부족하면 기존 순차 실행을 사용하고 `agent.parallel_reads_serial_fallback`
영수증에 사유를 남긴다. 외부 변경을 일으키는 도구는 병렬 읽기 묶음에
포함되지 않으며, 성공하지 않은 묶음은 학습 신호에서도 성공으로 세지 않는다.

## 목표 하네스

모델은 계획과 검토를 제안하고, Rust가 완료 상태를 결정합니다.
`POST /api/goals`에서 명시적인 `successCriteria`를 지정할 수 있습니다.

```json
{
  "goal": "report.md 파일을 작성하고 결과를 확인해라",
  "maxIterations": 5,
  "executor": {"agentId": "my-agent"},
  "successCriteria": [
    {"type": "file_changed", "path": "report.md"},
    {"type": "file_contains", "path": "report.md", "contains": "검증 결과"}
  ]
}
```

| type | 필수 값 | Rust가 확인하는 것 |
| --- | --- | --- |
| `file_exists` | `path` | 경로 존재 |
| `file_absent` | `path` | 경로 부재 |
| `file_changed` | `path` | 실행 시작 전 상태와 비교한 변경 |
| `file_contains` | `path`, `contains` | 파일의 실제 내용 |
| `tool_succeeded` | `tool` | 해당 이름의 실제 성공 결과 |
| `command_succeeded` | `command` | 동일 명령의 종료 코드 0 |

상대 경로는 워크스페이스 안으로 제한하고 심볼릭 링크 이탈을 거부합니다.
절대 경로는 `file_*` 도구와 같은 명시적 컴퓨터 경로입니다.
조건이 없으면 목표 문장에서 파일 경로와 변경·삭제 의도를 보수적으로 추론합니다.
셸 명령의 임의 의미를 완전히 판정할 수는 없으므로 복잡한 작업에는 명시적 조건을 사용하세요.
`tool_succeeded`는 그 도구의 성공만 확인하며 더 넓은 업무 결과를 보장하지 않습니다.

실행 후 검토 모델에 실제 도구 인수·결과와 Rust 검증의 미충족 조건을 전달합니다.
검토 JSON이 올바르고, 완료 판정이고, 미충족 조건도 없어야 `completed`가 됩니다.
미완료는 수정 턴으로 이어지고 한도에 도달하면 `needs_review`입니다.
실패·취소·검토 필요 상태는 부모 작업에도 그대로 반영합니다.

## 실행 기록·복구·취소

SQLite에 실행·사용자 메시지·도구 호출 시작을 저장하지 못하면 모델/도구 실행을 진행하지 않습니다.
각 도구 호출은 실행 ID·라운드·호출 위치를 가진 독립 기록으로 인수·결과·상태를 보존합니다.
셸의 비정상 종료·HTTP 오류·MCP `isError`는 성공 증거가 아닙니다.

작업은 원자적으로 선점합니다. 기본 워커는 4개이며 `DOMINION_JOB_WORKERS`로 1~16개를
설정할 수 있습니다. 동일 세션의 동시 턴은 차단합니다. 에이전트별 모델·지시·기억을 적용하고,
공유 회의 기록에서 다른 화자의 발언을 자신의 assistant 메시지로 전달하지 않습니다.

목표·워크플로·회의·프롬프트·Discord는 부수 효과 전에 체크포인트를 기록합니다.
중단 지점의 실행/전송 여부가 불확실하면 자동 반복하지 않고 `needs_review`로 남깁니다.
완료한 회의 발언과 워크플로 단계는 저장된 커서로 건너뜁니다. 검토가 끝나지 않은 목표도
확인 없이 다음 실행 턴을 시작하지 않습니다. 종료 상태의 목표를 재시작으로 다시 열지 않습니다.

취소는 DB에도 저장됩니다. 해당 세션의 대기 작업과 실행 중 작업을 취소하고,
새 UI 대화가 시작되어도 이전 작업의 취소를 되돌리지 않습니다.
응답 대기·스트리밍·승인 대기에서 취소를 감지합니다. 이미 완료된 외부 변경을 되돌리지는 않습니다.

## 주인이 참여하는 그룹 대화

방의 대화는 하나의 공용 세션에 저장됩니다. 실제 주인 메시지는 `metadata.authorKind: owner`,
에이전트 응답은 각 `agent_id`와 원문을 가리키는 `replyToMessageId`, 실행 ID를 갖습니다.
사용자 메시지와 응답 작업을 한 SQLite 트랜잭션으로 저장하고 클라이언트 재전송은 중복 제거합니다.
에이전트에게 차례를 줄 때는 기존 공용 기록을 읽히며 인공 사용자 턴을 만들지 않습니다.

- `discussion`: 참가자 모두의 첫 발언을 보장한 뒤 서로의 멘션에 따라 대화를 이어갑니다.
- `round_robin`: 사용자 메시지마다 다음 참가자가 응답합니다.
- `mentioned`: 주인이 선택하거나 `@이름`·`@ID`로 지정한 참가자가 시작하고 동료에게 이어갈 수 있습니다.

라운드·최대 발언 수로 자동 대화를 제한합니다. 새 주인 메시지가 들어오면 현재 발언을 마친
경계에서 이전 대화를 양보하고 새 작업을 처리합니다. 같은 방의 작업은 직렬로 수행합니다.
중지는 영속적인 취소 세대를 올립니다. 이후 새 메시지가 방을 재개해도 과거 작업이 살아나지 않습니다.
판단의 기준은 실제 주인 원문과 실행 결과이며 동료의 주장은 별도 인용 데이터입니다.

기억은 방·에이전트별로 격리합니다. 일반 개인 기억을 방에 공유하려면 명시적인 공유가 필요합니다.
과거에 생성된 가짜 사용자 프롬프트나 동료 발언을 주인 사실로 학습하지 않습니다.
모든 방의 현재 발언·도구 작업·승인·학습은 실제 서버 기록으로 표시합니다.
스트리밍은 턴 ID로 구분하고 최종 저장된 메시지 ID와 연결합니다.

브라우저는 작업 실행기가 아닙니다. Rust 워커가 큐를 소비하고 모델·도구·기억을 처리하므로
대화 탭을 닫아도 자동화는 계속됩니다. GUI 없는 실행은 다음과 같습니다.

```bash
DOMINION_HOME=/path/to/dominion-rust DOMINION_JOB_WORKERS=4 \
  /path/to/dominion-rust/target/release/dominion-rust
```

도구 허용과 기존 승인 정책은 그대로 적용됩니다. 모델이 임의로 실행 권한을 넓히지 못합니다.
현재 서버 바인딩은 로컬 루프백입니다.

## 페르소나·아바타 가져오기

`GET /api/personas/import/preview`에서 실제 파일을 미리 보고,
`POST /api/personas/import`에 `{"apply":true}`를 보내 적용합니다.
대상은 사용자의 `~/.hermes/profiles/` 안 9개 명시된 프로필이며 SOUL·아바타만 읽습니다.
원문은 `personaOriginal`로 보존하고 프로필에는 DOMINION 실행 경계를 함께 넣습니다.
기존 사용자 수정은 기본적으로 보존하고 명시적으로 갱신하더라도 모델·제공자는 유지합니다.
이미지는 `runtime/avatars`에 복사하며 안전한 네이티브 아바타 경로로만 제공합니다.
실제 봇 사진이 별도 이미지 캐시에 있다면 `avatarSources`에 프로필별 파일명을 지정할 수 있습니다.
허용된 로컬 이미지 폴더에서만 읽고, 선택한 원본과 이미지 URL 버전을 보존합니다.
이식된 개인 데이터와 이미지 자체는 저장소에 포함하지 않습니다.

## Discord 설정과 지원 범위

1. Discord Developer Portal의 Bot Token을 준비합니다. Message Content Intent를 활성화하고
   봇에 View Channel·Read Message History·Send Messages 권한을 부여합니다.
2. Discord 개발자 모드에서 서버 ID와 채널/스레드 ID를 복사합니다.
3. DOMINION의 연결 화면에서 Discord를 선택하고 토큰·ID·에이전트 또는 최대 9명 회의실을 저장합니다.
4. 활성화 시점 이후의 새 메시지를 약 5초 간격으로 수신합니다. 이전 대화에 갑자기 답하지 않습니다.

API로 저장할 수도 있습니다. 실제 토큰을 로그나 문서에 남기지 마세요.

```json
{
  "kind": "discord",
  "name": "내 Discord 방",
  "token": "YOUR_BOT_TOKEN",
  "guildId": "YOUR_SERVER_ID",
  "channelId": "YOUR_CHANNEL_ID",
  "agentId": "YOUR_AGENT_ID",
  "enabled": true,
  "mentionOnly": true,
  "allowedUserIds": "USER_ID_1,USER_ID_2",
  "allowedBotIds": "EXTERNAL_BOT_ID",
  "toolsEnabled": false
}
```

이 객체를 `POST /api/channels`에 보냅니다. `roomId`를 사용하면 해당 방의 에이전트들이
독립 세션으로 발언하며 한 Discord 봇 계정에서 이름을 붙여 전송합니다.
`roomPolicy`는 `all_or_named`, `named_only`, `round_robin`을 지원합니다.
기존 9개 Discord 봇을 각각 운용하려면 봇별 토큰으로 연결을 별도 등록합니다.
기존 OpenClaw 프로세스와 동일 토큰으로 동시에 응답하지 않도록 전환해야 합니다.
이 변경이 기존 봇 설정을 가져오거나 자동으로 중지하지는 않습니다.

외부 봇은 `allowedBotIds`와 DOMINION 봇 멘션이 모두 있어야 응답하며 연결별 분당 2회입니다.
외부 봇이 멘션을 요구하면 `replyMentionBots: true`를 선택합니다. 허용된 발신 봇만
첫 응답에서 엔진이 멘션하며, 모델이 만든 다른 멘션은 차단합니다. 기본값은 꺼짐입니다.
봇의 도구 실행·사용자 기억 학습은 차단합니다. 사람의 도구 실행은 `toolsEnabled: true`와
명시적인 `allowedUserIds`가 모두 있어야 합니다. 일반 도구 승인 정책도 적용됩니다.
허용한 대상 외의 출력 멘션을 막고, 참가자당 메시지는 최대 4개 조각으로 제한합니다.

토큰은 vault에서 암호화하며 목록·개별 조회·이벤트에 포함하지 않습니다.
수신 위치·메시지 ID·발언 생성·전송 상태를 저장하고, 불확실한 전송을 자동 재시도하지 않습니다.
REST 제한은 토큰별로 처리하며 최대 9개 연결을 병렬 조회합니다.
서버 텍스트/공지 채널·명시적으로 지정한 스레드를 지원합니다.
DM·음성·파일 첨부·슬래시 명령·Gateway 실시간 이벤트는 이번 범위에 포함하지 않습니다.

검증은 모의 모델과 모의 Discord HTTP 서버로 수행합니다. 사용자의 실제 봇에 접속하거나
실제 Discord 메시지를 보내는 검증은 수행하지 않았습니다.

2026-11-04 검증 결과:

- `cargo test --release`: 단위 테스트 99개 + E2E 8개 통과
- `tsc --noEmit`: 통과
- `npm run build`: 통과
- `git diff --check`: 통과

## 구조 참고 자료

다음 공식 문서는 기억 계층·프로필 격리·라우팅·프로토콜을 이해하기 위해 참고했습니다.
프로젝트 코드나 엔진을 가져오지 않았습니다.

- [OpenClaw 기억 구조](https://docs.openclaw.ai/concepts/memory-architecture)
- [OpenClaw 에이전트 라우팅](https://docs.openclaw.ai/concepts/multi-agent)
- [Hermes 지속 기억](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory)
- [Hermes 스킬](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills)
- [Discord 메시지 API](https://docs.discord.com/developers/resources/message)
- [Discord 요청 제한](https://docs.discord.com/developers/topics/rate-limits)
