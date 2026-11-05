# DOMINION — 범용 AI 하네스의 작동 원리

설계 2.4 · 2026-11-06 · DeepSeek 구현 인계본

**DeepSeek에게:** 이 문서의 실행 원리를 현재 Rust 엔진에 구현하라. 새 설계 보고서를 쓰는 작업이 아니다. 현재 HEAD·미커밋 작업을 먼저 확인하고 기존 기능을 유지하면서 11절 순서대로 연결하라. 아래는 목표 계약이며 이미 구현됐다는 뜻이 아니다. 프론트는 React/Vite/Tailwind를 유지하고 실행 판단은 Rust가 소유한다.

## 1. 엔진이 하는 일

**사용자 요청을 이해하고 → 필요한 정보를 모으고 → 행동하고 → 결과를 읽어 다음 판단을 바꾸고 → 작업을 마치고 → 경험에서 더 나은 방법을 만들어 다음 실행에 사용한다.**

일반 대화, 파일/컴퓨터 작업, 조사, 제작, 에이전트 협업이 모두 이 흐름을 쓴다. 모델은 상황을 해석하고 방법을 만든다. Rust 엔진은 실제 환경과 모델 사이를 왕복시키며 상태·결과·경험을 유지한다. React는 이를 보여 주는 화면이다.

## 2. 매 판단 시점에 엔진이 가지고 있는 상태

```text
S = {
  intent, requirements,                 // 무엇을 이루려는가
  facts, unresolved, hypotheses,         // 무엇을 알며 무엇이 불명확한가
  plan, completed_effects, observations, // 무엇을 했고 어떤 결과가 났는가
  available_tools, available_models,     // 무엇으로 행동할 수 있는가
  relevant_experiences,                  // 어떤 과거 방법이 적용 가능한가
  active_children,                      // 누가 어떤 일을 수행 중인가
  remaining_resources,                 // 시간·토큰·도구·비용
  policy_revision, local_adaptations     // 기본 방법과 이번 작업의 변경
}
```

모든 항목을 prompt에 통째로 넣지 않는다. 현재 판단에 필요한 원문·미해결 조건·관련 증거·도구 설명을 골라 넣는다. 나머지는 식별자로 보관하고 필요할 때 조회한다. 요약이 바뀌어도 원문과 실제 실행 결과는 남는다.

## 3. 실제 실행 루프

```text
run(request):
  S ← load_session_and_unfinished_work(request)
  S.intent, S.requirements ← reuse_or_update_task_contract(request, existing_context)
  S.policy_revision ← current_policy_for(task_features(S))
  S.relevant_experiences ← retrieve_and_match(S)

  while resources_remain(S):
    S ← incorporate_new_input_and_completed_operations(S)
    if waiting_without_runnable_work(S): save(S); return Waiting

    C ← compose_context(S)
    D ← next_program_step(S) if an adapted program is active
         else model_decide(C, available_tools(S))
    // D = Answer | ToolCalls | Delegate | ChangePlan | Probe | Wait

    if D is ChangePlan: update_plan(S, D); continue
    if D is Wait: register_wake_condition(S, D); save(S); return Waiting
    if D contains actions:
      A ← resolve_arguments_and_dependencies(D, S)
      O ← execute_ready_actions(A, S)
      S ← update_from_real_observations(S, O)
      if failure_or_stagnation(S): S ← adapt(S)
      save_checkpoint(S)
      continue

    E ← evaluate(D.answer_or_artifact, S.requirements, S)
    if E reveals repairable_gap:
      S.unresolved ← E.gaps; S ← adapt(S); continue
    finalize(S, D, E)
    enqueue_experience_learning(S, E)
    return Result(D, E)

  save_checkpoint(S); return Unfinished(reason, observed_results)
```

의도 해석은 기존 응답/행동 판단 호출에 합친다. 매 턴 별도 planner 호출을 추가하지 않는다. 사용자의 명시 조건과 모델이 추정한 조건은 구분한다.

모델의 다음 판단은 실제 도구 결과를 읽은 뒤 이루어진다. 처음 세운 계획은 작업 가설이며 새 관측에 따라 바뀐다. 인사나 단순 질문은 한 번의 판단으로 답하고 끝난다. 복잡한 작업만 탐색·위임·검증을 추가한다.

생성한 프로그램의 현재 위치와 분기 입력을 S에 저장한다. 도구 결과가 오면 해당 연산을 완료하고 다음 연산을 계산한다. 프로그램이 끝나거나 새 상황이 생기면 모델 판단으로 돌아간다.

