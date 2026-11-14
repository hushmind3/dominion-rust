# DOMINION · 2026-11-14 통합 업그레이드 소스 경로

## 엔진 → 하네스 → 알고리즘 통합 체크

- 진입점: `src/main.rs` → `src/api/rooms.rs` / `src/api/chat.rs` / `src/api/video.rs` / `src/api/rsi.rs`
- 공통 소유권·실행: `src/engine/jobs.rs`, `src/engine/chat/runtime/turn.rs`, `src/engine/chat/continuity.rs`
- 그룹: `api/rooms.rs::submit_internal` → `algorithms::choose_for_agent("multiagent.economy")` → 동결 `roomSnapshot.economyKernel` → `rooms/routing.rs::speaker_window` → `rooms.rs::run_locked` → `routing::tiered_model_pool` → `run_room_quiet` → `gateway::record_usage_tokens`.
- 사용자 결과 평가: `Rooms.tsx::rate` → `POST /api/rooms/:id/jobs/:job_id/feedback` → `api/rooms.rs::feedback` → 멱등 `room_feedback` → `algorithms::record_owner_feedback` → 전역 `algorithm_stat` → 다음 `choose_for_agent` 결정. 주관적 피드백은 독립적 검증 성공으로 취급하지 않습니다. 정책은 저장된 방 스냅샷에서 읽고, 사용자의 명시적 선택이 모든 계층 라우팅보다 우선합니다.
- 영상: `video.rs::enqueue` → `video/film.rs::spec` / `video/director.rs::shot_prompt` → `jobs::enqueue` → `video.rs::run` → `video/film.rs::run` → `video/ltx.rs::run` → `FFmpeg`. 요청별 과금 횟수 확인, 개별 제출 불확실성 기록, 완료 클립 재사용.
- RSI: `api/rsi.rs` → `jobs.rs` → `rsi/execution/campaign.rs` → `rsi/planner/descendant.rs` → `rsi/execution/worker.rs` → `rsi_worktree.rs` → `rsi_method_generalization.rs`, 채택된 소스와 개선법을 다음 세대가 재사용합니다. 모델 가중치를 학습시키지 않습니다.

## 2026년 근거 및 차이

- LTX 공식 2026년 8~11월 Changelog: https://docs.ltx.io/api-changelog
- LTX 2.5 고화질·네이티브 멀티쇼트·프레임별 지원 길이: https://docs.ltx.io/models/ltx-2-5/
- `video/ltx.rs::maximum_duration` 규칙을 `video/film.rs::spec`와 단일 클립 경로에서 같이 참조해 24/25/48/50FPS의 720p~4K 공식 제한을 실행 전 확인합니다.
- ICML 2026 LASE 선택적 에이전트 논의: https://proceedings.mlr.press/v306/yoa26a.html
- ICML 2026 SALE 소형/대형 모델 전략 경매: https://proceedings.mlr.press/v306/alazraki26a.html
- 2026년 프롬프트 캐시 운영 지침: https://developers.openai.com/api/docs/guides/prompt-caching

## 출시 이후 연결 확인 대상

1. LTX 작업 종료/중단/복귀 및 클립별 출력 파일 무결성, 동일 스펙 MP4 오디오 결합·플레이어 재생
2. 새로운 적응형 방과 이전 방의 호출 경로 비교, 사용자 전체 토론 명령의 우선권
3. 프롬프트 캐시 히트율, 실수금/실토큰 기반 비용, 저비용/고성능 티어의 실제 성능 및 품질
4. RSI 채택 → 독립 평가 → 후손 재컴파일·재계획·방법 계승의 다세대 검증
5. 영상 장편 제작의 쇼트 간 인물/조명 일관성은 프롬프트 유도이며 프레임 참조 기반의 완전 보장이 아닙니다.

이번 변경은 구조와 실제 호출 경로의 소스 통합입니다. CI·빌드·실모델·영상 벤치마크는 실행하지 않았으며, 절감률·품질 상승을 실측 성과로 주장하지 않습니다.


## Agent execution integration 2026-11-14
- `goal_context.rs::snapshot` freezes a small project file inventory and selected root docs before the planner. `goals.rs::run` passes the same bounded context to the executor without another API call.
- `run_quiet_outcome` uses existing `code_search`, workspace edits, delegation, browser, GitHub and MCP tools; `verification::acceptance_gaps` and the independent reviewer gate completion.
- `goal_artifacts.rs::collect` returns hashes of actual workspace files written through recorded successful tools. `goals.rs::finish` returns the artifact manifest and history; artifact hashes are not a substitute for test success.
- No compile, benchmark or model run performed for this patch.

