# 서브3 — 상호 지능 개선의 실제 세대 계승 (2026-11-12)

## 심판 피드백에 대한 구조적 대응

이전 서브3는 한 세대의 패치가 여러 동결 평가 영역에 영향을 미쳤는지를 학습했다. 그러나 A를 바꾼 뒤 **A가 만든 새 후손이 B를 실제로 개선했는지**, 이어서 **B를 개선한 후손이 A를 다시 개선했는지**는 메타 정책의 별도 입력이 아니었다. 서브4의 \`rsi_symbiosis.rs\`는 이 경로를 명시적으로 모델링했다.

서브3에서는 한 세대의 독립 A/B 관측 (\`rsi_interaction.rs\`)에 **연속 후손의 실제 변경·채택 증거** (\`rsi_mutual_program.rs\`)를 추가했다. 메타 개선기 코드가 두 계층의 증거를 동시에 사용한다.

## 실제 실행 경로

1. **재컴파일된 후손 계획기**: \`rsi_evolve::plan_generation_from_file\`은 현재 소스 인덱스에 기초해 전체 영역 후보를 가져온다. \`rsi_meta_controller::synthesize\`는 한 세대의 영역 간 변화와 자식 세대의 이종 영역 채택을 함께 평가한다.
2. **수정 가능한 세대 간 프로그램**: \`rsi_mutual_program::propose\`는 \`seed → complement → return\` 중 현재 단계를 결정한다. 이전 단계에서 실제 수정·독립 평가·실험상 채택된 커밋이 없으면 다음 단계로 넘어가지 않는다. 각 단계의 경로와 SHA는 새 후손 소스에 다시 묶는다.
3. **후손이 선택 알고리즘 자체를 변경**: \`rsi_meta_controller::steer_reciprocal\`은 후손의 \`metaPolicy.target\`, 예측, 탐색 모드를 스스로 바꾸고 정책 전체 SHA를 재계산한다. 고정된 외부 코디네이터가 후손의 선택 우선순위를 다시 계산하지 않는다.
4. **실행 정합성**: 부모는 고정 소스·세대 수·모델 정책·결합 소스 경로와 해시를 점검한다. \`rsiMutualProgram\`은 재컴파일된 worker까지 전달된다. 실제 worker가 이 연결을 받아 실행한 경우에만 \`compiledPlanExecuted.source=running_compiled_descendant_binary\`가 기록된다.
5. **부정적 결과도 다음 정책에 전달**: \`pairedMeasured\`와 \`pairedRevalidated\`를 구분한다. 독립 평가에서 실패했더라도 유효한 재생 가능 기록이면 다음 메타 학습이 이를 무시하지 않는다. 채택되지 않은 후보는 다음 세대의 개선 성과를 만들지 못한다.
6. **실제 소스 변경 기준의 영향 귀속**: \`rsi_interaction::actual_roles\`는 의도한 역할이나 프롬프트의 \`focusFiles\`가 아닌 \`changedSourcePaths\`와 실제 \`componentFiles\`를 사용한다. 복수 구성요소 동시 개입은 독립 인과 증거가 아니라고 표시한다.
7. **수명 전체의 계승 기록**: \`rsi_mutual_program::completed_cycles\`는 실제 채택 커밋 \`A → B → A\`가 연속적으로 연결돼 있는지를 확인해 \`rsi_system::handoff_evidence\`의 \`acceptedReciprocalRevisions\`로 별도 보고한다. 단순 계획을 완료된 개선으로 세지 않는다.

## 능력의 확장과 안전한 외부 경계

후손 바이너리에서 \`rsi_mutual_program.rs\`, \`rsi_meta_controller.rs\`, \`rsi_interaction.rs\`, 소스 탐색기, 하네스 및 도구 프로그램 모두 다시 쓸 수 있다. 따라서 세대의 변경 방식, 역할 관계, 개선 대상, 목표 선정 방식은 후손의 새 코드를 따를 수 있다.

동결 평가기·원격 모델 비용 상한·사용자 생산 반영 권한은 기존 감독기에 남는다. 독립 평가가 성공했더라도 그 사실만으로 구성요소 간 직접 인과관계가 확정되지는 않는다.

## 최신 외부 비교의 출발점

- [Meta^n — Recursive Self-Improvement through Emergent Depth](https://arxiv.org/abs/2608.24735): 이전 개선층의 실행 흔적과 실제 코드를 후속 층의 입력으로 사용한다.
- [MetaSkill-Evolve](https://huggingface.co/papers/2607.05297): 빠른 작업 능력과 느린 개선 방법 능력을 모두 진화시킨다.
- [The Path to Recursive Self-Improving Agents](https://github.com/D2I-ai/awesome-recursive-self-improving-agents): 모델·하네스·자료·학습기·개선기를 하나의 갱신 가능한 시스템으로 보도록 구조화한다.
- 최신 서브4 \`rsi_symbiosis.rs\`: A→B→A 단계를 명시하고 후손 채택을 기준으로 전이를 기록한다. 서브3는 여기에 실측 음성 결과, 개별 변경 파일 귀속, 수정 가능한 메타 보상과 독립적인 3채택 완료 증명을 연결했다.

## 검증 경계

이번 턴은 **소스 구현과 GitHub 푸시만** 수행했다. 사용자의 요청에 따라 CI·컴파일·유료 모델 호출·완주 실험은 수행하지 않았다. \`A→B→A\`는 이제 **실행 가능한 계획과 증거 수집 규칙**이며, 실제 완료된 개선 순환이라는 실증 주장은 하지 않는다.
