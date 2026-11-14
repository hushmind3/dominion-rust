# Sub1 · 실행 증거의 연속성과 장기 코딩 목표 완수 검증 (2026-11-14)

## 심사 기준

이번 재심사의 `/goal`, 별도 메뉴, `Goals.tsx` 존재 자체는 0점. 본 개선은 자율 에이전트가 **작업을 완료했다고 판정하는 실제 서버 기준과 도구 실행 능력**을 바꾼다.

## 실제 경로와 변경

1. `engine/goals.rs::run`은 매 iteration에서 `run_quiet_outcome`을 호출한다.
2. `chat/runtime/tool_dispatch.rs`는 원래 도구 실행 직후 `tool_end` 이벤트를 DB commit보다 먼저 보낸다. `engine/mod.rs`의 이전 이벤트 수집기는 `tool`, `arguments`, `result`만 남겼으므로 동일 세션의 앞선 성공 검사와 **그 뒤 소스 변경**을 구분할 수 없었다.
3. 새 경로: 도구 이벤트에 `operationId`를 전송하고, `run_quiet_outcome`이 종료된 후 정확히 그 ID로 저장된 `execution_operation`만 읽는다. 저장 실패 또는 `status != completed`인 이벤트는 성공 증거로 인정하지 않는다. 도구/인자/결과에 더해 `runId`, `opSeq`, `startedAt`, `finishedAt`, effect receipt 등 원본 기록이 목표로 전달된다.
4. `verification.rs::fresh_command_matches`는 `command_succeeded`의 성공 영수증이 실제 코드 쓰기 뒤의 것인지 확인한다. 단일 실행은 `opSeq`, 여러 실행은 RFC3339 시각으로 판정한다. 검증 이후 소스 쓰기가 있으면 같은 검증 명령을 최신 상태에서 다시 실행해야 한다.
5. 재검증이 필요한 경우 `acceptance_gaps`가 구체적으로 어떤 명령의 영수증이 오래됐는지 알려주므로, 다음 executor/replanner가 같은 성공 주장을 반복하는 대신 새 검사를 선택할 수 있다.
6. `chat/runtime/tool_selection.rs`의 `core` 또는 `query-relevant` 알고리즘에서도 코딩 요청이면 `code_search`, `workspace_replace_range`, `workspace_replace_function`을 반드시 남긴다. 대화/비코딩 요청에는 스키마를 추가하지 않는다. 일반 `all` 전략은 그대로다.
7. `verification::looks_actionable/infer_criteria`는 공통 어휘 경계 검사를 사용하므로 `brunch` 내 `run` 같은 부분 문자열 때문에 읽기 전용 작업을 실행 요청으로 오인하지 않는다.

## 평가 가능한 회귀 사례

- **검증 시간 순서:** `test → edit` = 미검증, `edit → test` = 통과, `edit → test → edit → test`는 마지막 검사에서만 통과.
- **여러 반복:** 이전 execution의 성공 명령 뒤 다음 execution에서 쓰면 성공 영수증 폐기. 최신 소스 이후의 새로운 명령 성공 영수증만 인정.
- **증거 원본 연결:** 도구가 저장되고 뒤이어 provider가 실패해도 저장된 소스 변경과 그 시각은 유지. DB에 기록되지 않은 이벤트는 완료 증거가 아니다.
- **2단계 실제 도구 통합:** 모의 LLM 응답을 이용해 목표 iteration 1에서 `workspace_write`로 `src/lib.rs` 작성, iteration 2에서 실제 `shell_exec`로 `echo verified` 실행, 두 조건과 독립 검토가 충족되어야 완료. 이 테스트는 작업 엔진의 경로 검증이지 유료 실모델 성능 기록이 아니다.
- **도구 검색:** 코딩 요청 `core`/0-extra에서도 소스 위치 검색과 국소 수정 도구를 보유, 평상시 대화에서 도구 카탈로그를 키우지 않는다.

## 적용 범위 및 한계

- `source_mutation`은 네이티브 파일 쓰기·수정과 보수적으로 식별한 소스 변경 셸 명령을 다룬다. 임의 MCP 외부 변형이나 난독화 셸 명령 전체의 파일 영향 분석을 주장하지 않는다.
- 기록에 실행 순서와 시간이 없어 같은 파일 변경이 검증 뒤에 일어났을 가능성을 지울 수 없다면, 해당 성공 영수증을 보수적으로 인정하지 않는다. 기존 기록의 자동 조작/삭제는 하지 않는다.
- 운영 모델의 실제 독립 작업 완수율, F2P/P2P, 비용, 장기 복구 성공률은 별도 유료/실환경 재실행 후 계측해야 한다. CI는 Rust 단위·통합 경로 및 프론트 정합성까지만 증명한다.

## 2026년 외부 근거 (실험 결과를 우리 제품 성과로 간주하지 않음)

- [SWE-Continuum (2026-11-09)](https://sotalab.ai/blog/swe-continuum): 최종 성공률만으로 단계별 회귀를 포착하기 어려움.
- [Long-Horizon Software Engineering Agents (2026)](https://longhorizonswe.github.io/): 이어지는 작업의 retained functionality와 conformance 평가.
- [PROBE (2026)](https://arxiv.org/abs/2605.08717): 실패 원인과 실제 다음 시도를 연결하는 구조화된 복구.
- [Sourcegraph CodeScaleBench (2026)](https://github.com/sourcegraph/CodeScaleBench/blob/public/docs/technical_reports/TECHNICAL_REPORT.md): 코드 탐색 도구의 접근 방식과 비용/성과 차이.