- `POST /api/goals/:id/steer` uses a compare-and-swap `planRevision` and applies owner edits only before the next executor turn; a new edit arriving during review prevents completion on an obsolete plan. `POST /api/goals/:id/cancel` marks queued/running jobs and goal records cancelled.

- UI entrypoint: `WorkspaceShell.tsx` `goals` → `App.tsx` `AutonomousGoalsPage` → `POST /api/goals`, `GET /api/goals`, `GET /api/jobs`, `POST /api/goals/:id/steer|cancel`, and the existing chat/developer pages. Server event refresh and active-goal 6-second polling show progress.

- Goal routing uses `router::select_provider` (at least standard complexity) and stores `resolvedExecutor` for durable resume; user-selected executor roles win. `GoalControl::complete` charges planner/reviewer tokens to the same goal session.
- `algorithms::choose_decomposition_for_agent` → `decompose::decompose` → frozen goal `decomposition` → planner/executor → independently verified goal completion → idempotent `goal_policy_reward` → `algorithms::record_owner_feedback("task.decomposition", revision, true)`. Human-edited plans and unverified runs receive no positive reward.

- `engine/goal_evidence.rs::synopsis` bounds retrospective reviewer/replanner evidence tokens without discarding deterministic verifier inputs. Full original evidence is used by `verification::acceptance_gaps` and source artifacts; summaries are explicitly lossy and must not serve as validation records.

- Tool `workspace_diff` is real, scoped read-only Git inspection in `agent_diff.rs`, connected through `tools::definitions`, `tools::execute` and `chat/runtime/tool_selection.rs`. The goal executor prompt requests actual diff review and commands for task-specific verification.

- Parallel research: `tools::definitions("delegate_parallel")` → `tools::delegate_parallel` → `agent_parallel::validate` → each child `delegate` → separate `jobs::enqueue("prompt")` → child `chat/runtime/turn.rs` schema filter and `tools::execute` read-only runtime check → `task_payload` result aggregation → owner-facing goal executor synthesis. No shared writable worktree, no parallel source-write races by this tool.

- A bounded goal diff UI is wired: `AutonomousGoalsPage::showDiff` → `GET /api/goals/:id/diff` → `api/goals.rs::diff_goal` → `agent_diff::inspect` restricted to goal artifact paths. The endpoint never writes and declines to show unrelated files when no goal artifact exists.

- Enforcement is three-layered: child frozen `delegatedReadOnly`, schema pruning in `chat/runtime/turn.rs`, execution gate for built-ins in `tools.rs`, and a distinct MCP dispatch gate in `chat/runtime/tool_dispatch.rs`.


### Autonomous coding, isolation and verified publishing (2026-11-14)

The goal engine is a working, owner-controlled pipeline, not a screenshot-only scaffold:

1. `POST /api/goals` → `jobs::enqueue("goal")` → `goals::run` creates a durable session. The creation UI selects `isolateWorktree` by default for Git coding tasks; API clients can opt in. `goal_git::prepare` freezes the pristine source commit, branch and a dedicated linked worktree under app data, refusing a dirty original repository.
2. `chat/runtime/tool_dispatch::{deterministic_tool,dispatch_tool_call}` resolves `goal_git::workspace_for_session` before native tools and effect-sealing. `tools::execute` prohibits absolute file tools during isolated tasks; `tools::delegate` passes the goal ID to read-only parallel research children. Git isolation does **not** confine arbitrary OS commands: existing command approval is still mandatory.
3. `goal_context::snapshot_in`, `verification::{baseline_checks_in,acceptance_gaps_in}` and `goal_artifacts::collect_in` read the **same isolated worktree** as the executor. Rejected edits cannot pass acceptance by accidentally inspecting the original checkout. The independent reviewer sees bounded real tool evidence and deterministic acceptance gaps.
4. `GET /api/goals/:id/diff` → `goal_git::changes` displays a bounded diff, base commit, untracked files and the full (not clipped) patch SHA-256. `POST /api/goals/:id/apply` → `goal_git::apply_reviewed` requires that exact SHA, no untracked files, a clean unchanged original HEAD, a stopped goal and a passing `git apply --check`. It applies without shell interpolation, then independently checks the source Git diff digest. `POST /api/goals/:id/rollback` removes only the owned worktree after active workers finish.
5. Legacy shared-workspace goals capture `goal_changes::baseline` before tools. `GET /api/goals/:id/changes` and `POST /api/goals/:id/undo` permit SHA-guarded restoration **one attributed tracked file at a time** when the original checkout began clean and HEAD has not moved. Other user changes are not reset.
6. `goals::finish` persists idempotent `goal_receipt` outcomes. `rsi_evolve::owner_goal_feedback::observe` summarizes bounded owner task outcomes and `rsi/planner/context.rs::generation_context` injects them into genuine descendant planning. These observations **are not** independent paired improvement proofs, synthetic RSI reward or model-weight training.

