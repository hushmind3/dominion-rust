#!/usr/bin/env bash
# This is a source-architecture check, not a substitute for model/task benchmarks.
set -euo pipefail
required_count() {
  local file="$1" pattern="$2" expected="$3"
  local observed status=0
  observed="$(grep -Ec "$pattern" "$file")" || status=$?
  if [[ "$status" -gt 1 ]]; then
    echo "ERROR: invalid architecture-check regex [$pattern] in $file" >&2
    exit 1
  fi
  if [[ "$observed" -ne "$expected" ]]; then
    echo "ERROR: $file expected $expected matches of [$pattern], observed $observed" >&2
    exit 1
  fi
}
# Only the strict provider-view projection is a supported production surface.
required_count src/algorithms/context.rs '^[[:space:]]*pub fn compact_strict\(' 1
required_count src/algorithms/context.rs '^[[:space:]]*pub fn compact\(' 0
# The single executable ReAct loop is outside the engine source.
if [[ -e src/algorithms/runner.rs ]]; then
  echo "ERROR: a mandatory runner cannot live in optional strategies" >&2
  exit 1
fi
required_count src/harness/mod.rs '^mod runner;' 1
required_count src/harness/runner.rs 'LoopStep::ModelTurn' 1
required_count src/engine.rs 'LoopStep::|TurnSnapshot' 0
required_count src/engine.rs 'pub[(]crate[)] loop_driver:Arc<dyn' 1
required_count crates/dominion-agent-core/src/lib.rs 'pub trait AgentLoop' 1
required_count crates/dominion-agent-algo/src/lib.rs 'impl AgentLoop for ReActLoop' 1
# Native Run, its single real tool dispatch, and durable journal retain control.
required_count src/engine.rs '^[[:space:]]*async fn drive\(' 0
required_count src/harness/runner.rs '^[[:space:]]*pub[(]crate[)] async fn drive\(' 1
required_count src/engine.rs '^[[:space:]]*pub[(]crate[)] async fn perform_tool\(' 1
required_count src/journal.rs '^[[:space:]]*pub fn begin\(' 1
# The kernel owns business-state transitions, not the DB or filesystem
# durability implementation. File I/O is an infrastructure adapter.
required_count src/engine.rs 'pub store: Arc<RunStorePort>' 1
required_count src/engine.rs 'store::Store|Store::open|tokio::fs|std::fs|persist\(' 0
required_count src/sdk.rs 'Store::open|store::Store' 0
required_count src/ports.rs 'pub trait RunRepository' 1
required_count src/ports.rs 'tokio::fs|std::fs|persist\(' 0
required_count src/infrastructure/persistence/mod.rs 'impl crate::ports::RunRepository for Store' 1
required_count src/infrastructure/persistence/mod.rs 'async fn persist\(' 1
required_count src/product/bootstrap.rs 'Store::open' 1
if [[ -e src/store.rs || -e src/store/tests.rs ]]; then
 echo "ERROR: concrete DB implementation leaked into execution root" >&2
 exit 1
fi

