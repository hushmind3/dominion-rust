DOMINION 토큰 문제 — 다음 세션 인계 (2026-11-08)

결론: 미해결. 이전 에이전트가 부분적인 캐시 적중을 실제 운영 비용 문제 해결처럼 보고했다. 완료로 취급하지 말 것.

요구
- 같은 봇이 대화를 매번 새로 시작하지 않게 하고, 짧은 인사의 반복 신규 입력 비용을 줄일 것.
- 9명, 현재 모델, 페르소나와 원문을 유지. 봇 수 축소·모델 교체·인사 하드코딩·표시 숫자 변경으로 우회하지 말 것.
- Rust 범용 하네스 유지. 매턴 요약 LLM 추가 금지. 프론트 디자인 변경 금지.
- 검증 안 된 지속 연결 등 큰 변경을 무리하게 넣지 말 것.

현재 상태
- 저장소: /Users/gim-yeonsu/dominion-rust
- 브랜치: dominion-sub, origin과 동일. 엔진 커밋 39ddc332cde409e05c3b10053db7508390a6e331.
- origin/main 로컬 참조: 2f64e7f. main에 병합하지 말 것. 새 브랜치 만들지 말 것.
- 운영: http://127.0.0.1:4317, runtime/dominion.sqlite. 방 room:nazarick-circle.
- 9명 모두 codex-oauth / gpt-6-luna. OAuth는 인증 방식이지 반복 입력의 원인으로 확정된 것이 아니다.
- 실행 바이너리 target/release/dominion-rust, 확인 당시 PID 32877.
- launch agent: com.hushmind.dominion.rust. 재시작 전 jobs/executions의 실행·승인 대기 작업 확인 필수.
- tmp/는 기존 사용자 소유 미추적 폴더. 건드리지 말 것.

이미 구현한 것 (해결 증거와 구분)
- src/engine/chat/continuity.rs: prepare/project/commit/commit_projection.
- 세션+봇의 participant_conversation에 작업 문맥 workingView 저장. 모델·제공자와 독립적인 논리 대화 상태.
- 실제 전송했던 문맥 뒤에 새 출처 ID의 메시지를 추가. 저장된 최종 답변 이후 확정. 재시작 복구.
- 계약·증거 변경, 대화 초기화, 이력 한도 초과 시 재구성. 원문은 삭제하지 않음.
- provider.rs의 stream_completion_once에서 공통 적용. context.rs의 render_history가 출처 태그 부여.
- 그러나 stream_codex_once는 매번 instructions와 전체 선택 input을 HTTP Responses로 전송한다.
- codex_input은 자기 답변을 텍스트로 다시 구성한다. 원격 응답 ID·native output/encrypted reasoning을 지속 저장·재생하는 구현은 없다.
- 캐시 키와 입력 접두사 유지가 실제 캐시 적중을 보장하지 않는다.
- 로컬 430 unit + 89 E2E + 16 benchmark 통과. CI 성공: https://github.com/hushmind3/dominion-rust/actions/runs/42475822457
- 테스트 성공은 사용자 비용 문제 해결을 의미하지 않는다.

실측
1. 최신 운영 관측 9건: 입력/신규 합계 19,996, 캐시 전원 0. 각 1회 모델 호출.
   SQLite 기록 2026-11-08 11:39:02~11:39:20 UTC. workingView는 전부 epoch=1, reusedFrames=0, reason=initial.
   이 9건만으로 이후 매턴 초기화된다고 확정할 수는 없다. 각 봇의 작업 문맥은 이후 DB에 저장되어 있었다.
2. 운영 문맥·기억·9명·방 ID·기존 workingView를 복사한 격리 재생:
   회차1 입력 21,620 / 캐시 12,032 / 신규 9,588, 캐시 적중 6명.
   회차2 입력 23,090 / 캐시 15,232 / 신규 7,858, 캐시 적중 7명.
   reusedFrames=5~13, rebaseReason=null. 논리 문맥은 이어져도 일부 캐시 0과 높은 비용이 남았다.
   toolsEnabled=true 유지, 인사에서 선택 스키마 0. 후속 자동 학습만 껐으므로 전체 운영 학습 비용 검증은 아니다.
