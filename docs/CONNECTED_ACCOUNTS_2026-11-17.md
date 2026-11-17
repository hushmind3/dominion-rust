# Connected Accounts: GitHub and Google Workspace (2026-11-17)

## Runtime

User → Connections / Accounts → OAuth grant → capability broker → model schema → explicit approval (writes) → durable execution intent → fixed official REST provider → remote read-back → durable receipt. Tokens, OAuth secrets and refresh tokens remain encrypted server-side; never pass them through MCP servers or model prompts.

## Register an OAuth app once

- GitHub: https://github.com/settings/developers — create OAuth App, copy Client ID/Secret, register the exact callback URL shown in Connections. Identity uses read:user/user:email. Repository work separately opts in to classic repo scope, which also permits private repository access.
- Google: https://console.cloud.google.com/apis/credentials — create a Web OAuth app, register the exact callback URL, enable Drive, Docs and Sheets APIs. Identity uses openid/email/profile. Workspace work separately requests drive/documents/spreadsheets. Google may require app publishing/verification for broad Drive scopes.

No fixed developer OAuth app credentials are bundled. Self-hosted apps must be registered with providers; the Connect button triggers the genuine browser consent flow.

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