required_count src/journal.rs '^[[:space:]]*pub fn finish\(' 1
# Prevent split admission contracts and duplicate credential-free metadata fetchers.
required_count src/product/frontend_rooms.rs '^[[:space:]]*async fn admit_owner_message' 1
required_count src/product/frontend_rooms.rs 'engine[.]groups[.]say_once' 1
required_count src/product/frontend_public_http.rs '^[[:space:]]*pub[(]crate[)] async fn read_json' 1
required_count src/product/frontend_huggingface.rs 'reqwest::Client::builder' 0
required_count src/product/frontend_provider_public.rs 'reqwest::Client::builder' 0
# Retired experiment executors and UI routes must not return.
required_count src/infrastructure/local_tools.rs 'compare_strategies' 0
required_count src/product/config.rs 'strategy_trials_enabled' 0
required_count src/product/frontend_growth.rs '"/optimizers"' 0
required_count src/product/frontend_growth.rs '"/experiment-requests"' 0
required_count web/src/i18n/errors.ts '영상' 0
# Do not bring back retired account screens, phantom login buttons, or OAuth app editors.
required_count web/src/pages/Connections.tsx '<ConnectedAccounts/>' 1
required_count web/src/pages/Connections.tsx 'oauthEditor|quick-connect|openOAuth' 0
required_count web/src/api.ts 'workspace=true' 1
required_count web/src/api.ts 'repository=true' 1
required_count src/addons/oauth.rs 'route.*oauth/providers/\{id\}' 0
required_count src/addons/github_cli_login.rs 'route.*oauth/github/cli/login' 0
required_count web/src/components/DominionCloudAccount.tsx 'disabled=\{!socialLogin\.' 0
required_count web/src/pages/Models.tsx 'startCliLogin|importCli\(' 0
required_count src/sdk.rs 'frontend_local_models::|providers[.]get\(' 0
required_count src/sdk/product.rs 'pub fn http_router' 1
required_count src/sdk.rs 'pub fn http_router' 0
for old in src/api.rs src/build_identity.rs; do
 if [[ -e "$old" ]]; then
  echo "ERROR: HTTP/build metadata must not live at Run-kernel root: $old" >&2
  exit 1
 fi
done
# The root is the instruction/Run/kernel contract, not a feature dumping ground.
# New application components belong in src/product/, external integrations in
# src/addons/. A *new core primitive* must be explicitly reviewed here.
if [[ -e src/product.rs ]]; then
 echo "ERROR: product host must live under src/product/mod.rs" >&2
 exit 1
fi
for candidate in src/*.rs; do
 case "${candidate##*/}" in
 acceptance.rs|domain.rs|engine.rs|hooks.rs|journal.rs|lib.rs|main.rs|policy.rs|sdk.rs|ports.rs|runtime_services.rs|tool_batch.rs|tool_schema.rs) ;;
 *) echo "ERROR: unexpected root module $candidate (classify core vs product/addons)" >&2; exit 1 ;;
 esac
done
# Model transport, OS tools and opt-in program interpreter are adapters.
for moved in src/provider.rs src/tools.rs src/workspace.rs src/sandbox.rs \
 src/config.rs src/read_program.rs src/program_ir.rs src/program_session.rs; do
 if [[ -e "$moved" ]]; then
  echo "ERROR: adapter code leaked into native execution core: $moved" >&2
  exit 1
 fi
done
required_count src/infrastructure/local_tools.rs 'pub async fn execute' 1
required_count src/addons/read_program.rs 'pub async fn execute_with_hooks' 1
# Reasoning, planning, memory projection and behavior selection are pluggable.
for moved in context tool_discovery repair_guidance loop_guard verification; do
 if [[ -e "src/$moved.rs" || ! -f "src/algorithms/$moved.rs" ]]; then
  echo "ERROR: algorithm must live under src/algorithms/: $moved" >&2
  exit 1
 fi
done
if [[ -e src/engine/local_task_catalog.rs || ! -e src/algorithms/local_task_catalog.rs ]]; then
 echo "ERROR: tool-presentation heuristic belongs to algorithms, not Engine" >&2
 exit 1
