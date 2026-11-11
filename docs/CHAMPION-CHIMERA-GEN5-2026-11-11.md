# DOMINION — 5세대 Champion 키메라 개발 인수인계 (2026-11-11)

## 역할·기준
- 역할: Champion / 담당 `dominion-champion`. 이 문서 작성 직전 HEAD `411868863891b5dcf083a7392ddbcd14e9331a4e`. `main`, Sub1~4는 이번 세션에서 수정하지 않음.
- 출처: `DOMINION_COMMON_SOURCE.md` 원본. 핵심은 모델 교체에도 지속하는 자율 목표 완주, 실패 수정, 경험 일반화, 실행 가능한 세대 계승, 하네스·알고리즘 자기개선.
- 독립 심판의 과거 배점: 핵심 40 / 완주 30 / 자기개선 20 / 일반화 10. 구 handoff에 남은 직전 심판 평가는 Champion 39, Sub2 37, Sub3 36, Sub4 35, Sub1 34; 최신 다섯 브랜치 재평가 아님. 교환 조건은 Sub >90 이고 Champion보다 높을 때만.
- 비교한 최신 Sub HEAD: Sub1 `95c72f4`, Sub2 `8ad1e11`, Sub3 `0759930`, Sub4 `0061b71`. GitHub commits/diffs 및 인수인계와 실제 Actions 로그를 대조함.

## 이번 Champion 실제 코드 변경
1. `0e02dcb`: Sub3가 실제 목표 분해 벤치마크에서 입증한 한국어 동작 연결 규칙을 `src/engine/decompose.rs` 및 실제 채팅의 `task.decomposition` 전략 선택에 결합. 원본 Sub3 0.75→1.00 측정은 좁은 과제에서의 값이지 DOMINION 전반 완주 품질 아님.
2. `5c55ebd` + 교정 `c160bf2`: Sub4의 `$rsiFocusPath` 동적 소스 바인딩, 사용된 도구 영수증과 적용 경로 전이를 통합. 통합 초기에 빠졌던 `focusPath/crossDomain` 실제 실행 영수증을 고쳐 실소스 2개 파일 수정 테스트 통과.
3. `0bf2343`: 명시적 소스 목표의 실행 프롬프트를 연구 설명으로 종료하지 않고 실제 `workspace_replace` 시도로 연결.
4. `edaff45`: Sub2 반복 무성과 연구에서 근거 있는 새 실행 가능한 탐색 정책을 합성·저장·다음 세대 시험에 대기시키는 기능. 계측된 후보 개선 없이 자동 승격하지 않음.
5. `6a43c3a`: Ollama 제공자의 bare `http://host:11434`를 실제 OpenAI-호환 `/v1/chat/completions` 경로에 연결. 기존 클라우드 모델 URL은 불변.
6. `9fbb473`: 실제 `qwen3:4b-instruct` 다운로드→DOMINION Rust 서버→SSE 채팅→도구 사용 테스트를 GitHub Actions에 구축하고 원본 진단 영수증 업로드.
7. `0a15947`: Sub3 최신의 실행 단계 전환: 명시적 소스 수정에 확인된 원문 근거가 있으면 다음 라운드에서 쓰기 도구만 제공, 각 모델 라운드에도 유지, 모델이 읽기 직후 끝내려 할 때 한번 수정 기회를 부여. 채팅과 RSI worker session 실제 경로 연결.
8. `4a11c6f`: 실제 로컬 모델이 60초 넘게 답한 직후 15분 격리돼 도구 호출 후 대화가 끊기는 문제 수정. 클라우드 느린 공급자 회전 정책은 보존하고 로컬의 장시간 추론은 허용.
9. `9b5782b`: 실모델 테스트가 `tool_start`만 보고 통과하지 않도록, 도구 완료 + 후속 모델 답변 저장 + 오류 없음까지 구분·필수화. 소스 경로 변경 시 재실행.
10. `1993e31`: Sub4 최신, 성공했던 학습 절차의 **다른 분야 전이 실패**가 실제 도구 작업과 위반 영수증으로 입증된 경우, 다음 세대가 동일 실제 파일에서 다른 구현을 1번 수정하도록 구성. Champion의 `improvement_directive`와 독립 채택 조건 유지. 테스트 추가.
11. `4118688`: 실제 무료 `openai-fast` 모델을 DOMINION RSI worktree/동결 평가까지 돌리는 Champion 캠페인 구성. 코드의 현실적 미해결 `-고` 접속형(`만들고`, `지우고`, `고치고`)에서 읽기→실제 수정→후보→독립 평가→계승 경로를 측정하도록 설정. 모델 결과와 스크립트 상태를 있는 그대로 저장하며 통과를 미리 주장하지 않음.

