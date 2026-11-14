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

## 4. 다중 에이전트와 모델 호환성

- `chat/runtime/tool_catalog.rs::select_room_tools`는 그룹의 코드 구현 요청에서 `workspace_read_range`, `workspace_replace_range`, `shell_exec` 대표 도구를 복원한다. 8개 스키마 예산으로 인해 편집·검증 기능이 실수로 사라지는 현상을 막는다.
- `chat/protocol.rs`는 Qwen 계열의 `✿FUNCTION✿` / `✿ARGS✿` 레거시 텍스트 호출을 파싱한다. 광고된 도구만 실행하고, 인자는 JSON object만 허용한다. Native ToolCall·DSML·기존 호환 경로를 보존한다.
- 영상·그룹·목표·RSI/워크플로 등의 기존 제품 경로는 제거하지 않는다. `video/quality.rs`의 FFprobe 검수는 기술적 검사이며 영상의 내용·화질·배우 동일성을 입증하지는 않는다.

## 5. 실제 확인과 미확인 구분

**코드로 추가한 회귀 확인**:
- 사용자 스티어링 SQLite 원자성·타실행 ID 오용 거부·종료 후 지시 보존
- SDK의 user instruction 직렬화/정확한 세션 주소와 효과 미확정 작업 승인 통제
- 모델 없는 matched outcome-aware vs recency 도구 라우팅 비교
- 그룹 코드 구현의 읽기·수정·검증 도구 필수 보존
- `execution.budget` 정책을 외부 실행 기한에 실제 적용
- 구현 명령과 설계 설명의 분리, 코드 무변경 상태에서 성공한 빌드만으로 가짜 완료 금지

**심사 기준으로 남는 실증**: 실제 여러 모델의 장기 소스 패치 완수율, 독립 비밀 테스트, 비용/캐시 hit에 따른 원화 청구액, 유료 멀티샷 영상의 시각적/시간적 품질, RSI 새 세대 채택 및 후손 재실행의 검증 성공, 운영 장애 복구. 이러한 기록이 없으면 프론티어 성능이 *이미 측정됐다*고 주장하지 않는다.

## 6. 2026년 공개 연구 근거 (알고리즘/평가 설계 참고)

- Google Developers Blog, **The Anatomy of Harness Engineering**, 2026-09-09: 최종 점수만이 아니라 행동별 회귀 평가. https://developers.googleblog.com/en/the-anatomy-of-harness-engineering-how-to-evaluate-iterate-and-guard-ai-coding-agents/
- **An Empirical Study of Harness Design for Coding Agents**, 2026-09-17: 계획·도구 공간·컨텍스트의 독립 실험. https://arxiv.org/abs/2609.20804
- **Agentic Harness Engineering**, 2026-04-28: 코드 구성·궤적 증거·의사결정 예측의 관측 가능성. https://arxiv.org/abs/2604.25850
- **HarnessFix**, 2026-06-04: 실행 궤적을 원인 컴포넌트와 연결해 국소 수정·검증. https://arxiv.org/abs/2606.06324
- **Rethinking Verification in Agent Harnesses**, 2026-10-25: 사전 지시/실행 중 리마인더/완료 게이트/독립 검토를 다른 비용-품질 조건에서 실증 비교. https://arxiv.org/abs/2610.34577

이 문서의 연구 수치는 DOMINION 자체 실측 수치가 아니다.