실행은 모델 호출 전, 도구 결과 저장 후, 대기 진입 시 checkpoint를 남긴다. 재시작하면 진행 중이던 외부 행동의 결과부터 확인한 뒤 다음 판단으로 돌아간다. UI를 닫아도 저장된 작업은 같은 상태에서 이어진다.

## 4. 실패하면 다음 방법을 어떻게 결정하는가

실패는 예외 문구 하나가 아니라 **예상한 결과와 실제 관측의 차이**다. 예: 검색 결과 없음, API schema 불일치, 문서의 근거 부족, 파일 내용 오류, 계획의 선행조건 누락.

```text
adapt(S):
  F ← difference(expected_observation, actual_observation)
  H ← explain(F, relevant_trace)               // 원인 가설과 근거
  H ← remove_hypotheses_contradicted_by_observation(H)
  R ← applicable_experiences(S, F)
  A ← instantiate(R) ∪ propose_new_interventions(S, F, H)
  A ← remove_same_attempts_without_changed_conditions(A, S)

  for a in A:
    a.goal_progress ← predicted_unresolved_requirements_resolved(a)
    a.distinction ← hypothesis_pairs_predicting_different_results(a, H)
    a.cost ← estimate_from_tool_and_model_history(a)

  if A is empty: return S with unresolved_failure=F
  if cause_is_unclear(H):
    a ← maximize(|a.distinction| / (1 + normalized_cost(a)))
  else:
    a ← lexicographic_max(comparable_past_effect, predicted_progress, -estimated_cost)

  S.local_adaptations.append(a, reason, predicted_result)
  return S with active_program=a, program_counter=0
```

각 점수의 정의: 과거 효과는 같은 조건에서 관측된 개선/비용이고, 없으면 Unknown이다. 예상 진전은 해소할 requirement ID의 중요도 합이다. 원인 구별 값은 서로 다른 관측을 예측하는 가설 쌍의 수다. 비용은 남은 token/time/tool 예산에 대한 비율이며, 미측정 비용은 Unknown으로 남긴다. 같은 단계에서 비교 불가능한 값은 우열을 확정하지 않고 예산 안의 작은 probe나 모델 선택으로 해소한다.

모델은 원인·새 접근·예상 결과를 만든다. 엔진은 이미 반박된 가설과 실제로 같은 시도를 제거하고, 어떤 관측이 원인을 구별하는지 비교한다. 모델이 예상한 진전은 선택 heuristic이며 달성한 성과는 실행 후 다시 측정한다.

바꿀 수 있는 것은 검색어, 정보원, 도구, 인자, 계획 구조, 검증 방법, 역할 모델, 순차/병렬 탐색 방식이다. 고정된 retry 목록 중 하나만 고르는 구조가 아니다.

같은 호출이어도 환경 상태가 달라졌으면 재시도할 수 있다. 동일 상태·동일 결과만 반복하면 다른 개입을 선택한다. 작은 probe로도 원인을 구분하지 못하면 다른 모델/전문 역할에 진단을 맡기거나 미확정 상태를 유지한다.

## 5. 모델·도구·에이전트를 어떻게 조합하는가

**도구:** 현재 하위 문제로 catalog를 검색해 필요한 도구 설명을 가져온다. 모델이 제안한 호출의 입력·의존성을 해석하고, 독립 작업은 병렬 실행한다. 같은 결과에 의존하는 작업은 관측 후 이어 간다. 새 도구는 같은 호출/관측 형식에 연결된다.

**모델:** 필요한 능력과 데이터 범위에 맞는 모델 중 사용자 지정과 실측 성능을 이용한다. 도구 인자 오류가 반복되면 구조화 지원을 추가하고, 의미 판단이 막히면 다른 모델에 해당 쟁점만 전달한다. 모든 문제에 큰 모델·검토 모델을 붙이지 않는다. 각 호출도 전체 작업 예산에 합산한다.

**협업:** 모델이 병렬로 풀 가치가 있는 하위 문제를 제안한다. 엔진은 각 자식에게 목적·입력·완료 조건·예산을 나누고 결과를 모은다. 결과가 충돌하면 해당 주장과 증거만 대조해 재확인한다. 동의한 에이전트 수보다 근거와 실제 결과로 통합한다. persona/icon은 역할 표현이고 인원수는 실행 구조를 제한하지 않는다.

