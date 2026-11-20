#!/usr/bin/env bash
# This is a source-architecture check, not a substitute for model/task benchmarks.
set -euo pipefail
required_count() {
  local file="$1" pattern="$2" expected="$3"
  local observed
  observed="$(grep -Ec "$pattern" "$file" || true)"
  if [[ "$observed" -ne "$expected" ]]; then
    echo "ERROR: $file expected $expected matches of [$pattern], observed $observed" >&2
    exit 1
  fi
}
# Only the strict provider-view projection is a supported production surface.
required_count src/context.rs '^[[:space:]]*pub fn compact_strict\(' 1
required_count src/context.rs '^[[:space:]]*pub fn compact\(' 0
# Native Run, its single real tool dispatch, and durable journal retain control.
required_count src/engine.rs '^[[:space:]]*async fn drive\(' 1
required_count src/engine.rs '^[[:space:]]*async fn perform_tool\(' 1
required_count src/journal.rs '^[[:space:]]*pub fn begin\(' 1
required_count src/journal.rs '^[[:space:]]*pub fn finish\(' 1
# Prevent split admission contracts and duplicate credential-free metadata fetchers.
required_count src/frontend_rooms.rs '^[[:space:]]*async fn admit_owner_message' 1
required_count src/frontend_rooms.rs 'engine[.]groups[.]say_once' 1
required_count src/frontend_public_http.rs '^[[:space:]]*pub[(]crate[)] async fn read_json' 1
required_count src/frontend_huggingface.rs 'reqwest::Client::builder' 0
required_count src/frontend_provider_public.rs 'reqwest::Client::builder' 0
# Retired experiment executors and UI routes must not return.
required_count src/tools.rs 'compare_strategies' 0
required_count src/config.rs 'strategy_trials_enabled' 0
required_count src/frontend_growth.rs '"/optimizers"' 0
required_count src/frontend_growth.rs '"/experiment-requests"' 0
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
# The execution Engine must own native Run state, not application products.
required_count src/engine.rs 'product: crate::product::ProductHost' 1
required_count src/engine.rs 'pub groups:|pub addons:|pub knowledge:|pub frontend:|pub providers:|pub owner_profile:' 0
required_count src/engine.rs 'frontend_local_models::|provider_registry::|frontend_state::|Knowledge::open' 0
required_count src/product.rs 'pub struct ProductHost' 1
required_count src/product.rs 'pub fn model_for_selection' 1
required_count src/product.rs 'async fn focused_notes' 1
# Optional integrations must never merge individually into the core API.
required_count src/frontend_api.rs 'merge\(crate::addons::routes\(addons_enabled\)\)' 1
required_count src/frontend_api.rs 'merge\(crate::frontend_channels|merge\(crate::frontend_extensions|merge\(crate::frontend_oauth' 0
required_count src/engine.rs 'pub addons: Arc<crate::addons::Host>' 1
required_count src/frontend_rooms.rs 'crate::addons::dispatch_room' 3
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
echo "PASS: one native Run/tool executor, one durable journal, one strict context projection"
