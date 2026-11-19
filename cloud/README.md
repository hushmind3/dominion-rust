# DOMINION Cloud Gateway (initial implementation)

This is the first independent DOMINION account gateway, **not** a Nous Portal integration.
The existing desktop UI and Rust execution engine remain unchanged.

Cloudflare Worker + D1 provides Google ID-token verification (RS256 against Google's JWKs),
a DOMINION bearer session, account balance, and a single-model OpenAI-compatible relay.
Users never receive the upstream provider API key. No paid model call is permitted without
a positive prepaid balance. The first release intentionally rejects streaming, tools,
unconfigured models, and unsupported options.

## Provisioning

Create a Cloudflare D1 database, replace the placeholder ID in `wrangler.toml`,
and apply `schema.sql` with Wrangler D1 migrations/execute. Create a Google OAuth
**web** client ID with your deployed frontend origin authorized.

Configure Worker vars: `GOOGLE_CLIENT_ID`, `ALLOWED_ORIGIN` (exact HTTPS origin),
`UPSTREAM_URL`, `MODEL_ID`, `PRICE_INPUT_MICRO_KRW`, `PRICE_OUTPUT_MICRO_KRW`.
Store `UPSTREAM_API_KEY` using `wrangler secret put`.
All prices are integer micro-KRW per token; one KRW equals 1,000 micro-KRW.

POST `/v1/auth/google` with `{"credential":"<Google ID token>"}` and receive
a DOMINION bearer token. GET `/v1/me` and POST `/v1/chat/completions`
require `Authorization: Bearer <DOMINION token>`.

## Before production

This is an **un-deployed backend foundation**, not a ready billing service.
Add a payment webhook with verified signatures and idempotent credit grants;
never let clients set balances. Add rate limits, replay protection, session
revocation, provider-specific pricing, streaming/tool support, admin audit,
recovery of stranded reservations, and production tests. A failed upstream
request is refunded; a successful request uses reported usage to settle.
The byte-count reservation is deliberately conservative but is not a formal
upper bound for every tokenizer, so reconcile and strengthen before charging
real customers. Do not put the Worker upstream secret in the desktop client.

Google login must use the official Google Identity Services client on the
frontend and submit the resulting ID token to this backend. Google OAuth
credentials, Cloudflare D1 ID, and model supplier credentials are not
committed to this repository.

## Google login and free models (November 2026)

The Model Hub now displays a DOMINION Cloud account card. Configure the frontend build
with `VITE_DOMINION_CLOUD_URL=https://YOUR_WORKER.workers.dev` and
`VITE_DOMINION_GOOGLE_CLIENT_ID=YOUR_GOOGLE_WEB_CLIENT_ID`. The Worker must use the
same `GOOGLE_CLIENT_ID` and set `ALLOWED_ORIGIN` to the exact frontend origin.
Use Google Identity Services on a supported HTTPS web origin. Google Sign-In
is not supported inside embedded desktop webviews; native desktop needs a
system-browser authorization-code/PKCE flow before production.

The Worker `FREE_MODELS_JSON` variable is a JSON array, e.g.
`[{"id":"example-free-model","upstream_model":"provider-specific-id"}]`.
Set `FREE_DAILY_LIMIT=20` for an initial per-account request limit. The
configured `UPSTREAM_URL` and secret `UPSTREAM_API_KEY` must refer to an
inference supplier that actually allows the intended commercial gateway use.
An empty model array intentionally displays no models. No free provider
credentials or quota have been provisioned by committing this code.

`GET /v1/models` lists configured free models for authenticated accounts.
`POST /v1/chat/completions` forwards supported free model requests and SSE
streams through the Worker with a daily account quota. Registering a model
in the desktop UI saves the DOMINION session as the provider credential in
the existing local Rust provider store. Reauthenticate when that session expires.

**Not production-ready:** the free quota is per request rather than tokens,
there is no global upstream spend ceiling, and upstream billing may continue
after client cancellation. Add upstream billing controls, token metering,
rate limiting by IP and account, replay/abuse protection, expiry handling,
and tests before enabling broad public signup.

## Free model catalog and desktop login

The existing Models page includes a DOMINION Cloud Google sign-in card. After
sign-in it requests `GET /v1/models`, displays the enabled free models in a
selector, and saves the chosen model as a normal Rust provider. No end-user
upstream API key is needed. The cloud URL and Google web client ID are supplied
at frontend build time:

```env
VITE_DOMINION_CLOUD_URL=https://your-worker.your-domain.example
VITE_DOMINION_GOOGLE_CLIENT_ID=your-google-web-client-id
```

Configure `FREE_MODELS_JSON` on the Worker to explicitly allow models supplied
by the configured upstream provider, for example:

```json
[{"id":"openrouter/free","name":"Free auto router","free":true}]
```

This is an example only; the upstream account must actually support the model.
Use an upstream API base URL that ends in `/api/v1` for OpenRouter, and store
its API key in `UPSTREAM_API_KEY` as a Worker secret. For specific free models,
use their current API IDs and confirm tool-call support individually. Free
availability and upstream quotas are not guaranteed; OpenRouter free models
have shared per-account rate limits. Never publish the upstream key or offer
unrestricted resale of provider free-tier capacity without permission.

Apply `cloud/schema.sql` to D1 before calling free models; `free_quota`
is required. Configure `FREE_DAILY_LIMIT` conservatively. This code has not
been deployed or verified end-to-end with real Google/Cloudflare credentials.

## Provisioned Cloudflare endpoint (2026-11-18)

Worker URL: `https://dominion-cloud.dominion-hushmind.workers.dev`
D1 database: `dominion-cloud` (APAC), ID `edc9c387-310e-4660-aac7-db53f9da5d17`.
Worker uploaded and workers.dev route enabled. The endpoint returns HTTP 503 until a real Google OAuth web client ID, allowed frontend origin and upstream model API credentials are configured. Do not place credentials in GitHub. Do not advertise free inference until an upstream key and working login are tested.


## Kakao and Naver sign-in

DOMINION Cloud supports Google, Kakao, and Naver account identities. Kakao and
Naver use authorization-code login in a popup, a random state stored in a
short-lived Secure HttpOnly SameSite=Lax cookie, server-side token exchange,
provider profile verification, and a DOMINION session scoped to the verified
provider user ID. The popup returns the session only to `ALLOWED_ORIGIN`.
The frontend checks the Worker origin before accepting it.

Register **web** applications in Kakao Developers and Naver Developers.
Set the exact callback URLs:
`https://YOUR_WORKER/v1/auth/kakao/callback` and
`https://YOUR_WORKER/v1/auth/naver/callback`.
Configure `KAKAO_CLIENT_ID` / `NAVER_CLIENT_ID` as Worker variables and
`KAKAO_CLIENT_SECRET` / `NAVER_CLIENT_SECRET` as Worker secrets.
The sign-in buttons do not function until those provider apps and callback
URLs are configured and the Worker is deployed. Google login uses its existing
separate client ID.

Provider-specific account IDs deliberately stay distinct: logging in with
Google, Kakao, and Naver does not silently merge accounts based on email.
A verified, explicit account-linking flow is required before shared credits,
payments, or subscriptions can move across identities.

TossPayments Pro purchases are one-time thirty-day entitlements, not
recurring card charges. Test merchant keys and a successful payment-confirmation
round trip are required before enabling live payments.

## Merchant settlement and bank payout (TossPayments)

DOMINION never transfers customer payments to the operator's bank account itself. TossPayments is the payment gateway and settles captured payments to the merchant's bank account registered in the TossPayments merchant agreement, net of applicable fees, on the contractual payout schedule. This is distinct from DOMINION's checkout confirmation and Pro entitlement grant.

Before production activation the operator must register a business, complete the PG merchant contract and underwriting, register and verify the merchant settlement bank account in the PG dashboard, and obtain production client and secret keys. Store the secret only as a Cloudflare Worker secret; never place it in GitHub or desktop app settings. Configure PLAN_PRO_KRW and the production TOSS_CLIENT_KEY, TOSS_SECRET_KEY only after merchant onboarding. Keep test keys during testing; test payments do not result in bank deposits. Verify actual deposits and fees against the PG merchant settlement report and bank statement. A merchant account and payout bank account cannot be created or verified by this repository.

Authenticated users can read their own recent order statuses through GET /v1/billing/payments (up to 50 entries; no payment keys or bank details returned). This endpoint is an order ledger, **not** proof that a merchant bank payout occurred. Before accepting real payments, add authenticated administrator-only settlement reconciliation, refund/cancellation handling, recovery of stuck processing orders, and end-to-end production verification. Do not label a payment as bank-settled merely because its checkout order is paid.


## Merchant settlement and payment operations

DOMINION never transfers a customer's payment directly to an operator bank account.
TossPayments processes payment approval and settles the net proceeds to the merchant
bank account registered during the merchant contract. Do not store bank account
numbers or Toss secret keys in the repository, browser, or desktop application.

Merchant setup checklist:
1. Register the business and complete TossPayments merchant onboarding and review.
2. Register the merchant-owned settlement bank account in TossPayments merchant admin.
3. Register the service URL, product description, terms, refund policy and contact details.
4. Configure `TOSS_CLIENT_KEY`, `TOSS_SECRET_KEY` (Worker secret), and `PLAN_PRO_KRW`.
5. Use TEST keys for end-to-end checkout and approval before enabling LIVE keys.
6. Compare confirmed orders with TossPayments merchant settlement reports and bank deposits.

Authenticated `GET /v1/billing/orders` returns the current member's most recent
30 checkout orders, including order ID, price, payment status, and timestamps.
It intentionally excludes provider payment keys and all bank details. This is a
payment ledger, **not** proof that the payment provider has transferred funds.
Settlement timing, fees, chargebacks and refunds are controlled by the
merchant agreement and must be reconciled against provider reports.

Before live charging: implement and verify refund/cancellation handling,
payment-status webhook reconciliation, stuck `processing` order recovery,
admin-only settlement reconciliation and customer support processes. Existing
Pro checkout is a one-time 30-day purchase, not recurring auto-billing.