웹·Discord·Telegram은 같은 엔진에 요청을 넣는다. 공통 대화로 묶은 채널은 같은 작업/대화 기록을 공유하고, 다른 채널은 별도 맥락으로 실행한다.

## 6. 경험을 저장한 뒤 무엇이 달라지는가

하나의 Episode는 `문제 특징 → 실패 조건 → 실제 개입 → 결과 → 평가`다. 저장 후에는 **조건을 가진 실행 방법**으로 바꿔야 다음 판단에 영향을 준다.

```text
generalize(episodes):
  G ← group_by_problem_structure_and_intervention(episodes)
  for group in G:
    T ← align_traces_and_parameterize_names_paths_endpoints(group)
    M ← propose_mechanism(T)                // 어떤 변화가 왜 도움이 됐는가
    P ← predicates_for_environment_features(T, M)
    examples ← matching_successes_and_counterexamples(M)
    rule ← True
    while rule matches known failing examples:
      p ← predicate_maximizing(
           failing_examples_excluded / (1 + successful_examples_excluded))
      if no p excludes a failure while retaining support:
        split_group_or_keep_as_unresolved_hypothesis; break
      rule ← rule AND p
    candidate ← {rule, intervention(T), expected_effects, evidence, counterexamples}
    freeze(candidate); compare_on_unseen_environments(candidate)
    emit candidate with applicability_and_effect_results
```

predicate는 `pagination=opaque_cursor`, `next_token_available`, `independent_partitions`, `response_schema_version`처럼 관측으로 검사할 수 있는 특징이다. 실패 label은 실제 실행/검증 결과로 만든다. 미확정 사례는 성공/실패 대신 보류하고, 같은 환경의 반복이 규칙을 지배하지 않도록 환경별 가중치 합을 같게 둔다.

새 작업에서 `rule=True`면 방법을 구체적인 인자에 연결한다. `False`면 제외한다. `Unknown`이면 필요한 특징을 조회한다. 적용 후 예상과 다르면 그 사례가 새 반례가 되어 조건을 좁히거나 방법을 분리한다. 효용과 적용 범위를 함께 기록한다.

예: cursor를 임의 생성해 누락된 경험은 “응답의 next token을 따라가고 반복 token을 감지한다”로 추출한다. offset 방식은 다른 방법으로 분리한다. API 이름이 달라도 같은 구조면 재사용할 수 있다.

## 7. 기존에 없던 해결 방법을 만드는 과정

생성물은 자연어 조언만이 아니라 **엔진이 실행할 수 있는 입력·분기·반복·도구·검증의 조합**이다.

```text
Program := Observe | ModelDecision | CallTool | Verify | Delegate
         | Sequence(Program...)
         | Branch(predicate, then, else)
         | Parallel(independent Program...)
         | Loop(condition, Program, resource_budget)
```

이 프로그램은 실행 결과를 받아 다음 경로를 정한다. 저장된 고정 순서만 재생하는 구조가 아니다. `ModelDecision`이 남아 있어 새로운 상황의 의미 판단도 모델이 한다.

```text
synthesize(problem, traces, experiences, search_budget):
  frontier ← model_generate_distinct_mechanisms(problem, traces)
  frontier += compose_compatible_fragments(experiences)
  archive ← {}
  while search_budget remains and frontier not empty:
    candidate ← select_diverse_promising_program(frontier, archive)
    candidate ← resolve_types_inputs_and_dependencies(candidate)
    result ← execute_in_test_environment(candidate)
    archive.add(candidate, result, total_cost)
    if result reveals failure:
      counterexample ← minimize_failure_case(result)
      frontier += model_repair(candidate, counterexample)
      frontier += structural_mutations(candidate)
    else:
      frontier += mutations_targeting_cost_or_uncovered_conditions(candidate)
    frontier ← deduplicate_and_keep_quality_cost_diversity(frontier, archive)
  return freeze_pareto_candidates(archive)
```

구조 변이는 `다른 정보원으로 교체`, `probe→조건 분기 추가`, `독립 탐색 병렬화`, `반복 종료 조건 변경`, `검증 위치 이동`, `역할 모델 교체`, `실패 branch 결합`이다. 같은 문장을 바꿔 쓰는 후보는 같은 구조로 묶는다.

