# Connected Accounts: GitHub and Google Workspace (2026-11-17)

## Runtime

User → Connections / Accounts → OAuth grant → capability broker → model schema → explicit approval (writes) → durable execution intent → fixed official REST provider → remote read-back → durable receipt. Tokens, OAuth secrets and refresh tokens remain encrypted server-side; never pass them through MCP servers or model prompts.

## Product-managed one-click login (no end-user API key or Client ID)

The person using Dominion clicks **GitHub로 로그인** or **Google로 로그인** and approves provider permissions. The OAuth app registration is **a one-time responsibility of the product/service operator**, never the end user.

Set the following deployment secrets in the server process (e.g., a secret manager). Never put the secrets in React or public source code:

- `DOMINION_GITHUB_OAUTH_CLIENT_ID`, `DOMINION_GITHUB_OAUTH_CLIENT_SECRET`
- `DOMINION_GOOGLE_OAUTH_CLIENT_ID`, `DOMINION_GOOGLE_OAUTH_CLIENT_SECRET`
- `DOMINION_PUBLIC_URL` — the exact externally accessible HTTPS product origin or the local loopback origin

GitHub: register the application at https://github.com/settings/developers, with callback `/api/oauth/github/callback`. Google: register a Web OAuth app at https://console.cloud.google.com/apis/credentials, callback `/api/oauth/google/callback`, and enable Drive/Docs/Sheets APIs. The product requests the normal browser consent flow, then stores tokens in its encrypted server-side vault. Google may require OAuth brand/app verification for the requested Workspace scopes.

Legacy encrypted local OAuth app configuration remains readable for existing installations, but ordinary users have no Client ID or secret entry screen. The server-provided credentials always take precedence. OAuth login requests freeze the application identity at authorization start so a configuration rotation cannot silently switch the callback's client.

For a **local** installation with the official GitHub CLI already logged in (`gh auth login --web`), **기존 GitHub CLI 계정 연결** imports the authenticated CLI token into the encrypted vault with permission checks. This fallback does not require a separately registered Dominion GitHub app, and is disabled when the product is exposed as a non-loopback server. Google Workspace still requires a registered OAuth application: provider authorization cannot be generated from nothing.

## GitHub tools (12)

Repository files: github_list_repositories, github_read_file, github_commit_file.
Branches: github_list_branches, github_create_branch.
Issues: github_list_issues, github_get_issue, github_create_issue, github_comment_issue.
PR: github_list_pull_requests, github_get_pull_request, github_create_pull_request.

## Google tools (11)

Drive: google_drive_search, google_drive_read_text, google_drive_upload_text, google_drive_create_folder.
Docs: google_docs_get, google_docs_create, google_docs_append.
Sheets: google_sheets_create, google_sheets_get, google_sheets_update, google_sheets_append.

## Permissions, recoverability, concurrency

- GitHub updates require an observed blob or base branch SHA. Remote commit, branch, issue, PR and comment writes are read back after acknowledgment.
- Google Docs append checks an observed revision and uses Docs writeControl.requiredRevisionId. Sheets exact update checks the expected_values read-back before write and verifies after write; Sheets has no atomic value compare-and-set so competing collaborators may still update cells.
- Remote writes require human approval and durable journal intent; ambiguous transport/confirmation outcomes are paused for reconciliation and never blindly retried.
- Read tools are explicitly replay-safe. A read-only subagent may not use connected mutating tools. The Google refresh token is refreshed server-side, retained for the same identity and never exposed.
- No arbitrary external host requests, Google Drive deletion, Gmail sending, GitHub PR merges or unattended account-permission escalation is exposed.

## Tested vs. not tested

Mock-provider tests cover new REST create/read-back and permission boundaries, and E2E covers OAuth client configuration. Rust SDK, TypeScript and React use the same native engine and event store.
Real user OAuth consent, Google app verification and remote Github/Google live-account mutations require legitimate configured provider credentials and authorization; these were not executed as part of CI.
