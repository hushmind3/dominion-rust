# DOMINION sub3 — 전체 제품 프론티어 통합 경로 (2026-11-14)

이 문서는 **실제 제품 실행 경로에 반영된 변경**과 **독립 검증이 아직 필요한 부분**을 분리한다. 목표는 사용자 중심의 범용 자율 에이전트이며, 모델 호출기나 타사 SDK 껍데기가 아니다. 모델 가중치 수정, LoRA, 파인튜닝은 RSI 경로에 없다.

## 1. 사용자 지배권: 일반 채팅에도 실행 중 지시 변경

- 화면: `web/src/pages/Chat.tsx`에서 처리 중 새 명령을 다시 전송한다. 기존 SSE 화면의 과거 잠정 출력은 `owner_steer_applied`에서 지운다.
- 외부 인터페이스: `POST /api/sessions/:id/steer` (`input: string`)와 1차 JS SDK `DominionClient.sessions.steer(id,input)`. 내부 API를 호출하지 않는 임의 외부 실행 엔진은 사용하지 않는다.
- 저장: `chat/owner_inbox.rs::enqueue`가 SQLite `messages`의 실제 owner 원문과 `owner_steer` 큐 레코드를 **한 트랜잭션**에 넣는다. 세션·실행 ID로 바인딩하고 유효하지 않은 주체의 지시 승격은 막는다.
- 적용: `runtime/turn.rs::absorb_owner_steering`이 다음 결정 경계에서 `claim` 후 이전 계획을 무효화하고 사용자의 새 메시지로 다음 모델 호출을 진행한다. 새 요청의 acceptance 원문과 도구 선택 쿼리도 교체하며 기존 성공 도구를 새 요청의 근거로 자동 승격하지 않는다.
- 구분: `queued`는 저장됐다는 뜻, `claimed`는 실행 루프가 수용했다는 뜻이다. 마지막 결정 경계 뒤 도착한 명령은 `deferred`로 기록해 수행한 것처럼 보이지 않게 한다.
- **범위:** 지금은 일반 개인 채팅의 모델 결정 경계만 지원한다. 오래 실행 중인 외부 셸/브라우저의 즉시 취소, 기존 자율 goal/RSI 세대의 중간 재계획은 별도 연결이다.

## 2. 알고리즘이 실제 도구 판단과 연결되는 경로

- `src/engine/algorithms.rs`의 `tool.routing` 알고리즘 종류: `outcome-aware` / `recency-biased`.
- `chat/runtime/turn.rs`가 실제 작업 내역(`execution_operation`)을 읽어 생산 라우터 `tool_route::select`에 입력한다.
- 기존 recency 가산점은 실패 도구도 `+40`으로 높였다. outcome-aware는 실제 성공·실패 기록을 구분해 재랭킹하되 **선택에서 제외된 도구라도 이후 라운드에 복원 가능**하다. 각 단계에서 실제 도구의 허용 목록/스키마/승인 조건이 우선한다.
- 실행한 알고리즘 버전과 선정 도구를 `tool.native_route` 감사 기록에 남기고 실제 도구 결과로 `algorithm_usage` 신호를 쌓는다. 메뉴만 표시한 경우는 성과로 채점하지 않는다.
- `algorithm_eval.rs::run` → `chat::evaluate_tool_routing` → 같은 production `tool_route::select`를 사용해 **고정 모델 호출 없이 동일한 실패·복구 사례의 선택 결과**를 비교한다. API: `/api/algorithms/evaluate`. 이는 5번째 차원이며 기존 기억, 중요도, 작업 분해, 그룹 충돌 평가를 그대로 보존한다. 시험 결과는 후보 추천이지 자동 채택이 아니다.

## 3. 실행 예산·행동 완수 판정

- `runtime.rs::run_turn_with_policy`가 `execution.budget` 알고리즘을 1회 결정하고 `turnBudgetAlgorithm`을 실행 기록에 동결한다.
- `runtime.rs::selected_turn_limit`은 알고리즘의 `maxElapsedSecs`를 바깥 Tokio timeout에 적용하고 `runtime/turn.rs`는 **같은 설정**을 내부 BudgetLedger 및 모델 선택 스냅샷에 사용한다. 과거에는 `extended=900초`를 골라도 바깥 고정 `600초`에 중단됐다. 기본값은 600초, 설정은 60~3600초 내로 제한된다. RSI 캠페인의 별도 소유 예산은 보존한다.
- `chat/evidence.rs::prepare_owner_action`은 명시적 구현·완성 명령을 실행 목표로 해석하되 `어떻게 구현할지`와 같은 설명·가정 질문은 작업 명령으로 승격하지 않는다.
- `verification::acceptance_gaps_in`은 소스 구현·리팩터 요청에 **실제 성공한 소스 변경 영수증**을 요구한다. `cargo test` 통과나 읽기만으로 새 코드를 만들었다고 판정하지 않는다. 검증 명령이 코드 변경보다 이전이면 재실행을 요구하는 기존 영수증 계약도 유지한다.