현재 작업에서 생성한 프로그램은 작업 상태 안에서 즉시 시도한다. 여러 작업의 기본 방법으로 쓸 후보는 기존 방법과 별도 비교한다. 새 연산 자체가 필요하면 동일한 입력/출력 계약을 가진 Rust 구현 patch를 생성해 build·실행 비교 후 새 엔진 버전에 포함한다.

## 8. 정답이 없는 작업을 어떻게 비교하고 학습하는가

평가 결과는 하나의 점수가 아니라 다음 묶음이다.

```text
E = {verified_facts, requirement_results, semantic_preferences,
     user_feedback, unknown_items, cost, latency, evaluator_revision}
```

사실·수치는 원문/실행 결과와 대조한다. 조사·설계·분석의 품질은 목적에 맞춘 rubric으로 baseline과 candidate의 결과를 작성자 정보 없이 비교한다. 의미 평가의 Win/Tie/Loss/Unknown과 후속 사용자 정정을 각각 남긴다.

비교 순서: `필수 목적 달성 → primary 품질 축 → 비용/시간`. 관측되지 않은 사용자 만족은 학습 target에서 빠지고 Unknown 비율로 남는다. 지연된 정정은 같은 평가의 새 revision으로 반영한다.

비교 시작 전에 primary 지표·최소 개선폭·비열등성 폭·비용 상한·표본/중단 규칙을 manifest에 고정한다. 실험에서는 같은 모델·도구·초기 상태·총예산으로 두 방법을 짝지어 실행한다. timeout도 전체 시도의 결과에 포함한다. 과제/환경 단위의 차이와 불확실성을 계산한다. 품질 향상 또는 품질 비열등+비용 절감이 확인된 범위에 적용한다.

학습에는 `어떤 결정/방법을 썼는가 → 어떤 환경에서 어떤 평가가 나왔는가`를 연결한다. 여러 방법을 동시에 바꿨으면 묶음 효과로 갱신한다. 기억 검색·적용 조건·실행 방법을 따로 비교하면 어느 부분을 바꿔야 하는지도 알아낼 수 있다.

## 9. 개선 방법 자체가 발전하는 과정

개선기는 `경험 묶는 방식·후보 생성법·구조 변이·탐색 폭·실험 배분·종료 규칙`으로 구성된다. 이것도 version을 가진 프로그램이다.

```text
meta_improve(search_history):
  bottlenecks ← locate(repeated_invalid_candidates, duplicate_search,
                       poor_transfer, high_evaluation_cost)
  optimizers ← generate_optimizer_changes(bottlenecks)
  for O in incumbent + optimizers:
    for independent_family_seed in meta_suite:
      policies ← run_search(O, same_starting_data, same_total_budget)
      freeze(policies)
      gain ← per_dimension_difference(outer_evaluation(policies), baseline_outer_evaluation)
      record(gain, coverage, generation_cost + evaluation_cost)
  choose quality_improving or noninferior_cheaper optimizer
  apply to next search jobs
```

예: 반례를 먼저 만드는 탐색, 실패 유형별로 분리해 생성하는 탐색, 서로 다른 구조를 먼저 시도하는 탐색을 비교한다. 단순 생성 횟수가 아니라 **같은 총비용으로 실제 다음 작업을 얼마나 더 잘하게 했는지**가 선택 기준이다. 새 생성/탐색 프로그램도 7절의 합성 대상으로 들어간다.

## 10. 두 루프가 하나의 엔진에서 연결되는 지점

```text
현재 Run: model_decide → execute → observe → adapt → 다음 판단
종료 후: Episode → generalize → synthesize → compare → 새 기본 정책
다음 Run: 새 정책 + 현재 상황에 맞는 경험 → 동일 실행 루프
개선 작업 종료 후: SearchEpisode → meta_improve → 다음 개선 방식
```

RunCoordinator는 현재 상태와 실제 행동을 소유한다. PolicyRegistry는 다음 Run의 기본 방법을 공급한다. 현재의 계획/도구/모델 변경은 local adaptation으로 기록하고, 그것을 다음 작업의 기본값으로 만드는 결정은 비교 결과로 한다.

Rust 구현 순서는 **상태와 관측을 갖춘 공통 실행 루프 → 실행 중 적응 → 조건부 경험 추출·재사용 → 프로그램 합성·비교 → 개선기 자체의 비교**다. 기존 Rust runtime의 호출 경로 안에서 연결한다. 새 엔진이나 타 프로젝트 엔진을 옆에 붙이지 않는다.