fi
required_count src/engine.rs 'algorithm:Arc<dyn AgentAlgorithm>' 3
required_count src/algorithms/mod.rs 'pub trait AgentAlgorithm' 1
required_count src/algorithms/mod.rs 'fn fused_messages' 2
required_count src/algorithms/mod.rs 'fn initialize_fused' 0
required_count src/engine.rs 'Self::admit_algorithm_catalog' 0
required_count src/harness/runner.rs 'Self::admit_algorithm_catalog' 1
required_count src/engine.rs 'fn admit_algorithm_catalog' 1
required_count src/engine.rs 'const SYSTEM|local_task_catalog::|tool_discovery::|compact_execution[(]' 0
required_count src/product/bootstrap.rs 'pub fn new_with_algorithm' 2
required_count src/product/bootstrap.rs 'pub fn new_with_runtime_services' 1
# The Run kernel must not know a remote transport or duplicate progress loop.
required_count src/engine.rs 'McpProgress|McpCallOutcome|call_with_progress|starts_with\("mcp__"\)' 0
required_count src/addons/execution.rs '^pub async fn invoke\(' 1
required_count src/addons/execution.rs 'journal::begin|journal::finish|pub async fn drive' 0
# The execution Engine must own native Run state, not application products.
required_count src/engine.rs 'product: Arc<crate::product::ProductHost>' 1
required_count src/engine.rs 'pub[(]crate[)] services: Arc<dyn RuntimeServices>' 1
required_count src/engine.rs 'self[.](knowledge|addons|providers)[.]' 0
required_count src/harness/runner.rs 'self[.](knowledge|addons|providers)[.]' 0
required_count src/runtime_services.rs 'pub trait RuntimeServices' 1
required_count src/runtime_services.rs 'fn complete' 1
required_count src/runtime_services.rs 'pub enum CapabilitySupport' 1
required_count src/harness/runner.rs 'model_capability_denied' 1
required_count src/product/runtime_services.rs 'fn model_capabilities' 1
required_count src/product/runtime_services.rs 'fn complete' 1
required_count src/harness/runner.rs 'selected[.]complete|selected_model[.]complete|model_for_run' 0
required_count src/product/runtime_services.rs 'impl RuntimeServices for ProductHost' 1
required_count src/engine.rs 'pub groups:|pub addons:|pub knowledge:|pub frontend:|pub providers:|pub owner_profile:' 0
required_count src/engine.rs 'frontend_local_models::|provider_registry::|frontend_state::|Knowledge::open' 0
required_count src/product/mod.rs 'pub struct ProductHost' 1
required_count src/product/mod.rs 'pub fn model_for_selection' 1
required_count src/product/mod.rs 'async fn focused_notes' 1
# GitHub collaboration/skills/strategy evidence are optional adapters, not
# additional Run schedulers or core-owned product features.
for entry in src/github_collab.rs src/skills.rs src/strategy.rs src/strategy; do
 if [[ -e "$entry" ]]; then
  echo "ERROR: optional service leaked back into Run kernel root: $entry" >&2
  exit 1
 fi
done
# Product-specific web service adapters must not reappear under src/.
for old in src/frontend_*.rs src/frontend_api src/frontend_rooms; do
 if [[ -e "$old" ]]; then
  echo "ERROR: product adapter leaked back into core root: $old" >&2
  exit 1
 fi
done
# First-party product state and room logic are not kernel-root modules.
for moved in group knowledge profile provider_registry frontend_state; do
 if [[ -f "src/$moved.rs" || ! -f "src/product/$moved.rs" ]]; then
  echo "ERROR: product service must live under src/product/$moved.rs" >&2
  exit 1
 fi
done
# Product-facing SDK methods are not part of the Run-control SDK.
required_count src/sdk.rs 'pub async fn (chat_sessions|create_group|remember|tool_catalog)\(' 0
required_count src/sdk/product.rs 'pub async fn (chat_sessions|create_group|remember|tool_catalog)\(' 4
# Optional integrations must never merge individually into the core API.
required_count src/product/frontend_api.rs 'merge\(crate::addons::routes\(addons_enabled\)\)' 1
required_count src/product/frontend_api.rs 'merge\(crate::frontend_channels|merge\(crate::frontend_extensions|merge\(crate::frontend_oauth' 0
required_count src/product/mod.rs 'pub addons:Arc<addons::Host>' 1
required_count src/product/group.rs 'pub struct RoomReadCursor' 1
required_count src/product/group.rs 'room[.]read_cursors[.]insert[(]participant[.]name' 1
required_count src/product/group.rs 'room[.]read_cursors[.]clear' 1
required_count src/product/coordination.rs 'unread_range[(]' 1
required_count src/product/frontend_rooms.rs 'crate::addons::dispatch_room' 3
required_count web/src/pages/Connections.tsx 'lazy\(\(\)=>import\(' 4
for legacy in src/mcp.rs src/mcp/tests.rs src/frontend_channels.rs src/frontend_extensions.rs \
 src/frontend_oauth.rs src/frontend_skill_import.rs src/connected_tools.rs \
 src/github_work.rs src/google_workspace.rs src/github_cli_login.rs; do
 if [[ -e "$legacy" ]]; then
  echo "ERROR: optional integration source leaked into core root: $legacy" >&2
  exit 1
 fi