## 4. RSI 후손 실험의 신뢰성·재귀적 평가 (추가 구현)

2026년 공개된 중국 개발자 오픈소스 [AnyRSI](https://github.com/TraceLite-AI/openrsi)의 '채점 불가능한 평가 인프라를 작업 실패로 계산하지 않음'과 [Harness²](https://github.com/google-research/harness2)의 '동일 조건 행동 대비로 개선을 검토함'을 실제 코드까지 조사했다. 미국·중국 런타임의 정책/SDK를 가져오지 않고 **도미니언 Rust**의 기존 후손 독립 평가기만 보강했다. 재학습·LoRA·가중치 수정은 하지 않는다.

- `rsi/evaluation/trials.rs::measured_generation`은 64자리 평가 프로필·객관적 목표 해시, `pairedMeasured`, 독립 평가 verdict가 모두 있어야 비교 가능하다고 판정한다.
- `matched_trial_envelope`은 같은 원본 생성기 커밋·도메인 외에도 **동일 동결 평가 프로필 및 목표**를 요구한다. 단지 같은 도메인에서 쉬운 작업을 처리한 상대방을 성공 사례로 잘못 비교하지 않는다.
- 평가 실행 자체가 빠진 기록은 `unscorable`로 별도 감사하고, 후보의 패배나 향후 2회 독립 성공의 패배표로 합산하지 않는다. 실제 실패·회귀는 기존 `rejected`/비교 verdict로 계속 다룬다.
- 컴파일된 후손의 계획 단계에는 아직 새 세대의 평가 영수증이 없으므로 `available_trial_baseline`으로 과거 기준판이 측정됐는지만 확인한다. 새 세대 실행 후에야 `comparable_trial_baseline`의 전체 대조 계약을 적용한다. 계획 단계에 미래의 평가 영수증을 요구하면 시험 알고리즘을 영원히 실행할 수 없게 된다.
- `rsi/learning/generation_feedback.rs`와 `rsi/planner/descendant.rs`는 평가기 실패/측정 부족으로 남은 후보 ID를 다음 제어 실험에 계승한다. 다시 측정했다고 자동 채택하는 구조는 아니며, 실제 동결 비교가 2번 유효해야 활성화한다.
- 리그레션: `tests/support/engine/rsi_evolve.tests.rs`에 프로필·목표 불일치/미측정 실험 거부, 미측정 사례 이후의 정상 독립 시험 2회 채택 조건을 추가했다.
- 실모델 RSI Action 43607295439/43607411081은 `cargo build --locked`의 Cargo.lock 업데이트 요구로 **모델 실행 전에 실패**했다. `.github/workflows/sub3-live-model-cycle.yml`은 동일 일반 CI처럼 Cargo 잠금 재해석을 허용하고 해시와 변경 로그를 남기도록 수정했다. 이 변경을 모델 작성·후손 채택 성공 실적으로 세지 않는다.

### 실제 RSI 실행 진입 개선 (2026-11-15)

- Cargo.lock는 실모델 Linux 실행 로그에서 추출한 **Cargo resolver의 실제 19개 패치 구간**으로 동기화했다. `.github/workflows/sub3-live-model-cycle.yml`은 다시 `cargo build --release --locked`로 엔진을 빌드하고, 재귀 후손 실행 역시 잠금 파일을 검사한다.
- 실제 연결 실험 [Action #43609071999](https://github.com/hushmind3/dominion-rust/actions/runs/43609071999)은 **Rust 엔진 빌드·모델 연결·후손 내부 진입 성공**, `modelCalls=1`, `toolsUsed=[]`, `verifiedWrites=0`, `acceptedGenerations=0`이었다. 모델은 파일을 요청하는 글로 종료했고 코드를 수정하지 않았다. 이 실행을 RSI 성공 사례로 채점하지 않는다.
- `chat/runtime/turn.rs`의 새 `rsi.first_tool_recovery`는 모델의 도구 없는 중단을 검사한다. 이미 Rust 계획기가 선정하고 검증한 소스 경로에 대해 **실제 `workspace_read_range`를 같은 엔진 dispatch로 실행**하고, `execution_operation`의 영수증·SHA-256을 만든다. 검증 가능한 소스 관찰이 없으면 변경을 만들어내지 않는다. 다음 모델 단계에는 경계가 표시된 실제 소스 자료를 준다.
- 테스트는 `tests/support/engine/chat_runtime.tests.rs::native_rsi_source_recovery_is_an_actual_audited_read_not_fake_model_text` 및 기존 `rsi_no_tool_recovery_tests`로 수행 경로와 도구 원문/스코프 제한을 점검한다. **자율 후손 편집과 실모델 개선 채택은 다음 실험에서 확인할 성과**로 남는다.

## 5. 다중 에이전트와 모델 호환성

- `chat/runtime/tool_catalog.rs::select_room_tools`는 그룹의 코드 구현 요청에서 `workspace_read_range`, `workspace_replace_range`, `shell_exec` 대표 도구를 복원한다. 8개 스키마 예산으로 인해 편집·검증 기능이 실수로 사라지는 현상을 막는다.
- `chat/protocol.rs`는 Qwen 계열의 `✿FUNCTION✿` / `✿ARGS✿` 레거시 텍스트 호출을 파싱한다. 광고된 도구만 실행하고, 인자는 JSON object만 허용한다. Native ToolCall·DSML·기존 호환 경로를 보존한다.
- 영상·그룹·목표·RSI/워크플로 등의 기존 제품 경로는 제거하지 않는다. `video/quality.rs`의 FFprobe 검수는 기술적 검사이며 영상의 내용·화질·배우 동일성을 입증하지는 않는다.

## 6. 실제 SWE-bench 실행 및 측정

- `scripts/swebench-verified.mjs`는 실제 배포된 Rust 실행 파일을 별도 SQLite/워크트리에서 시작해 **실제 `/api/goals` 자율 계획·편집·재시도**를 수행한다. 공식 JSONL의 `problem_statement`·`repo`·`base_commit`만 전달하고 `test_patch`나 정답을 에이전트에 넣지 않는다.
- 사용자가 허용한 실제 모델 연결과 단가가 필요하며 비신뢰 코드의 셸 실행은 별도 VM/컨테이너가 필요하다. 결과는 Git `--intent-to-add`와 `git diff --binary`의 실제 변경 패치로 제출한다. 공식 Docker 재현 채점은 별도로 실행한다.
- `scripts/swebench-verified.test.mjs`은 **실제 Git 저장소·추가 파일·편집 결과**, 데이터 누출 방지, 영수증 기반 도구 오류 복구율, 사용자가 입력한 원화 토큰 단가 계산을 확인한다. 이 파일의 Node 5개 테스트는 [CI #43611352207](https://github.com/hushmind3/dominion-rust/actions/runs/43611352207) Ubuntu 단계에서 통과했다.
- 모델 호출/도구 수정/성공률이 계측되지 않은 문제를 0% 성공으로 꾸미거나, 캐시 결측을 임의 원가로 채우지 않는다.

## 7. 실제 확인과 미확인 구분

**코드로 추가한 회귀 확인**:
- 사용자 스티어링 SQLite 원자성·타실행 ID 오용 거부·종료 후 지시 보존
- SDK의 user instruction 직렬화/정확한 세션 주소와 효과 미확정 작업 승인 통제
- 모델 없는 matched outcome-aware vs recency 도구 라우팅 비교
- 그룹 코드 구현의 읽기·수정·검증 도구 필수 보존
- `execution.budget` 정책을 외부 실행 기한에 실제 적용
- 구현 명령과 설계 설명의 분리, 코드 무변경 상태에서 성공한 빌드만으로 가짜 완료 금지

**심사 기준으로 남는 실증**: 실제 여러 모델의 장기 소스 패치 완수율, 독립 비밀 테스트, 비용/캐시 hit에 따른 원화 청구액, 유료 멀티샷 영상의 시각적/시간적 품질, RSI 새 세대 채택 및 후손 재실행의 검증 성공, 운영 장애 복구. 이러한 기록이 없으면 프론티어 성능이 *이미 측정됐다*고 주장하지 않는다.

## 8. 2026년 공개 연구 근거 (알고리즘/평가 설계 참고)

- Google Developers Blog, **The Anatomy of Harness Engineering**, 2026-09-09: 최종 점수만이 아니라 행동별 회귀 평가. https://developers.googleblog.com/en/the-anatomy-of-harness-engineering-how-to-evaluate-iterate-and-guard-ai-coding-agents/
- **An Empirical Study of Harness Design for Coding Agents**, 2026-09-17: 계획·도구 공간·컨텍스트의 독립 실험. https://arxiv.org/abs/2609.20804
- **Agentic Harness Engineering**, 2026-04-28: 코드 구성·궤적 증거·의사결정 예측의 관측 가능성. https://arxiv.org/abs/2604.25850
- **HarnessFix**, 2026-06-04: 실행 궤적을 원인 컴포넌트와 연결해 국소 수정·검증. https://arxiv.org/abs/2606.06324
- **Rethinking Verification in Agent Harnesses**, 2026-10-25: 사전 지시/실행 중 리마인더/완료 게이트/독립 검토를 다른 비용-품질 조건에서 실증 비교. https://arxiv.org/abs/2610.34577

이 문서의 연구 수치는 DOMINION 자체 실측 수치가 아니다.
