# 서브4 2026-11-14 v3.0 · ComfyUI 멀티샷 · RSI 운영 피드백 · 병렬 조사

## 실제 진입점과 제품 기능

### 1. 영상 · 기술적 불량 샷만 재생성

- `web/src/pages/Video.tsx` → `POST /api/video/jobs` (`src/api/video.rs::create_job`) → `src/engine/video.rs::enqueue`에서 `cinematic`과 `badShotRetries`를 범위 검증하고 작업 입력에 동결한다.
- `src/engine/video.rs::run` → `src/engine/video/cinematic.rs::render`: 장면별 프롬프트·고정 시드·길이와 전체 visual continuity bible을 포함한 `storyboard.json`, `shot-XX/output.mp4`, `shots.ffconcat`, 최종 `output.mp4` 생성.
- `src/engine/video/comfy.rs::render`: 실제 ComfyUI HTTP API `/prompt` → `/history/{id}` → `/view`를 사용한다. 원격 요청 결과가 불명확한 경우 `needs_review`; 무작정 재요청하지 않는다. 원래 `video.rs`에서 독립 모듈로 분리했다.
- `src/engine/video/quality.rs::inspect_clip`: 각 클립에서 ffprobe 해상도·비율·길이를 확인한다. 검증된 기존 클립은 원격 API에 다시 제출하지 않는다. 기술 검수 실패한 클립에만 선택한 최대 1회 재시도를 허용한다. 최종 FFmpeg 출력은 1080p 형식으로 렌더링하고 별도 ffprobe를 통과해야 완료한다.
- 사용자에게 `shotResults`의 재사용 여부, 클립별 ffprobe 영수증, 재시도 횟수, 최종 파일 검증 결과를 노출한다.
- **품질/원가의 증거 경계:** 시각 미학·인물 일관성의 자동 평가가 아닌 파일 기술 검증이다. GPU 전력·API 청구액이 측정되지 않았다면 `비용 절감률`로 보고하지 않는다. 재생성 수와 재사용 수는 원가 최적화에 대한 운영 관측 지표이다. 8GB VRAM 사용자에게 로컬 고사양 영상생성을 강제하지 않고 기존 native 카드 및 유료 API 선택지를 유지한다.
- 2026 자료: [LTX 공식 two-stage](https://docs.ltx.io/open-source-model/usage-guides/two-stage-generation), [LTX 공식 RetakePipeline](https://github.com/Lightricks/LTX-2/blob/main/packages/ltx-pipelines/docs/pipelines.md). 현재 구현은 ComfyUI의 장면 단위 분리 및 재시도이며, LTX RetakePipeline의 프레임 시간 구간 수정 구현은 아니다.

### 2. 목표 수행 → RSI 조사 학습 피드백

- `src/engine/goals.rs::finish`가 결정론적 `finalReport`와 실제 종료 상태를 동결한 뒤 `src/engine/goal_feedback.rs::record`에 `goal_receipt`를 생성한다. 작업 ID 기준 멱등 키. 완료 검증, 소스 관측 경로, 도구 영수증 수, 반복 수, 기록된 세션 토큰, 부족한 검사 수를 집계한다.
- `src/engine/rsi/planner/focus.rs::generation_context` → `goal_feedback::context`로 기존 이력 중 실제 선택 파일에 해당하는 최근 8건만 모델의 다음 세대 **조사 입력**에 넣는다. 사용자의 원문 프롬프트·동결 holdout·개인 작업공간 경로는 전달하지 않는다.
- `independentRsiGain:false`, `independentPairedEvaluation:false`를 명시한다. 운영 목표의 성공률은 RSI의 동결 독립 평가, 소스 채택, 후손 실행을 대체하지 않는다. `rsi/evaluation`, `rsi/lineage` 게이트는 변경하지 않았다.
- `tests/unit/goal_feedback_tests.rs`의 멱등 기록·검증 경계·민감 프롬프트 배제 확인 테스트를 추가했다.
- 2026 참고: [(Self-Improving)² coding agents, 2026-09](https://self-improving-coding-agents.github.io/SI2CA-Visualization/site/).

### 3. 읽기 전용 병렬 연구 에이전트

- `tools.rs::definitions` → `delegate_tasks` → `tools/batch.rs::delegate_batch`에서 최대 4개 독립 질문을 사전 검증하고, 각 질문을 `delegate_task(wait=false,readOnly=true)`로 동시에 등록한 뒤 `task_status/task_result`로 결과를 수집한다.
- `tools.rs::delegate`는 `session.delegatedReadOnly=true`, `job.input.delegatedReadOnly=true`를 기록하며 기존 session/goal 부모 관계를 유지한다.
- `chat/runtime/turn.rs`는 읽기 전용 자식에게 허용된 도구만 스키마로 제공한다. `tools.rs::execute` 및 `chat/runtime/tool_execution.rs::dispatch_tool_call` 양쪽이 권한 범위를 검사해 직접 호출·MCP 우회도 거부한다.
- 부모 목표가 격리 worktree를 사용하면 자식도 같은 격리 작업공간을 상속한다. 최종 쓰기와 검증 책임은 부모에게 남는다. 동시 실행 4개 및 세션별 총 24개 제한은 전체 작업 큐의 관측 기반이며 분산 멀티노드 원자적 세마포어는 아니다.

## 체크포인트·정합성

- 영상의 원격 요청 불명확 상태에서는 추가 결제를 막고 사용자 검토로 전환한다. 기술 검증에 통과한 기존 샷만 재사용한다.
- 정적 연결과 테스트 결과는 구분한다. 이 변경 자체로 영화 시청 품질이나 모델 주도 RSI A→B→A 성공을 입증하지 않는다.
- 큰 `video.rs`에서 ComfyUI 전송을 별도 140줄 모듈로 분리했고, 읽기 전용 위임도 `tools/batch.rs`로 분리했다. 다음 후보는 도구의 네트워크/파일/프로세스 계층과 `rsi_system.rs`의 설계 인터페이스이다.
