# DOMINION 자체 Rust 엔진

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
