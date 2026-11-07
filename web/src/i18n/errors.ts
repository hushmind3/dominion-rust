import { getLocale } from './index'

/**
 * The engine reports errors as Korean text. The UI keeps those messages as
 * the source of truth, and localizes them at the API boundary so English
 * users see actionable English errors without rewriting every backend call
 * site. Unknown messages pass through unchanged rather than being hidden.
 */
const exact: Record<string, string> = {
  'API 연결을 저장할 수 없습니다.': 'Could not save the API connection.',
  'API 연결을 찾을 수 없습니다.': 'API connection not found.',
  'API 연결을 해제할 수 없습니다.': 'Could not disconnect the API connection.',
  'API 형식 워크플로 JSON 객체가 필요합니다.': 'An API-format workflow JSON object is required.',
  'Bot Token 형식이 올바르지 않습니다.': 'Invalid bot token format.',
  'ComfyUI HTTP(S) 주소를 확인하세요.': 'Check the ComfyUI HTTP(S) address.',
  'FreeLLMAPI 연결을 찾을 수 없습니다.': 'FreeLLMAPI connection not found.',
  'FreeLLMAPI 허브에 연결할 수 없습니다.': 'Could not connect to the FreeLLMAPI hub.',
  'HTTP(S) 주소만 허용됩니다.': 'Only HTTP(S) addresses are allowed.',
  'archived:true/false가 필요합니다.': 'archived must be true or false.',
  'enabled는 true/false여야 합니다.': 'enabled must be true or false.',
  'fork 실패': 'Fork failed.',
  '공통 지침은 16000자 이내 문자열이어야 합니다.': 'Shared instructions must be a string of at most 16,000 characters.',
  '공통 지침을 저장할 수 없습니다.': 'Could not save shared instructions.',
  '권한 정책 JSON 객체가 필요합니다.': 'A permission policy JSON object is required.',
  '권한 정책을 저장하지 못했습니다.': 'Could not save the permission policy.',
  '그룹과 외부 채널은 각 대화 화면에서 관리하세요.': 'Manage groups and external channels from their conversation screens.',
  '기억 추출 작업을 저장할 수 없습니다.': 'Could not save the memory extraction job.',
  '다른 메신저는 새 연결로 등록하세요.': 'Register other messengers as a new connection.',
  '다른 요청이 먼저 이 승인을 처리했습니다.': 'Another request already resolved this approval.',
  '메시지를 보낼 DOMINION 회의실을 선택하세요.': 'Choose a DOMINION room to send the message.',
  '모델 ID가 너무 깁니다.': 'The model ID is too long.',
  '모델 목록 형식을 확인할 수 없습니다. 서버 주소와 API 방식을 확인하세요.': 'Could not parse the model list. Check the server address and API mode.',
  '모델 제공자가 필요합니다.': 'A model provider is required.',
  '목표 내용이 필요합니다.': 'Goal content is required.',
  '목표를 저장할 수 없습니다.': 'Could not save the goal.',
  '목표를 찾을 수 없습니다.': 'Goal not found.',
  '반복·재계획 한도가 유효하지 않습니다.': 'The iteration and replanning limits are invalid.',
  '번역 결과가 비어 있습니다.': 'The translation result is empty.',
  '번역 대상 언어가 올바르지 않습니다.': 'Invalid target translation language.',
  '번역 서버에 연결할 수 없습니다.': 'Could not reach the translation server.',
  '번역 응답을 읽지 못했습니다.': 'Could not read the translation response.',
  '번역할 텍스트를 5,000자 이하로 선택하세요.': 'Select up to 5,000 characters to translate.',
  '변경할 에이전트를 1~100명 선택하세요.': 'Select between 1 and 100 agents to change.',
  '봇 발언은 주인의 기억 근거로 학습하지 않습니다.': 'Bot messages are not learned as the owner memory evidence.',
  '사용 가능한 학습 제공자를 선택하세요.': 'Choose an available learning provider.',
  '서버 주소를 입력하세요.': 'Enter the server address.',
  '서버가 모델 목록 JSON을 반환하지 않았습니다. 서버 주소를 확인하세요.': 'The server did not return model list JSON. Check the server address.',
  '선택한 에이전트를 모두 찾고 저장할 수 없습니다.': 'Could not find and save all selected agents.',
  '세션 보관 상태를 저장할 수 없습니다.': 'Could not save the session archive state.',
  '세션을 저장할 수 없습니다.': 'Could not save the session.',
  '세션을 찾을 수 없습니다.': 'Session not found.',
  '스킬을 찾을 수 없습니다.': 'Skill not found.',
  '승인 id 가 필요합니다.': 'An approval ID is required.',
  '승인 요청을 찾을 수 없습니다.': 'Approval request not found.',
  '실행 검토에 사용할 모델 ID를 입력하세요.': 'Enter a model ID for execution review.',
  '실행 에이전트를 찾을 수 없습니다.': 'Execution agent not found.',
  '실행 중인 로컬 모델은 모델 관리에서 중지하세요.': 'Stop the running local model in Models.',
  '실행자·검토자·계획자는 객체로 지정하세요.': 'Executor, reviewer and planner must be objects.',
  '실행할 작업 지시가 필요합니다.': 'A task instruction is required.',
  '에이전트 ID가 올바르지 않습니다.': 'Invalid agent ID.',
  '에이전트 이름이 필요합니다.': 'An agent name is required.',
  '에이전트를 저장할 수 없습니다.': 'Could not save the agent.',
  '연결 ID가 올바르지 않습니다.': 'Invalid connection ID.',
  '연결 설정 객체가 필요합니다.': 'A connection settings object is required.',
  '연결 설정을 저장할 수 없습니다.': 'Could not save the connection settings.',
  '연결을 찾을 수 없습니다.': 'Connection not found.',
  '영상 설정을 저장할 수 없습니다.': 'Could not save the video settings.',
  '영상 작업을 찾을 수 없습니다.': 'Video job not found.',
  '영상 파일을 열 수 없습니다.': 'Could not open the video file.',
  '영상 폴더를 열 수 없습니다.': 'Could not open the video folder.',
  '워크플로는 1MiB 이하로 저장하세요.': 'Workflows must be 1 MiB or smaller.',
  '워크플로를 찾을 수 없습니다.': 'Workflow not found.',
  '유효한 모델 ID가 필요합니다.': 'A valid model ID is required.',
  '이 작업은 지원하지 않습니다. 영상 작업 만들기를 사용하세요.': 'This operation is not supported. Use Create video job.',
  '이 채널의 진단을 지원하지 않습니다.': 'Diagnostics are not supported for this channel.',
  '작업 상태가 변경되었습니다.': 'The job state changed.',
  '작업을 대기열에 저장할 수 없습니다.': 'Could not save the job to the queue.',
  '작업을 찾을 수 없습니다.': 'Job not found.',
  '작업이 없습니다.': 'No job.',
  '제공자를 찾을 수 없습니다.': 'Provider not found.',
  '제목이 비어 있습니다.': 'The title is empty.',
  '지원하지 않는 채널입니다.': 'Unsupported channel.',
  '진행 중인 작업만 중단할 수 있습니다.': 'Only running jobs can be stopped.',
  '채널 토큰을 암호화하지 못했습니다.': 'Could not encrypt the channel token.',
  '채널 토큰을 입력하세요.': 'Enter the channel token.',
  '학습할 세션을 찾을 수 없습니다.': 'Session to learn not found.',
  '학습할 대화가 없습니다.': 'No conversation to learn from.',
  '다운로드 작업 정보가 손상되었습니다.': 'Download job information is corrupted.',
  '안전하지 않은 모델 파일 경로입니다.': 'Unsafe model file path.',
  '다운로드 스트림 오류': 'Download stream error.',
  '모델 파일을 찾을 수 없습니다.': 'Model file not found.',
  '학습할 참가자가 없습니다.': 'No participants to learn from.',
  '해당 작업은 대화·회의·영상의 실행 API에서 시작하세요.': 'Start this job from the chat, room or video execution API.',
  '활성화된 모델 제공자를 선택하세요.': 'Choose an enabled model provider.',
  '회의실을 찾을 수 없습니다.': 'Room not found.',
  '모델을 찾을 수 없습니다.': 'Model not found.',
  '모델 제공자를 찾을 수 없습니다.': 'Model provider not found.',
  '로컬 모델을 실행하지 못했습니다.': 'Could not start the local model.',
  '로컬 모델을 중지하지 못했습니다.': 'Could not stop the local model.',
  'API 키가 필요합니다.': 'An API key is required.',
  'API 키를 확인하세요.': 'Check the API key.',
  '요청이 실패했습니다.': 'The request failed.',
  '요청 본문이 너무 큽니다.': 'The request body is too large.',
  '지원하지 않는 메서드입니다.': 'Unsupported method.',
  '인증이 필요합니다.': 'Authentication is required.',
}

