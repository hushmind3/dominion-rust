# Sub4 response to five-branch structure audit (2026-11-17)

Target: `dominion-sub4` only. Main and other competition branches were not edited. CI evidence must be tied to the final HEAD; source work is not a frontier-model completion score.

## 1. Durable Store: remove global fsync lock

**Observed before**: `Store::put` and `Store::mutate` held the same `Mutex<HashMap<Uuid,Run>>` through complete Run serialization, file `sync_all`, atomic rename, and directory fsync. An unrelated run could wait for another run's disk write.

**Changed**: a short-lived metadata lock still guards the in-memory Run map, but write transactions now acquire a **per-Run ID gate** first, release the metadata lock before `persist`, and publish the new snapshot + `RunUpdate` only *after* the fsync chain succeeds. The gate covers both `put` and `mutate` for the same Run ID; failed writes never replace visible memory or emit a fabricated status. Existing startup recovery, uncertain-tool receipts, and child settlement invariants remain.

**Limits**: full Run JSON still gets cloned, serialized and fsynced for each mutation, and the metadata map must still be visited for broad history views. This is a **contention fix**, not an incremental journal implementation or measured CPU/IOPS reduction. The gate map grows with touched IDs; profile its retention in long-lived servers before further redesign.

## 2. Connected account to action: GitHub native tools

An explicit **Repository access** action in the existing Connections account card initiates GitHub OAuth with `repo` scope; normal identity login retains `read:user user:email`. GitHub OAuth scopes are broad, including private repository access. A future first-party GitHub App can narrow access to repository Contents-only permission, but this build does not silently install one. Existing OAuth Client ID/Secret configuration is still required; there is no automatic universal OAuth application registration.

The encrypted OAuth vault supplies tokens to a dedicated native Rust executor `src/github_work.rs`, never to the model. The agent and Rust SDK expose exactly the following on a repository-authorized connection:

1. `github_list_repositories` — page through repositories, return default branch and actual `permissions.push` metadata
2. `github_read_file` — read an explicitly selected repository, branch and path, returning the remote SHA and UTF-8 contents
3. `github_commit_file` — **requires human approval** for the frozen exact tool arguments; preflight authenticated push permissions and a matching remote blob SHA, PUT via the official GitHub Contents API, then GET the same branch/path and compare content + SHA before reporting success

HTTP targets are pinned to `https://api.github.com`; model input cannot replace the origin, bearer token or arbitrary REST path. Workflow-directory writes are rejected. After a write dispatch, a disconnected response or unverifiable commit creates an **uncertain remote effect** and must be reconciled by the operator before any retry. The new tool uses the existing journal, cancellation and hook contracts, not an alternate scheduler.

Current OAuth sessions may only have the earlier identity scopes and must be explicitly reconnected for repository work. GitHub OAuth expiration/denial returns a reconnect instruction instead of a fake success. Google OAuth still has only identity scopes: **no Google Drive or Docs write action is claimed**. An MCP server does not inherit GitHub/Google tokens; its credentials remain isolated.

## 3. Verification contracts

New native test cases cover:
- A Run's pending transaction does not block another Run's durable update
- Same-Run ordered fsync persistence survives restart
- GitHub identity login does not grant repository write scopes
- Path/ref validation and workflow path denial
- Mock authenticated GitHub file PUT, optimistic SHA precondition and remote GET receipt verification
- Stale remote SHA refuses to issue a write
- Agent GitHub commit call pauses for approval without any external dispatch

Testing a fixture with a local HTTP server is **not** evidence that a real GitHub OAuth authorization or a live repository commit succeeded. The real-world acceptance test remains: connect OAuth app, grant repository permission, select repo+branch, read file SHA, authorize update, verify new bytes and commit SHA from GitHub, revoke permission, confirm tool fails closed. Repeat with an interrupted response and reconcile the observed remote state.

## 4. Unchanged boundaries and next audited gaps

- Google Drive/Docs/Sheets scopes and API actions need a separate consent flow and provider-specific tools. Existing identity login is *not* a file-work token.
- MCP Streamable HTTP sessions still rely on the existing transport and server-specific bearer secrets. Connection lifecycle / OAuth resource authentication should not be conflated with user GitHub tokens.
- Frontend global refresh, large inactive product panels, and native JSON full-run snapshot bytes still require separate profiling and reductions.
- The mock coding workload proves a bounded tool cycle, **not** matched frontier model success. Real model/provider and budget conditions must be the same across sub3/sub4 before ranking.
- Architecture, startup, approval and regression tests should be rerun after every engine contract change. Full CI success, if observed, is a regression check rather than proof of commercial readiness.