## 독립 검증, 실제 모델 영수증
- **확인된 CI 성공**: `0e02dcb` run 42948871457, `c160bf2` run 42949854051 (프론트 빌드, 16 벤치마크 / 644 Rust 단위 / 3 추가 / 99 E2E = 762), `edaff45` run 42950036370, `6a43c3a` run 42950168227, `9fbb473` run 42950479447.
- **일시 실패→교정 완료**: `5c55ebd`, `0bf2343`는 각각 run 42949140046 / 42949530489에서 실제 method application 영수증 `focusPath` 누락으로 테스트 1건 실패. `c160bf2`에서 수정하고 762 전체 통과.
- **실제 모델 1차** `9fbb473` run 42950479339: CPU Ollama Qwen 4B가 `/api/chat`으로 응답을 생성·저장했고, 두번째 turn에서 `workspace_list` 시작/종료 이벤트를 실제 발생시켰음. 그러나 후속 모델 출력 저장 이전에 '최근 응답이 너무 느렸던 모델' 격리 오류 발생. 그 실행의 `transportConnected=true`, `toolExecutionObserved=false`가 정확한 영수증. 그래서 `4a11c6f` 고친 뒤 `9b5782b` 재실행.
- 최신 `0a15947`, `4a11c6f`, `9b5782b`, `1993e31`, `4118688`의 CI와 최신 Qwen 실험 및 무료 모델 RSI 실험은 **이 문서 작성 시점에 아직 결과 미확인**. GitHub Actions 참조: CI 42950992665 / 42951437155 / 42951558595 / 42951655389 / 42951864235, Qwen run 42951558544, 실제 RSI run 42951864296.
- Sub3 최신 무료 모델 실험 run 42949697316은 workflow 자체 성공했지만 실제 모델 3회 호출, `workspace_replace` 0회, 후보 0, 채택 0. 진짜 병목은 조사→수정 도구 단계이므로 `0a15947`를 도입. 성공을 추정하지 말 것.
- **자율 3세대 연속 실제 독립 품질 향상 및 개선된 바이너리·전이 절차의 재계승은 아직 입증되지 않음.** 빌드/테스트 통과는 기능 연결 검증이지 새 RSI 능력 향상 영수증은 아님.

## 남은 결정적 결함과 다음 우선순위
1. 위의 최신 CI와 실모델 영수증을 읽고 **실패 원인 코드 수정**. 특히 Ollama tool 후속 응답의 실제 종료와 무료모델의 `workspace_replace` 횟수 및 후보 평가를 본다. workflow success 와 task success 구분.
2. 실제 모델에서 0회 write가 계속되면 `rsiExplicitFocus`, `rsiFocusContextReady`, 실제 실행 operation, 허용 tool set, `sourceExcerptDeferred`, 모델의 `tool_calls`를 같은 run에서 연결 조사하고 수정. 의미 없는 프롬프트 장황화는 후순위.
3. 실제 패치 생성시 독립 holdout의 task quality/latency·regression·토큰 비용과 baseline 대비 gain을 확인. 채택된 바이너리가 후속 세대에 반영되고 다른 작업에도 학습 절차를 활용하는지 실제 영수증으로 증명.
4. Sub1 최신 `complete_candidate` (도구 편집 실행은 있었지만 미완성 후보) 경로를 Champion의 관측 기반 `improvement_directive`와 비교해 추가 통합할 가치 판단.
5. 성공한 계승 증거가 쌓인 뒤 독립 심판의 40/30/20/10 기준으로 최신 5개 브랜치를 재평가; 자격 충족 시 사용자 최신 역할 배정대로 자리 교환.

## 반복하지 말 것
- 연구 기록만 늘리고 세대/후보 완주를 주장하지 말 것.
- 로컬 CPU 모델의 60초 넘는 정상 추론을 provider failure로 간주해 중간 격리하지 말 것.
- 선택적 2개 파일 병합으로 영수증의 `focusPath` 등 실행 데이터가 빠지는 일을 반복하지 말 것.
- `main`, Sub1~4 수정하지 말 것. 작업은 반드시 현재 배정받은 `dominion-champion`.

## 최신 실측 업데이트 (GitHub Actions 재확인)
- **실제 Qwen 4B 도구 왕복 완주 확인**: `9b5782b` run **42951558544**는 성공으로 끝났고 실제 SSE 영수증에서 첫 turn `text → assistant_saved → done` (오류 0), 다음 turn `workspace_list tool_start → tool_end → text → assistant_saved → done` (오류 0). `transportConnected=true`, `toolDispatchObserved=true`, `toolExecutionObserved=true`. 첫 실험 run 42950479339의 `toolExecutionObserved=false` 실패를 `4a11c6f`가 수정한 실측 개선. 단, 작업공간 파일을 고치는 RSI 완주와는 별개.
- **추가 확인된 전체 CI 성공**: `0a15947` run 42950992665 (grounded edit-only 단계), `4a11c6f` run 42951437155 (로컬 모델 중도 격리 수정), `9b5782b` run 42951558595 (실모델 테스트 엄격화), `1993e31` run 42951655389 (실패한 전이의 1회 교정).
- **Sub1에서 추가 도입한 핵심 계승 개선**: `3439285`에서 첫 세대 명시적 소스 수정은 `implement_research` 모드로 곧바로 들어가고, 동일 지식 분할·동일 목표에서 최근 실제 결과의 시각과 소스 버전을 비교해 다음 캠페인으로 전달. 오래된 연구보다 뒤의 무성과 구현 실패를 우선하며 새로운 소스 revision에는 이전 실패를 적용하지 않는다. `completedAt` 밀리초 영수증과 회귀 테스트 4개 추가. 해당 커밋 CI run **42952429110**, 실측 결과 확인 전에는 통과라고 보고 금지.
- **별도 실제 RSI 캠페인**: `4118688` run **42951864296** 은 `openai-fast` 무료 실모델로 현실적 한국어 동작 연결 결함을 수정하고 독립 평가·후속 계승까지 측정. 이 문서 작성 중 해당 실행은 진행 중; 실제 `sourceEditCalls`, `candidateCount`, `acceptedGenerations` 및 실제 실험 오류를 판독하기 전까지 완주로 인정하지 않는다.
- 다음 세션 최우선: latest CI 및 실제 RSI 실험의 **소스 쓰기 횟수와 채택 실패 원인**을 읽어 수정한 뒤 재실행. 4B 로컬 모델의 도구 왕복은 이미 확인된 성과이므로 같은 기본 연결 점검을 처음부터 반복하지 말 것.
