# Dominion Cloud Gateway (initial scaffold)

This service is the **Dominion-owned** identity and model gateway, not a Nous Portal integration. It is deliberately isolated from the native Rust agent engine and existing UI.

## Intended request flow

1. Dominion web/desktop app starts Google OAuth authorization-code + PKCE against a Dominion-owned callback endpoint.
2. Dominion backend validates Google ID tokens (issuer, audience, signature, expiry, nonce) and creates a Dominion session.
3. The app uses a Dominion session to request models from the gateway. Provider API keys are stored server-side only.
4. Gateway enforces per-user concurrency, rate limits and prepaid budget reservations before sending upstream requests.
5. Gateway reconciles reservations against trusted provider usage and records an immutable ledger entry; errors release unused reservations.

## Security and billing invariants

- Never accept a client-supplied user ID, price, model cost or provider URL as authority.
- Never expose provider API keys to the app or persist them in local agent records.
- Never proxy arbitrary URLs: models map to an operator-maintained allowlist.
- Enforce upstream terms and explicit commercial resale permission for any advertised free inference.
- Use atomic database transactions for credit reservations and reconciliation; idempotency keys prevent duplicate charges.
- Fail closed on missing usage and timeouts: reconcile from provider receipts before releasing disputed reservations.
- Rate limits and abuse prevention apply to free and paid accounts.
- Keep local BYOK and local LLM routes working without a Dominion account.

## Status

Architecture scaffold only. No Google OAuth client, provider billing credentials, payment processor or hosted database is configured. Do not market this as an operational login, free model service or paid gateway yet.

See [gateway contract](gateway-contract.ts) for the typed boundary and [integration](INTEGRATION.md) for Rust wiring.
