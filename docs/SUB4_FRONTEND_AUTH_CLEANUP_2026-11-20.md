# Sub4 frontend, connected-account and abandoned-login cleanup — 2026-11-20

## Visible legacy UI removed

- **Connections** had two competing GitHub/Google account screens. The old OAuth app configuration tab prompted users for product-owned Client ID and Client Secret and advertised a duplicate sign-in button. It is removed; a single **계정 연결** tab uses `ConnectedAccounts`.
- Removed the unused browser/desktop OAuth popup stub, the public route for editing OAuth app credentials, and retired GitHub CLI OAuth URL aliases. The active `/api/providers/cli/github/login` routes still drive the CLI fallback. The internal `import_github_cli` method is retained because that live fallback invokes it after verified CLI authorization.
- **Google Workspace** now requests real Drive, Docs and Sheets scopes, not an identity-only login. **GitHub** requests repository permission. Account status is ready only with the respective persisted capability scopes; expired or identity-only grants do not become false connected status. Missing OAuth app setup hides unusable login controls.
- **Model menu** no longer lists unsupported Claude/Gemini/OpenCode CLI sign-in cards or their inert browser login, import, and model-picker handlers. OpenCode Go and ordinary API key providers remain in the supported provider list. The Codex CLI device auth tab appears only when Codex CLI is installed; CLI auth is not misrepresented as a ready native provider.
- **DOMINION Cloud** does not render inactive Kakao/Naver sign-in buttons or an empty sign-in modal. Working, configured Cloud sign-in, subscriber billing and PASS identity verification are untouched.
- **Channel filter** lists only the categories actually served by the current Telegram and Discord outbound drivers. Removed stale Slack/WhatsApp/Matrix placeholder references and corrected ready-state counting to `outbound_ready`.
- Deleted abandoned translation strings and adapted account E2E and native/React contract regressions.

## Not reintroduced

Embedded YouTube/video generation, Money opportunity UI and standalone RSI UI/engine remain absent. Cloud billing authentication, GitHub, Google Workspace, native model authorization, MCP and the normal Rust harness remain supported. Preexisting encrypted credentials and user data are not modified or deleted.

## Verification

Run `scripts/check-sub4-architecture.sh` and native development CI (Rust all-target compilation and tests, React TypeScript, SDK and Worker tests). Desktop binary builds and browser Chromium E2E remain manual and are not proof of successful live Google consent.
