# Sub2 · Issue #9: cross-menu product completion path

**Date:** 2026-11-14 · **Branch:** `dominion-sub2` · **Previous independent score:** 62/100.
The rating is from **before** these modifications. There is no new judge score and no asserted increase in live-model pass rate, billed savings or video quality.

## One useful task through multiple menus

```text
Connections > Installed Skills > activate
    web/src/pages/extensions/SkillsPanel.tsx
      "에이전트에 배정" -> #/agents?skill=<id>
       |
Agents > assign at most five active skills
    web/src/pages/Agents.tsx -> POST /api/agents
    api/agents.rs: reject disabled, draft, deleted or repeated skill IDs
    persist agent.skillIds in SQLite
       |
Chat / Group > select the agent
    chat/context.rs::context_block_with_manifest
    memory::visible AND active skill AND agent.skillIds membership
    -> only that agent receives its selected instructions in the
       actual model input context; scope / tenant boundaries unchanged
       |
Agent card -> "이 에이전트로 검증 워크플로 만들기"
       |
Workflows > multi-agent steps + explicit file/tool acceptance criteria
    web/pages/Workflows.tsx -> POST /api/workflows
    api/collections.rs: registered agent IDs + valid checks, max 8 / step
    POST /api/workflows/:id/run -> job
    engine/workflows.rs:
        frozen per-job step definition and in-flight checkpoint
        run_quiet_outcome -> executed model and tool calls
        execution_operation -> durable, session-scoped evidence
        verification::acceptance_gaps_for_workspace
        session_has_uncertain_effects -> fail closed if unknown
        ONLY on valid proof -> next agent receives {{previous}}
        not verified -> needs_review, no next step, preserve checkpoint
        output includes per-step verification + model token deltas
       |
Workflows > "반복 실행 등록" -> #/automations?workflow=<id>
       |
Automations > choose saved workflow + input + interval
    POST /api/automations -> api/collections.rs validates flow
    jobs.rs::schedule_automations
    db/automation_claim.rs::Store::claim_due_automation:
        SQLite transaction: due-slot current value + enabled +
        running/queued overlap guard + INSERT new job +
        update nextAt / lastJobId / lastAt COMMIT together
        -> no duplicate scheduled job after retries or crashes
       |
Home / Activity / Workflows:
    job status, latest automation run, actual per-step verification,
    completed-step outputs, recorded tool operation IDs and token usage,
    incomplete verification gaps -> inspect/re-run manually
```

**New capability, not a new menu:** Installed reusable skills now determine the actual model context of specific agents. Those agents can pass checked artifacts through multi-agent workflow steps. The same verified workflow can run periodically, with its completed job traced across the home/workflow/activity surfaces. The Developer tool can start a checked workflow against the exact same engine workspace, passing the real Git branch, HEAD and changed-file list as input. Growth displays recorded per-agent verified steps and token usage, without inventing a learning gain.

**Backwards compatibility:** Agents without a `skillIds` field retain the legacy shared-skill context; opening and saving an edited agent gives it an explicit selection (empty means none). Existing workflow steps without acceptance checks remain conversational with `verification.status=unchecked`. The scheduler preserves old `prompt` and `meeting` kinds and their existing engine dispatch.

**Reliability:** No workflow step with explicit checks hands an unverified output to the next agent. Pending external effects halt the chain. Each scheduled slot atomically admits a job and advances its nextAt marker, with a guard against overlapping queued or running work of the same automation. SQLite injection tests confirm rollback if the automation update fails after job insert. The original job and receipt persist for inspection.

## 15-menu audit for Issue #9