const nouns: Record<string, string> = {
  세션: 'session',
  회의실: 'room',
  방: 'room',
  작업: 'job',
  '실행 기록': 'execution',
  연결: 'connection',
  스킬: 'skill',
  목표: 'goal',
  에이전트: 'agent',
  '승인 요청': 'approval request',
  '모델 제공자': 'model provider',
  제공자: 'provider',
  '영상 작업': 'video job',
  워크플로: 'workflow',
  개선안: 'improvement',
  레코드: 'record',
  'API 연결': 'API connection',
  채널: 'channel',
  모델: 'model',
  메시지: 'message',
  파일: 'file',
  폴더: 'folder',
  대화: 'conversation',
  기억: 'memory',
  '학습 작업': 'learning job',
  '첨부 파일': 'attachment',
  'API 경로': 'API path',
  '대화 세션': 'conversation session',
}

function enNoun(value: string): string {
  const key = value.trim()
  return nouns[key] || key
}

export function translateError(message: string): string {
  if (getLocale() !== 'en' || !message) return message
  const text = message.trim()
  if (exact[text]) return exact[text]

  const rule = (pattern: RegExp, build: (match: RegExpMatchArray) => string) => {
    const match = text.match(pattern)
    return match ? build(match) : ''
  }

  return (
    rule(/^(.+?)(?:을|를) 찾을 수 없습니다\.$/, match => `Cannot find ${enNoun(match[1])}.`) ||
    rule(/^(.+?)(?:이|가) 없습니다\.$/, match => `No ${enNoun(match[1])}.`) ||
    rule(/^(.+?)(?:을|를) 저장할 수 없습니다\.$/, match => `Could not save ${enNoun(match[1])}.`) ||
    rule(/^(.+?)(?:을|를) 불러올 수 없습니다\.$/, match => `Could not load ${enNoun(match[1])}.`) ||
    rule(/^(.+?)(?:이|가) 필요합니다\.$/, match => `${enNoun(match[1])} is required.`) ||
    rule(/^(.+?) ID가 필요합니다\.$/, match => `A ${enNoun(match[1])} ID is required.`) ||
    rule(/^(.+?) ID가 올바르지 않습니다\.$/, match => `Invalid ${enNoun(match[1])} ID.`) ||
    rule(/^지원하지 않는 (개발 작업|미리보기 작업|작업|채널)입니다: (.+)$/, match => `Unsupported ${enNoun(match[1])}: ${match[2]}.`) ||
    rule(/^참가 에이전트를 찾을 수 없습니다: (.+)$/, match => `Participant agent not found: ${match[1]}.`) ||
    rule(/^(.+?) 레코드를 찾을 수 없습니다\.$/, match => `Cannot find ${enNoun(match[1])} record.`) ||
    rule(/^([A-Za-z0-9_]+)는 true\/false여야 합니다\.$/, match => `${match[1]} must be true or false.`) ||
    rule(/^([A-Za-z0-9_]+)는 문자열이어야 합니다\.$/, match => `${match[1]} must be a string.`) ||
    rule(/^([A-Za-z0-9_]+)는 (\d+)자 이하여야 합니다\.$/, match => `${match[1]} must be at most ${match[2]} characters.`) ||
    rule(/^([A-Za-z0-9_]+) 범위는 (\d+)~(\d+)입니다\.$/, match => `${match[1]} must be between ${match[2]} and ${match[3]}.`) ||
    rule(/^([A-Za-z0-9_]+)는 양의 정수여야 합니다\.$/, match => `${match[1]} must be a positive integer.`) ||
    rule(/^([A-Za-z0-9_]+)에는 문자열 ID 목록이 필요합니다\.$/, match => `${match[1]} requires a list of string IDs.`) ||
    rule(/^([A-Za-z0-9_]+)는 서버가 관리하는 상태입니다\.$/, match => `${match[1]} is managed by the server.`) ||
    rule(/^모델 목록 조회 실패 \(HTTP (\d+)\)\. (.*)$/, match => `Model list request failed (HTTP ${match[1]}). ${match[2]}`) ||
    rule(/^Rust 백엔드에 없는 API 경로입니다: (.+)$/, match => `API path not found in the Rust backend: ${match[1]}`) ||
    rule(/^(.+) 종료 \(exit (.+)\)$/, match => `${match[1]} exited (exit ${match[2]})`) ||
    rule(/^(.+) 가 시작 중 종료되었습니다: (.*)$/, match => `${match[1]} exited while starting: ${match[2]}`) ||
    rule(/^다운로드 HTTP (\d+)$/, match => `Download HTTP ${match[1]}`) ||
    rule(/^secret key 복원 실패: (.+)$/, match => `Secret key restore failed: ${match[1]}`) ||
    rule(/^복구 대상의 secret key를 확인할 수 없습니다: (.+)\. 현재 상태를 유지합니다\.$/, match => `Cannot read the recovery secret key: ${match[1]}. The current state is preserved.`) ||
    rule(/^복구 후 마이그레이션 실패: (.+)$/, match => `Post-recovery migration failed: ${match[1]}`) ||
    rule(/^허브 모델 목록 HTTP (\d+)$/, match => `Hub model list HTTP ${match[1]}`) ||
    rule(/^모델 목록 조회 실패 \(HTTP (\d+)\)\. (.*)$/, match => `Model list request failed (HTTP ${match[1]}). ${match[2]}`) ||
    rule(/^(.+?) 실패$/, match => `${enNoun(match[1])} failed.`) ||
    text
  )
}