done
# Do not resurrect an independent agent run loop or retired media/RSI executor.
for obsolete in \
  src/engine/chat/runtime.rs \
  src/rsi.rs \
  src/video.rs \
  src/video \
  src/engine/rsi.rs \
  src/money \
  web/src/pages/Money.tsx \
  crates/dominion-agent-sdk \
  crates/harness; do
  if [[ -e "$obsolete" ]]; then
    echo "ERROR: duplicate or retired execution implementation reintroduced: $obsolete" >&2
    exit 1
  fi
done

# Owner-authored Task Teams reuse the ONLY native Run executor.
required_count src/domain/events.rs 'pub struct UnifiedEvent' 1
required_count src/domain/team.rs 'pub struct TeamGraph' 1
required_count src/engine/task_team.rs 'pub async fn advance_task_team' 1
required_count src/addons/agent_bridge.rs 'pub const TOOL:&str="external_agent_consult"' 1
required_count src/addons/mod.rs 'agent_bridge::TOOL[|]agent_bridge::EDIT_TOOL' 3
required_count src/addons/mod.rs 'remote_worker::TOOL' 3
required_count src/addons/execution.rs 'host[.]remote_worker[.]as_ref' 1
required_count src/product/remote_worker.rs 'run[.]read_only=!req[.]writer' 1
required_count src/product/remote_worker.rs 'writer_authorized[(]&headers[)]' 1
required_count src/policy.rs 'remote_worker_admitted' 2
required_count src/addons/remote_worker.rs 'remote_writer_patch_apply_intent' 1
required_count src/infrastructure/persistence/mod.rs 'pub async fn put_if_absent' 1
required_count src/addons/execution.rs 'host[.]bridge[.]execute' 1
required_count src/addons/agent_bridge.rs 'run[.]event[(]"external_agent_unmetered"' 1
required_count src/addons/agent_bridge.rs 'kill_on_drop[(]true[)]' 1
required_count src/addons/agent_bridge.rs 'Command::new[(]"(sh|bash)"[)]|dangerously-skip-permissions' 0
# The only write-capable external bridge is an approved isolated Team Writer.
required_count src/addons/agent_bridge.rs 'verified_external_writer_worktree' 1
required_count src/addons/agent_bridge.rs 'if editing[{]"workspace-write"[}]else[{]"read-only"[}]' 1
required_count src/engine/task_team.rs 'self[.]launch[(]task[.]run_id' 2
required_count src/engine/task_team.rs 'impl Engine' 1
required_count src/engine/task_team.rs 'async fn drive[(]|fn perform_tool[(]' 0
required_count src/engine.rs 'run[.]team[.]is_some[(]' 3
# Prevent dead pre-migration facades and mock-only handlers returning as live API.
required_count src/product/frontend_api/agents.rs 'async fn (model_runtime|model_installed|channels)\(' 0
required_count src/engine.rs '^[[:space:]]*pub[(]crate[)] fn from_ports\(' 0
required_count src/addons/agent_bridge/app_server.rs '^[[:space:]]*pub[(]super[)] async fn consult<' 0
required_count src/infrastructure/model_transport.rs 'strip_internal_cache_markers' 0
required_count src/product/coordination.rs 'pub fn (completed|total|can_schedule)\(' 0
echo "PASS: one native Run/tool executor, one durable journal, one strict context projection"