구현 인수는 파일/디렉터리 수가 아니라 한 실제 작업의 trace로 판단한다: **실패 관측 → 새 방법 생성 → 실행 결과로 수정 → 결과 평가 → 경험 일반화 → 다른 작업에서 재사용**이 끊기지 않아야 한다.

## 11. 구현할 계약과 순서

| 순서 | 현재 연결점 | 작성할 동작 / 완료 결과 |
| --- | --- | --- |
| 1 | `chat/runtime.rs::run_turn_body`, `dispatch_tool_call` | 3~4절의 상태·관측·적응을 현재 loop에 연결. 반복 실패 후 실제 다른 행동이 실행됨 |
| 2 | `experience.rs::retrieve`, `context_block_with_manifest`, runtime finalization | 6절의 규칙 추출·조건 판정·반례 갱신. 새 작업의 판단에 선택된 경험 revision이 들어감 |
| 3 | `policy.rs::propose_candidates`, `register`, 기존 procedure 실행 경로 | 7절의 typed 프로그램 생성·해석·반례 수정. outline on/off 설정 변경을 넘어 새 제어 흐름이 실행됨 |
| 4 | 기존 실험 runner와 `policy.rs::apply_experiment_result` | 8절 비교를 통과한 방법을 scope별 활성 revision으로 적용. 다음 Run에서 실제 소비 |
| 5 | 기존 개선 작업 scheduler·실험 기록 | 9절 개선기 비교. 다음 search에서 선택된 생성/탐색 revision을 실제 사용 |

기존 runtime을 다른 loop로 복제하지 않는다. 모델·절차·자식의 행동은 현재 공통 도구 dispatch로 들어가고, 새 기능마다 따로 실행 통로를 만들지 않는다.

### 데이터와 모델 출력

```text
Observation = {id, runId, stepId, operationId, status, resultRef, stateVersion}
Hypothesis = {id, claim, evidenceIds, predictions:[predicate→expectedValue]}
Intervention = {id, hypothesisIds, preconditions, program, expectedChecks, estimatedCost}
Strategy = {id, revision, applicability, program, supportingCases, counterexamples, effectByEnvironment}
SearchEpisode = {optimizerRevision, generatedCandidates, trials, selectedPolicy, allCosts}
```

현재 native tool 호출 형식은 유지한다. 진단/합성이 필요한 호출만 위 구조의 JSON 출력을 요구하고 잘못된 필드·참조를 명시적 오류로 돌려 모델이 교정하게 한다. JSON 객체가 생긴 것만으로 실행/학습 완료로 취급하지 않는다.

`Program`의 CallTool은 `{toolId, schemaRevision, inputs, saveAs}`, Branch는 `{predicate, then, else}`, Loop는 `{predicate, body, resourceBudget}`를 갖는다. 입력은 literal 또는 `{from: observationId, path: JSON-pointer}`로 전달한다. 문자열 eval 대신 typed 참조를 해석한다.

predicate 초기 집합은 exists/equals/compare/all/any/not이다. 없는 값은 Unknown이고, Branch는 이 경우 해당 값을 얻는 probe나 모델 판단으로 돌아간다. 생성기는 새 predicate 구현을 가정하지 않고 관측 가능한 특징에 연결한다.

프로그램 해석기는 `programRevision, frameStack, resolvedInputs, completedOperationIds, pendingOperationIds`를 checkpoint에 저장한다. 결과를 기다리는 동안 worker를 해제하고, 재개 시 완료된 연산은 건너뛰며 분기 입력부터 이어 간다. 프로그램 실패를 단순 종료 처리하지 않고 4절 적응 입력으로 돌린다.a

### 딥시크가 보여 줘야 할 결과

한 흐름으로 보여라: **검색 실패 → 새 검색/확인 프로그램 생성 → 실제 실행 → 결과에 따라 수정 → 작업 평가 → 조건부 경험 추출 → 다른 자료/환경에서 재사용**.

별도로 확인할 것은 단순 대화의 불필요한 호출 증가 여부, changing-state polling과 무진전 반복의 구분, 재시작 후 프로그램 위치 복원, 경험 조건 불일치 시 재판단, 실험 결과의 다음 실행 반영이다. fixture로는 연결을, 실제 모델·새 과제로는 문제 해결과 전이를 확인한다.

인계 결과는 수정한 코드·이 실행 trace·검사 결과·남은 항목으로 제출하라. 후보 저장/문구 주입만 하고 엔진 동작이 그대로면 구현 미완료다.