| Menu | Existing user function | New sub2 contribution in this iteration | Actual call path / proof state | Next gap |
|---|---|---|---|---|
| Home | Session/provider/job/channel status | Live model→skill→agent→verified workflow→automation readiness, actionable next task, checked output summaries | `Home.tsx` reads actual collections/jobs; web typecheck/build evidence only | Guided OAuth/model setup, real completion metrics |
| Chat | Model, tools, owner approval, memory | Agent-bound enabled skill subset affects system prompt | `Agents.tsx → api/agents.rs → chat/context.rs → model`; Rust context regression | Live multi-provider success/error trial |
| Group conversation | Multiple agents, shared turn context | Same per-agent skill assignment applies in group | Group request `agent_id` → `chat/context.rs`, regression on scope; no new group coordination planner | Verified joint artifact merge |
| Models | Providers, local/remote/CLI | Inherited, unchanged this iteration | Existing provider resolution; not claimed new | Real router memory, latency and KRW cost measures |
| Agents | Roles and model selection | Persisted assigned active skill IDs and direct verified-workflow entry | `Agents.tsx → POST /api/agents → Store → chat context` | Skill efficacy per task and permission profiles |
| Video | LTX/Comfy scenes, clip receipts, editing | Inherited, unchanged this iteration | Existing video engine; no new visual measurements | Subjective quality, motion coherence, voice & actual bills |
| Connections | MCP, installed skill catalog and channels | Active installed skills link directly to binding into an agent | `SkillsPanel.tsx → #/agents?skill=` | Automatic MCP authentication checks, capability health |
| Memory | Scoped recall, active/editable entries, growth | Growth panel now attributes **actual verified workflow artifacts and usage per agent** without presenting these as self-improvement | `Growth.tsx → /api/jobs → job.result.outputs[].verification` plus `memory::visible` | Learning/retention measured across sessions |
| Goal | Planning, verified tool receipts, recovery | Inherited, unchanged this iteration | Existing atomic admission/resume; no new menu credit | Large independently verified task pass@1 |
| Automations | Periodic prompt/meeting tasks | **New workflow scheduling** with exactly-once due-slot admission and running overlap prevention | `Operations.tsx → collections.rs → jobs.rs → db/automation_claim.rs`; SQLite rollback tests | Notifications, retry policy/threshold/owner approval on failure |
| Activity | Jobs, operations, audit | Display scheduled workflow source and verified steps/recorded token counts | `Operations.tsx → /api/jobs` current persisted job result | Actual provider-bill attribution |
| RSI | Isolated candidates, evaluations and descendants | Inherited; no change to frozen independent evaluator | Existing RSI remains separate from workflow success | Genuine multi-generation adopted model-led improvement |
| Settings | Policy, provider accounts, backup | Inherited; policy still gates tool calls | Existing `policy` and backup endpoints | End-to-end restore and bounded spending |
| Workflows | Agent steps and `{{previous}}` passing | **New verified handoff** with server-approved per-step criteria, durable tool IDs, tokens, explicit gaps, links to automation | `Workflows.tsx → collections.rs → workflows.rs → run_quiet_outcome → verification.rs → job result` | Human-approved retry from safe checkpoints, independent real-model runs |
| Developer | Git, diff, preview, job runs | **Verified multi-agent test runs for the attached Git source**, with strict sourceDir===engine.workspace preflight, Git changed file paths and HEAD included as run input | `Developer.tsx → /api/status + /api/dev/diff + /api/workflows/:id/run → workflow job receipts` | Real patch applicability and CI artifact delivery, OS-level path isolation |

## Verification tiers; do not confuse them

- **Static:** Changes are committed to the named sub2 branch, not shared by other branches unless merged. The Developer preflight compares configured folder paths on the client; backend workflow tools still follow their original approvals and workspace restrictions, so this is not an OS security sandbox.
- **Deterministic:** Rust tests check agent-only skill context and an actual mocked-provider workflow path with file writes versus read-only/no-artifact outcomes. Scheduler tests inspect real SQLite transactions, due-slot idempotency, no overlapping jobs and injected rollback. Web typecheck/build validates views; Node independent-oracle tests are inherited.
- **Live:** **No paid API or external model was invoked by the tests above.** The mocked-provider workflow test is native HTTP→engine→tool→SQLite→artifact verification, not a new real-model reliability benchmark.
- **Measured:** Workflow outputs retain `modelCalls`, `totalTokens`, `usageEstimated`, and source operation IDs. Recorded token use can be incomplete/estimated; **no verified KRW bill, savings percentage, or actual audiovisual quality** is attributed.

## First realistic acceptance exercise

1. Connect a local model or a separately budget-limited API provider.
2. In Connections, create/enable one skill; click **에이전트에 배정**.
3. In Agents, configure an agent's model and selected skill. From the card, create a two-step workflow.
4. Step 1 writes `reports/final.md` and has a `file_contains` check for the required line. Step 2 summarizes `{{previous}}` using another agent.
5. Run the workflow; inspect the persisted tool operations, step outputs, and reported token count. If the check fails, verify that step 2 does not execute and review the failed artifact.
6. Choose **반복 실행 등록**, configure the same saved workflow with an explicit interval, inspect subsequent real jobs from Automation and Activity.
7. Compare local vs remote model completion and recorded token usage on the **same** acceptance criteria. Treat missing provider data and real provider invoice as separate concerns.

See [Issue #9](https://github.com/hushmind3/dominion-rust/issues/9) for the complete product acceptance objectives.