**Video:** `video::enqueue` freezes an `technicalGate` option and checks `ffprobe` availability **before** paid LTX requests (LTX default on; native default off). `video::film::run` checks each completed paid scene's actual media streams, duration, aspect ratio and requested audio before the next charged shot; the compositor retains its prior successful clips for reuse. `video::run` independently probes the final MP4 and records `technicalQuality` with the job result. Quality failures return `needs_review` and preserve the paid output for the owner; there is no unapproved repeat charge. This is a *technical media proof*, not a claim to measure semantic cinematography or aesthetic quality.

**Boundaries:** `src/engine/goal_git.rs` owns linked-worktree lifetime and reviewed apply; `goal_changes.rs` owns guarded shared-file undo; `verification.rs` owns acceptance, not goal storage; `chat/runtime/tool_dispatch.rs` owns permissioned tools/effect receipts; `rsi/learning/goal_feedback.rs` owns aggregate planning signals; `video/quality.rs` owns bounded ffprobe media checks. No LoRA, fine-tune or weight-update path is introduced.


### Goal code navigation and immutable completion proof (2026-11-14)
- Before the first planner/executor request, `goals::run` calls `goal_navigation::collect` on the **effective goal worktree**. The preexisting persistent code-index kernel refreshes source hashes, ranks up to five goal-relevant files and selects bounded verified excerpts; the navigation summary is saved on the goal. No additional model call or unscoped source read is added.
- Goal finalization now calls `goal_receipts::record` once per goal, storing actual execution-operation evidence/action counts, verifier criteria count, reviewer decisions, artifact count, replans, session attribution, token-count signals and worktree base revision. It is explicitly **not** an independent RSI holdout success. The existing RSI research context reads this same receipt stream; the frozen evaluator remains authoritative.


### Actual prompt-cache reads and writes (2026-11-14)
- `chat/provider/cache_usage.rs::observe` parses the supported provider **usage response** (OpenAI-compatible cache read/write and 30-minute/12-hour write lifetime buckets, Anthropic cache-read and cache-creation counts). A cache hint is **not** counted as a hit. Missing fields remain `null`, never an invented zero. No unverified provider hint is emitted.
- `chat/provider.rs::stream_completion_once` audits each exact, actually returned response as `model.cache_usage`. `chat/runtime/turn.rs` aggregates cache-write tokens into per-message `metadata.usage` and room execution `inputContext`, alongside cache reads, total input and ordinary input where both components were reported. Read/write tokens are subsets of total input, never additional billed tokens; without provider pricing and billing receipts this is **token accounting, not claimed currency savings**.
- Current Anthropic documentation supports top-level `cache_control`; OpenAI caches matching prefixes with model-specific lifetimes. We preserve the existing negotiated hints and provider fallback, avoid introducing unsupported model-specific request parameters by default.


### ComfyUI cinematic production and paid re-render restraint (2026-11-14)
- `POST /api/video/jobs` with `engine:"comfyui", cinematic:true` validates an actual template binding `{{prompt}}` and `{{duration}}`, FFmpeg and ffprobe **before queuing**. It freezes a 3–120 second, deterministic 1–15-shot storyboard, seed and provider workflow. Both cheap/local ComfyUI and existing native card production remain available; paid LTX 2.5 is an opt-in alternative.
- `video::run` dispatches cinematic jobs to `video/cinematic.rs::render`, which submits each shot to the existing ComfyUI adapter, stores a bounded storyboard, checks each actual clip with `ffprobe`, preserves verified clips and reuses them without another generation, and stitches a 1080p H.264/AAC MP4. The final file passes the existing independent final technical check before becoming deliverable.
- A rejected clip never automatically gets regenerated: **zero** extra attempts by default. The optional `badShotRetries:1` requires explicit `confirmRetryCosts:true` at enqueue. Only an unequivocally returned clip that failed deterministic technical inspection can consume that retry; its rejected original is preserved. Uncertain remote submission remains `needs_review`. Job result reports total generation attempts rather than invented currency costs. Semantic consistency, viewership and visual appeal still need real assessments.
