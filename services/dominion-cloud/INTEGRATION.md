# Dominion native integration contract

Keep existing UI and native agent harness intact. Add a new opt-in provider type **Dominion Cloud** to the existing provider selection path in `src/engine/chat/provider.rs` and `src/engine/gateway.rs` only after a live gateway exists.

The native app should store a revocable Dominion session token in the OS credential store (not the provider API-key field or project SQLite). Use a fixed operator-owned HTTPS base URL; never allow the client to substitute arbitrary relay origins. The gateway owns Google login, token validation, upstream credentials, per-account quota and billing.

Local models, BYOK providers, Codex OAuth and existing agent/room execution must continue to function without Dominion login. The native app forwards model requests and receives OpenAI-compatible streamed responses; the cloud gateway must preserve tool calls and usage receipts. Do not use Nous OAuth endpoints or Nous access tokens.

## Rollout gates

Google OAuth application + verified redirect domains; hosted database with atomic credit ledger; server-side model registry; commercial terms for each free provider; billing integration; rate-limit and replay tests; streamed usage accounting; then end-to-end CI. Do not enable paid routing before these gates pass.