3. 실제 WebSocket+previous_response_id 단일 봇 3회 시험:
   입력/캐시: 1983/0 → 2008/1792 → 2033/0. 확인어 응답은 모두 맞음.
   지속 연결만 넣으면 해결된다는 근거 없음. 제품 코드에 이 실험을 반영하지 않았다.
4. instructions를 developer 메시지로 이동하는 3회 시험:
   기존 방식 캐시 0/1792/1792, 이동 방식도 0/1792/1792. 이동 방식은 입력이 9토큰 늘었다. 제품 미반영.

헤르메스 조사
- 소스: /Users/gim-yeonsu/.hermes/hermes-agent
- 현재 config.yaml에서 안전한 항목만 읽어 확인: provider=nous, model=reflection/beam-1.0-501b-a23b:free, api_mode=chat_completions.
- 사용자가 본 절감을 Codex App Server의 효과라고 단정하면 안 됨. 현재 설정과 별개로 해당 실행 경로도 소스에는 존재함.
- agent/prompt_cache_scope.py, agent/transports/chat_completions.py, agent/transports/codex.py: 안정적인 캐시 범위와 키 구성.
- agent/usage_pricing.py: Codex/Chat의 전체 입력에서 cache read/write를 빼 신규 입력으로 정규화. 화면 숫자와 실제 요청 증분을 혼동하지 말 것.
- agent/codex_runtime.py, agent/codex_runtime_history_seed.py: App Server thread start/resume 및 새 thread에만 과거 이력을 seed하는 별도 경로.
- 헤르메스 동일 모델·동일 제공자 조건의 실행 비교는 아직 하지 않았다.

잘못된 설명 및 남은 조사
- “Codex OAuth는 캐시 미지원/항상 0”: 틀림. 실제 캐시 적중을 반복 관측했다.
- “UI에서 공통 계약/페르소나 분리했으니 입력 최적화”: 틀림. 합쳐서 매번 보내면 동일 비용 문제가 남는다.
- 한국어 추정 토큰과 제공자 실측을 더해 정확한 구성표로 제시했던 설명도 부정확하다.
- 캐시 0은 사용량 보고이며 기억 상실이나 물리적 전체 재계산을 직접 증명하지 않는다.
- room_knowledge.rs의 run에는 학습 모델 호출이 있다. 자동 학습 0회라는 과거 설명을 가정하지 말 것. 운영 autoLearning=true.
- continuity.rs evidence_digest에 confidence/importance/revision/supportingCases 등이 포함된다. 내용 동일한 학습 갱신에도 재구성되는지 미검증. 확정 원인으로 쓰지 말 것.
- 제공자 캐시 불안정의 정확한 원인은 미확정. 서버 탓이나 하네스만의 문제로 단정하지 말 것.
- 다음 작업은 동일 조건에서 실제 요청 구성·응답 사용량·문맥 재구성 이유를 비교하고, 차이가 입증된 최소 수정만 할 것.
- 전체 입력, 캐시 읽기/쓰기, 신규 입력, 출력/추론, 별도 학습 호출을 각각 집계. 캐시 키 동일·HTTP 200·표본 몇 개 적중만으로 완료 선언 금지.

자료
- 기존 보고: docs/research/PARTICIPANT-WORKING-VIEW-2026-11-08.md
- 기존 실험: benchmark/PARTICIPANT-WORKING-VIEW-2026-11-08.json
- 추가 실측을 비밀정보 없이 보존: docs/research/handoff-evidence/*.json
- 추가 실측 JSON은 임시 스크립트의 잘못 복사된 toolsEnabled 표기를 실제 재생 조건에 맞게 수정했다.
- 이번 인계 작업에서는 엔진 코드를 추가 변경하지 않았다. 인계 파일과 증거는 아직 커밋·푸시하지 않았다.
- 인증 토큰·키를 문서나 출력에 포함하지 말 것. 실제 방에 시험 메시지를 넣지 말고 격리 환경을 사용할 것.
