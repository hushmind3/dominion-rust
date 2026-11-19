// DOMINION account gateway: no Nous credentials and no shared client-side API key.
// Cloudflare Worker + D1. Credits are integer micro-KRW (one KRW = 1,000 units).
const json = (data, status = 200, headers = {}) => new Response(JSON.stringify(data), {
  status, headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store', ...headers }
});
const bytes = text => new TextEncoder().encode(text);
const b64url = data => btoa(String.fromCharCode(...new Uint8Array(data))).replace(/=/g, '').replace(/\+/g, '-').replace(/\//g, '_');
const unb64 = text => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - text.length % 4) % 4)), c => c.charCodeAt(0));
const hash = async value => b64url(await crypto.subtle.digest('SHA-256', bytes(value)));
const err = (message, status = 400) => json({ error: message }, status);
const now = () => Math.floor(Date.now() / 1000);
const maxBody = async (request, limit = 150000) => {
  if (Number(request.headers.get('content-length') || 0) > limit) throw new Error('Request too large');
  const text = await request.text();
  if (text.length > limit) throw new Error('Request too large');
  return JSON.parse(text);
};
function cors(origin, env) {
  return origin && origin === env.ALLOWED_ORIGIN ? {
    'access-control-allow-origin': origin,
    'access-control-allow-methods': 'POST, GET, OPTIONS',
    'access-control-allow-headers': 'content-type, authorization',
    'access-control-max-age': '600',
    vary: 'Origin'
  } : {};
}
async function verifyGoogle(idToken, env) {
  const parts = idToken.split('.');
  if (parts.length !== 3) throw new Error('Invalid Google token');
  const header = JSON.parse(new TextDecoder().decode(unb64(parts[0])));
  const claims = JSON.parse(new TextDecoder().decode(unb64(parts[1])));
  if (header.alg !== 'RS256' || !header.kid || claims.aud !== env.GOOGLE_CLIENT_ID ||
      !['accounts.google.com', 'https://accounts.google.com'].includes(claims.iss) ||
      !claims.sub || !Number.isSafeInteger(claims.exp) || claims.exp <= now() ||
      !Number.isSafeInteger(claims.iat) || claims.iat > now() + 60) throw new Error('Invalid Google claims');
  const res = await fetch('https://www.googleapis.com/oauth2/v3/certs');
  if (!res.ok) throw new Error('Google keys unavailable');
  const jwk = (await res.json()).keys.find(k => k.kid === header.kid && k.kty === 'RSA' && k.use === 'sig');
  if (!jwk) throw new Error('Unknown Google signing key');
  const key = await crypto.subtle.importKey('jwk', jwk, { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, false, ['verify']);
  if (!await crypto.subtle.verify('RSASSA-PKCS1-v1_5', key, unb64(parts[2]), bytes(parts[0] + '.' + parts[1])))
    throw new Error('Invalid Google signature');
  return claims;
}
async function account(request, env) {
  const token = /^Bearer ([A-Za-z0-9_-]{32,})$/.exec(request.headers.get('authorization') || '')?.[1];
  if (!token) return null;
  return env.DB.prepare('SELECT a.id, a.credits FROM sessions s JOIN accounts a ON a.id=s.account_id WHERE s.token_hash=? AND s.expires_at>?')
    .bind(await hash(token), now()).first();
}
async function login(request, env) {
  const { credential } = await maxBody(request);
  if (typeof credential !== 'string' || credential.length > 10000) return err('Google credential required');
  if(!env.GOOGLE_CLIENT_ID)return err('Google login not configured',503);
  const google = await verifyGoogle(credential, env);
  const id = 'google:' + google.sub;
  await env.DB.prepare('INSERT INTO accounts (id,email,credits,created_at) VALUES (?,?,0,?) ON CONFLICT(id) DO UPDATE SET email=excluded.email')
    .bind(id, String(google.email || '').slice(0, 320), now()).run();
  const token = b64url(crypto.getRandomValues(new Uint8Array(32)));
  await env.DB.prepare('INSERT INTO sessions (token_hash,account_id,expires_at) VALUES (?,?,?)')
    .bind(await hash(token), id, now() + 7 * 86400).run();
  return json({ token, expires_in: 7 * 86400, account_id: id });
}


// Kakao / Naver authorization-code login. State is short-lived, signed and
// bound to a browser cookie; provider tokens never reach the frontend.
const oauthProviders = {
  kakao: { authorize:'https://kauth.kakao.com/oauth/authorize', token:'https://kauth.kakao.com/oauth/token', profile:'https://kapi.kakao.com/v2/user/me' },
  naver: { authorize:'https://nid.naver.com/oauth2.0/authorize', token:'https://nid.naver.com/oauth2.0/token', profile:'https://openapi.naver.com/v1/nid/me' }
};
function oauthConfig(provider, env) {
  const prefix=provider.toUpperCase();
  const id=env[prefix+'_CLIENT_ID'], secret=env[prefix+'_CLIENT_SECRET'];
  return id&&secret ? {id,secret,...oauthProviders[provider]} : null;
}
async function createSession(env,id,email) {
  await env.DB.prepare('INSERT INTO accounts(id,email,credits,created_at) VALUES(?,?,0,?) ON CONFLICT(id) DO UPDATE SET email=excluded.email')
    .bind(id,String(email||'').slice(0,320),now()).run();
  const token=b64url(crypto.getRandomValues(new Uint8Array(32)));
  await env.DB.prepare('INSERT INTO sessions(token_hash,account_id,expires_at) VALUES(?,?,?)')
    .bind(await hash(token),id,now()+7*86400).run();
  return token;
}
function oauthCookie(provider,value,maxAge) {
  return 'dominion_oauth_'+provider+'='+value+'; HttpOnly; Secure; SameSite=Lax; Path=/v1/auth/'+provider+'; Max-Age='+maxAge;
}
async function oauthStart(request,env,provider) {
  const config=oauthConfig(provider,env);
  if(!config)return err('Login provider not configured',503);
  const state=b64url(crypto.getRandomValues(new Uint8Array(32)));
  const callback=new URL('/v1/auth/'+provider+'/callback',request.url).href;
  const url=new URL(config.authorize);
  url.searchParams.set('response_type','code');
  url.searchParams.set('client_id',config.id);
  url.searchParams.set('redirect_uri',callback);
  url.searchParams.set('state',state);
  if(provider==='naver')url.searchParams.set('state',state);
  return new Response(null,{status:302,headers:{location:url.href,'set-cookie':oauthCookie(provider,state,600),'cache-control':'no-store'}});
}
async function oauthCallback(request,env,provider) {
  const config=oauthConfig(provider,env);
  if(!config)return err('Login provider not configured',503);
  const url=new URL(request.url),state=url.searchParams.get('state')||'';
  const cookie=(request.headers.get('cookie')||'').split(';').map(x=>x.trim()).find(x=>x.startsWith('dominion_oauth_'+provider+'='))?.split('=')[1];
  if(!state||!cookie||state.length!==cookie.length||await hash(state)!==await hash(cookie))return err('OAuth state mismatch',403);
  const code=url.searchParams.get('code');
  if(!code||code.length>2048)return err('OAuth authorization denied',400);
  const callback=new URL('/v1/auth/'+provider+'/callback',request.url).href;
  const params=new URLSearchParams({grant_type:'authorization_code',client_id:config.id,client_secret:config.secret,code,redirect_uri:callback});
  if(provider==='naver')params.set('state',state);
  const exchange=await fetch(config.token,{method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},body:params,signal:AbortSignal.timeout(12000)});
  if(!exchange.ok)return err('OAuth token exchange failed',502);
  const tokens=await exchange.json();
  if(typeof tokens.access_token!=='string')return err('OAuth token missing',502);
  const profileResponse=await fetch(config.profile,{headers:{authorization:'Bearer '+tokens.access_token},signal:AbortSignal.timeout(12000)});
  if(!profileResponse.ok)return err('OAuth profile unavailable',502);
  const profile=await profileResponse.json();
  const user=provider==='naver'?profile.response:profile;
  const id=user?.id;
  if((typeof id!=='string'&&typeof id!=='number')||!String(id))return err('OAuth identity missing',502);
  const email=provider==='kakao'?user.kakao_account?.email:user.email;
  const session=await createSession(env,provider+':'+id,email);
  // The popup reports only to the configured DOMINION frontend origin.
  const payload=JSON.stringify({type:'dominion-oauth-session',provider,token:session}).replace(/</g,'\\u003c');
  const target=JSON.stringify(env.ALLOWED_ORIGIN);
  const html='<!doctype html><meta charset="utf-8"><title>DOMINION 로그인</title><script>if(window.opener){window.opener.postMessage('+payload+','+target+');window.close()}else{document.body.textContent="로그인 창을 닫고 다시 시도하세요."}</script>';
  return new Response(html,{headers:{'content-type':'text/html; charset=utf-8','cache-control':'no-store','content-security-policy':"default-src 'none'; script-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'",'set-cookie':oauthCookie(provider,'',0)}});
}

// Checkout amounts are set by the operator, never trusted from a browser.
function proPrice(env) {
  const amount = Number(env.PLAN_PRO_KRW);
  return Number.isSafeInteger(amount) && amount >= 100 && amount <= 10000000 ? amount : null;
}
async function billingSummary(env, user) {
  const subscription = await env.DB.prepare('SELECT plan,status,current_period_end FROM subscriptions WHERE account_id=?').bind(user.id).first();
  const freeDailyLimit = Math.max(1, Math.min(1000, Number(env.FREE_DAILY_LIMIT || 20)));
  const quotaDay = new Date().toISOString().slice(0, 10);
  const freeUsage = await env.DB.prepare('SELECT used FROM free_quota WHERE account_day=?').bind(user.id + ':' + quotaDay).first();
  return json({
    free_daily_limit: freeDailyLimit,
    free_daily_used: Math.min(freeDailyLimit, freeUsage?.used || 0),
    free_daily_remaining: Math.max(0, freeDailyLimit - (freeUsage?.used || 0)),
    free_quota_reset_utc: new Date(Date.parse(quotaDay + 'T00:00:00Z') + 86400000).toISOString(),
    plan: subscription?.status === 'active' && subscription?.current_period_end > now() ? subscription.plan : 'free',
    valid_until: subscription?.current_period_end || null,
    pro_price_krw: proPrice(env),
    checkout_enabled: Boolean(proPrice(env) && env.TOSS_CLIENT_KEY && env.TOSS_SECRET_KEY),
    toss_client_key: proPrice(env) && env.TOSS_SECRET_KEY ? (env.TOSS_CLIENT_KEY || null) : null
  });
}
// Account-scoped payment ledger. Settlement is performed by the contracted PG,
// never by the desktop client or by transferring funds from this Worker.
async function paymentHistory(env, user) {
  const rows = await env.DB.prepare("SELECT id,plan,amount_krw,status,created_at,paid_at FROM checkout_orders WHERE account_id=? ORDER BY created_at DESC LIMIT 50").bind(user.id).all();
  return json({ payments: rows.results || [], settlement_provider: 'tosspayments', settlement_managed_externally: true });
}
// Paid Cloud access requires independently verified identity. OAuth alone is not identity proof.
async function identityStatus(env, user) {
  const row = await env.DB.prepare('SELECT verified_at,provider FROM identity_verifications WHERE account_id=?').bind(user.id).first();
  return { verified: Boolean(row?.verified_at), verified_at: row?.verified_at || null, provider: row?.provider || null };
}
async function identityStart(env, user) {
  if (!env.PORTONE_STORE_ID || !env.PORTONE_IDENTITY_CHANNEL_KEY)
    return err('Identity verification provider not configured',503);
  // The client starts the provider SDK with this server-issued, account-bound identity ID.
  const identityVerificationId = crypto.randomUUID();
  await env.DB.prepare('INSERT INTO identity_attempts(id,account_id,status,created_at) VALUES(?,?,?,?)')
    .bind(identityVerificationId,user.id,'pending',now()).run();
  return json({identityVerificationId,storeId:env.PORTONE_STORE_ID,channelKey:env.PORTONE_IDENTITY_CHANNEL_KEY});
}
async function identityConfirm(request, env, user) {
  if (!env.PORTONE_API_SECRET) return err('Identity verification provider not configured',503);
  const {identityVerificationId}=await maxBody(request);
  if (typeof identityVerificationId!=='string'||!/^[0-9a-f-]{36}$/.test(identityVerificationId))
    return err('Invalid verification ID');
  const attempt=await env.DB.prepare('SELECT status FROM identity_attempts WHERE id=? AND account_id=?')
    .bind(identityVerificationId,user.id).first();
  if (!attempt) return err('Unknown identity verification',403);
  if (attempt.status==='verified') return json({verified:true});
  if (attempt.status!=='pending') return err('Verification not pending',409);
  const response=await fetch('https://api.portone.io/identity-verifications/'+encodeURIComponent(identityVerificationId),{
    headers:{Authorization:'PortOne '+env.PORTONE_API_SECRET},signal:AbortSignal.timeout(12000)
  });
  if (!response.ok) return err('Identity provider verification failed',502);
  const result=await response.json();
  if (result.status!=='VERIFIED') return err('Identity verification incomplete',403);
  await env.DB.batch([
    env.DB.prepare("UPDATE identity_attempts SET status='verified' WHERE id=? AND account_id=? AND status='pending'").bind(identityVerificationId,user.id),
    env.DB.prepare("INSERT INTO identity_verifications(account_id,provider,verified_at) VALUES(?,'portone',?) ON CONFLICT(account_id) DO UPDATE SET provider='portone',verified_at=excluded.verified_at").bind(user.id,now())
  ]);
  return json({verified:true});
}
async function checkout(request, env, user) {
  if (!(await identityStatus(env,user)).verified) return err('Identity verification required for paid access',403);
  if (!env.TOSS_CLIENT_KEY || !env.TOSS_SECRET_KEY || !proPrice(env)) return err('Test checkout not configured', 503);
  const body = await maxBody(request);
  if (body.plan !== 'pro') return err('Unknown plan');
  const orderId = 'dominion_' + crypto.randomUUID().replace(/-/g, '');
  const amount = proPrice(env);
  await env.DB.prepare('INSERT INTO checkout_orders(id,account_id,plan,amount_krw,status,created_at) VALUES(?,?,?,?,?,?)')
    .bind(orderId, user.id, 'pro', amount, 'pending', now()).run();
  return json({ orderId, amount, orderName: 'DOMINION Pro 30일', customerKey: await hash(user.id), clientKey: env.TOSS_CLIENT_KEY });
}
async function confirmCheckout(request, env, user) {
  if (!env.TOSS_SECRET_KEY) return err('Checkout not configured', 503);
  const { orderId, paymentKey, amount } = await maxBody(request);
  if (typeof orderId !== 'string' || !/^dominion_[a-f0-9]{32}$/.test(orderId) ||
      typeof paymentKey !== 'string' || paymentKey.length > 200 ||
      !Number.isSafeInteger(amount)) return err('Invalid payment confirmation');
  const order = await env.DB.prepare('SELECT * FROM checkout_orders WHERE id=? AND account_id=?').bind(orderId,user.id).first();
  if (!order || order.amount_krw !== amount) return err('Order not found or amount mismatch', 403);
  if (order.status === 'paid') return order.payment_key === paymentKey ? json({status:'paid',orderId}) : err('Payment mismatch',409);
  if (order.status !== 'pending') return err('Payment already being processed',409);
  const locked = await env.DB.prepare("UPDATE checkout_orders SET status='processing' WHERE id=? AND account_id=? AND status='pending' RETURNING id").bind(orderId,user.id).first();
  if (!locked) return err('Payment already being processed',409);
  try {
    const basic = btoa(env.TOSS_SECRET_KEY + ':');
    const response = await fetch('https://api.tosspayments.com/v1/payments/confirm',{
      method:'POST',headers:{authorization:'Basic '+basic,'content-type':'application/json','Idempotency-Key':orderId},
      body:JSON.stringify({paymentKey,orderId,amount}),signal:AbortSignal.timeout(15000)
    });
    const result = await response.json();
    if (!response.ok || result.orderId !== orderId || result.totalAmount !== amount || result.status !== 'DONE') {
      await env.DB.prepare("UPDATE checkout_orders SET status='failed' WHERE id=? AND status='processing'").bind(orderId).run();
      return err('Payment was not approved',502);
    }
    // One transaction: grant exactly one subscription period after verified payment.
    await env.DB.batch([
      env.DB.prepare("UPDATE checkout_orders SET status='paid',payment_key=?,paid_at=? WHERE id=? AND account_id=? AND status='processing'").bind(paymentKey,now(),orderId,user.id),
      env.DB.prepare("INSERT INTO subscriptions(account_id,plan,status,current_period_end,updated_at) VALUES(?,'pro','active',?,?) ON CONFLICT(account_id) DO UPDATE SET plan='pro',status='active',current_period_end=MAX(COALESCE(subscriptions.current_period_end,0),?)+2592000,updated_at=?")
        .bind(user.id,now()+2592000,now(),now(),now())
    ]);
    return json({status:'paid',orderId});
  } catch {
    // Timeout may mean the provider approved payment: preserve processing for reconciliation.
    return err('Payment status pending verification; contact support with order ID',503);
  }
}
const builtinModels = [
  { id: '@cf/meta/llama-3.1-8b-instruct-fast', name: 'Llama 3.1 8B Fast', free: true, backend: 'workers-ai' },
  { id: '@cf/meta/llama-3.2-3b-instruct', name: 'Llama 3.2 3B', free: true, backend: 'workers-ai' }
];
function freeModels(env) {
  let extras = [];
  try {
    const items = JSON.parse(env.FREE_MODELS_JSON || '[]');
    if (Array.isArray(items)) extras = items.filter(x => typeof x.id === 'string' && /^[a-zA-Z0-9._:/-]{1,120}$/.test(x.id));
  } catch { /* Invalid optional catalog must not hide built-in models. */ }
  const upstreamEnabled = Boolean(env.UPSTREAM_API_KEY && env.UPSTREAM_URL);
  const allowed = extras.filter(x => x.backend === 'workers-ai' ? Boolean(env.AI) : upstreamEnabled);
  const all = [...(env.AI ? builtinModels : []), ...allowed];
  return [...new Map(all.map(x => [x.id, x])).values()];
}
async function upstreamFreeCatalog(env) {
  if (!env.UPSTREAM_API_KEY || !env.UPSTREAM_URL || !env.UPSTREAM_FREE_DISCOVERY) return [];
  const url = env.UPSTREAM_URL.replace(/\/$/, '') + '/models';
  try {
    const response = await fetch(url, {
      headers: { authorization: 'Bearer ' + env.UPSTREAM_API_KEY },
      signal: AbortSignal.timeout(12000)
    });
    if (!response.ok) return [];
    const catalog = await response.json();
    if (!Array.isArray(catalog.data)) return [];
    return catalog.data.filter(m => typeof m.id === 'string' && m.id.endsWith(':free') &&
      m.pricing && Number(m.pricing.prompt) === 0 && Number(m.pricing.completion) === 0)
      .slice(0, 300).map(m => ({ id: m.id, name: m.name || m.id, free: true, backend: 'upstream' }));
  } catch { return []; }
}
// Worker-isolate cache reduces catalog traffic. Expired snapshots are retained
// for transient upstream failures; the scheduled handler refreshes proactively.
let catalogCache = { expires: 0, models: [], refreshedAt: 0, upstreamVerifiedAt: 0 };
const modelFailures = new Map();
const catalogTtl = 15 * 60 * 1000;
function modelAvailable(id) {
  const entry = modelFailures.get(id);
  return !entry || entry.until <= Date.now();
}
function markModelResult(id, ok) {
  if (ok) { modelFailures.delete(id); return; }
  const old = modelFailures.get(id);
  const count = (old?.count || 0) + 1;
  modelFailures.set(id, { count, until: count >= 3 ? Date.now() + 5 * 60 * 1000 : 0 });
}
async function availableModels(env, refresh = false) {
  if (!refresh && catalogCache.expires > Date.now()) return catalogCache.models;
  const upstream = await upstreamFreeCatalog(env);
  // Keep the last known catalog when discovery temporarily fails.
  // Never keep a promotional free tier indefinitely: prices may change.
  const staleSafe = Date.now() - catalogCache.upstreamVerifiedAt < 60 * 60 * 1000;
  const previous = upstream.length ? upstream : staleSafe ? catalogCache.models.filter(m => m.backend === 'upstream') : [];
  const models = [...new Map([...freeModels(env), ...previous].map(m => [m.id, m])).values()];
  catalogCache = { models, expires: Date.now() + catalogTtl, refreshedAt: Date.now(), upstreamVerifiedAt: upstream.length ? Date.now() : catalogCache.upstreamVerifiedAt };
  return models;
}
async function freeProxy(request, env, user) {
  const body = await maxBody(request);
  const model = (await availableModels(env)).find(x => x.id === body.model);
  if (!model || (model.backend !== 'workers-ai' && (!env.UPSTREAM_API_KEY || !env.UPSTREAM_URL))) return err('Free model not available', 503);
  if (!modelAvailable(model.id)) return err('Model temporarily unavailable', 503);
  // Chat clients may request streaming and advertise tools by default.
  // Free text models do not execute tools; compatibility mode ignores tool schemas.
  if (body.n !== undefined && body.n !== 1) return err('Only one completion supported');
  if (!Array.isArray(body.messages) || !body.messages.length ||
      JSON.stringify(body.messages).length > 50000 ||
      (body.max_tokens !== undefined && (!Number.isSafeInteger(body.max_tokens) || body.max_tokens < 1)))
    return err('Invalid or oversized messages');
  // Free and Pro both have hard account-level limits. A subscription is not unlimited inference.
  const subscription = await env.DB.prepare('SELECT status,current_period_end FROM subscriptions WHERE account_id=?').bind(user.id).first();
  const isPro = subscription?.status === 'active' && subscription.current_period_end > now();
  const configured = Number(isPro ? env.PRO_DAILY_LIMIT || 50 : env.FREE_DAILY_LIMIT || 20);
  const limit = Number.isSafeInteger(configured) ? Math.max(1, Math.min(1000, configured)) : 20;
  const monthLimitConfig = Number(env.PRO_MONTHLY_LIMIT || 500);
  const monthLimit = Number.isSafeInteger(monthLimitConfig) ? Math.max(1, Math.min(10000, monthLimitConfig)) : 500;
  const day = new Date().toISOString().slice(0, 10);
  const key = user.id + ':' + day;
  const monthlyKey = user.id + ':pro-month:' + day.slice(0, 7);
  let monthlyReserved = false;
  if (isPro) {
    const month = await env.DB.prepare('INSERT INTO free_quota(account_day,used) VALUES(?,1) ON CONFLICT(account_day) DO UPDATE SET used=used+1 WHERE used<? RETURNING used')
      .bind(monthlyKey, monthLimit).first();
    if (!month) return err('Monthly Pro quota exhausted', 429);
    monthlyReserved = true;
  }
  const refundQuota = async () => {
    await env.DB.prepare('UPDATE free_quota SET used=MAX(0,used-1) WHERE account_day=?').bind(key).run();
    if (monthlyReserved) await env.DB.prepare('UPDATE free_quota SET used=MAX(0,used-1) WHERE account_day=?').bind(monthlyKey).run();
  };
  const updated = await env.DB.prepare('INSERT INTO free_quota(account_day,used) VALUES(?,1) ON CONFLICT(account_day) DO UPDATE SET used=used+1 WHERE used<? RETURNING used')
    .bind(key, limit).first();
  if (!updated) {
    if (monthlyReserved) await env.DB.prepare('UPDATE free_quota SET used=MAX(0,used-1) WHERE account_day=?').bind(monthlyKey).run();
    return err('Daily free quota exhausted', 429);
  }
  if (model.backend === 'workers-ai') {
    try {
      const answer = await env.AI.run(model.id, { messages: body.messages, max_tokens: Math.min(1024, body.max_tokens || 512) });
      const content = typeof answer?.response === 'string' ? answer.response : '';
      if (!content) throw Error('Empty model response');
      markModelResult(model.id, true);
      const completionId = crypto.randomUUID();
      const quotaHeaders = { 'x-dominion-free-remaining': String(Math.max(0, limit - updated.used)) };
      if (body.stream === true) {
        const chunk = (delta, finish_reason = null) => ({
          id: completionId, object: 'chat.completion.chunk', created: now(), model: model.id,
          choices: [{ index: 0, delta, finish_reason }]
        });
        const payload = 'data: ' + JSON.stringify(chunk({ role: 'assistant', content })) + '\n\n'
          + 'data: ' + JSON.stringify(chunk({}, 'stop')) + '\n\n'
          + 'data: [DONE]\n\n';
        return new Response(payload, { status: 200, headers: {
          'content-type': 'text/event-stream; charset=utf-8', 'cache-control': 'no-store', ...quotaHeaders
        } });
      }
      return json({ id: completionId, object: 'chat.completion', model: model.id, choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }], usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 } }, 200, quotaHeaders);
    } catch { markModelResult(model.id, false); await refundQuota(); return err('Workers AI inference unavailable', 502); }
  }
  let upstream;
  try {
    upstream = await fetch(env.UPSTREAM_URL.replace(/\/$/, '') + '/chat/completions', {
      method: 'POST',
      headers: { authorization: 'Bearer ' + env.UPSTREAM_API_KEY, 'content-type': 'application/json' },
      body: JSON.stringify({ model: model.upstream_model || model.id, messages: body.messages, stream: body.stream === true, max_tokens: Math.min(2048, body.max_tokens || 1024) }),
      signal: AbortSignal.timeout(90000)
    });
  } catch {
    markModelResult(model.id, false);
    await refundQuota();
    return err('Free model provider connection failed', 502);
  }
  if (!upstream.ok) { if (upstream.status === 404 || upstream.status === 429 || upstream.status >= 500) markModelResult(model.id, false); await refundQuota(); return err('Free model provider unavailable', 502); }
  markModelResult(model.id, true);
  const headers = new Headers({ 'cache-control': 'no-store', 'x-dominion-free-remaining': String(Math.max(0, limit - updated.used)) });
  headers.set('content-type', upstream.headers.get('content-type') || (body.stream ? 'text/event-stream' : 'application/json'));
  return new Response(upstream.body, { status: upstream.status, headers });
}

// Operator-managed provider registry. Keys stay in Worker secrets, never in /v1/models.
function paidModels(env) {
  let configs;
  try { configs = JSON.parse(env.PAID_MODELS_JSON || '[]'); } catch { return []; }
  if (!Array.isArray(configs)) return [];
  const providers = {
    openrouter: { url: 'https://openrouter.ai/api/v1', key: env.OPENROUTER_API_KEY },
    openai: { url: 'https://api.openai.com/v1', key: env.OPENAI_API_KEY },
    deepseek: { url: 'https://api.deepseek.com/v1', key: env.DEEPSEEK_API_KEY },
    groq: { url: 'https://api.groq.com/openai/v1', key: env.GROQ_API_KEY },
    together: { url: 'https://api.together.xyz/v1', key: env.TOGETHER_API_KEY },
    mistral: { url: 'https://api.mistral.ai/v1', key: env.MISTRAL_API_KEY }
  };
  return configs.filter(m => m && typeof m.id === 'string' && /^[a-zA-Z0-9._:/-]{1,120}$/.test(m.id) &&
    typeof m.provider === 'string' && providers[m.provider]?.key &&
    Number.isSafeInteger(m.input_micro_krw) && m.input_micro_krw > 0 &&
    Number.isSafeInteger(m.output_micro_krw) && m.output_micro_krw > 0)
    .slice(0, 300).map(m => ({ ...m, name: m.name || m.id, free: false, backend: m.provider,
      endpoint: providers[m.provider].url, apiKey: providers[m.provider].key,
      upstream_model: m.upstream_model || m.id }));
}
async function proxy(request, env, user) {
  if (!(await identityStatus(env,user)).verified) return err('Identity verification required for paid models',403);
  const body = await maxBody(request);
  const selected = paidModels(env).find(m => m.id === body.model);
  const legacy = body.model === env.MODEL_ID && env.UPSTREAM_API_KEY && env.UPSTREAM_URL && env.PRICE_INPUT_MICRO_KRW && env.PRICE_OUTPUT_MICRO_KRW;
  if (!selected && !legacy) return err('Paid model not enabled by operator', 403);
  const modelId = selected?.id || env.MODEL_ID;
  const modelEndpoint = selected?.endpoint || env.UPSTREAM_URL;
  const modelKey = selected?.apiKey || env.UPSTREAM_API_KEY;
  const upstreamModel = selected?.upstream_model || env.MODEL_ID;
  if (!Array.isArray(body.messages) || !body.messages.length)
    return err('Unsupported model or messages');
  // Paid provider adapter: non-streaming text only until usage accounting supports streams.
  if (body.stream || body.tools || body.tool_choice || body.n || body.response_format)
    return err('Unsupported request options');
  const maxTokens = Math.min(4096, Math.max(1, Number(body.max_tokens || 1024)));
  if (!Number.isSafeInteger(maxTokens)) return err('Invalid max_tokens');
  const inputPrice = Number(selected?.input_micro_krw ?? env.PRICE_INPUT_MICRO_KRW), outputPrice = Number(selected?.output_micro_krw ?? env.PRICE_OUTPUT_MICRO_KRW);
  if (![inputPrice, outputPrice].every(n => Number.isSafeInteger(n) && n > 0)) return err('Invalid pricing configuration', 503);
  const encoded = JSON.stringify(body.messages);
  if (encoded.length > 100000) return err('Context too large');
  // Conservative upper bound: charge for all input bytes as tokens, plus max output tokens.
  const reserve = encoded.length * inputPrice + maxTokens * outputPrice;
  if (!Number.isSafeInteger(reserve) || reserve > 1000000000) return err('Request exceeds limit');
  // Operator-side daily ceiling is independent of users' prepaid balances.
  // Count outstanding reservations; strict concurrency enforcement needs an atomic budget ledger.
  const dailyBudget = Number(env.PAID_DAILY_BUDGET_MICRO_KRW);
  if (!env.PAID_DAILY_BUDGET_MICRO_KRW || !Number.isSafeInteger(dailyBudget) || dailyBudget <= 0)
    return err('Operator daily paid budget must be configured', 503);
  {
    const dayStart = Math.floor(Date.now() / 86400000) * 86400;
    const spent = await env.DB.prepare(
      "SELECT COALESCE(SUM(CASE WHEN status='reserved' THEN reserved ELSE charged END),0) AS total FROM usage WHERE created_at>=?"
    ).bind(dayStart).first();
    if (Number(spent?.total || 0) + reserve > dailyBudget)
      return err('Operator daily model budget reached', 429);
  }
  const id = crypto.randomUUID();
  const debited = await env.DB.prepare('UPDATE accounts SET credits=credits-? WHERE id=? AND credits>=? RETURNING credits')
    .bind(reserve, user.id, reserve).first();
  if (!debited) return err('Insufficient prepaid credits', 402);
  await env.DB.prepare('INSERT INTO usage(id,account_id,model,reserved,status,created_at) VALUES(?,?,?,?,?,?)')
    .bind(id, user.id, modelId, reserve, 'reserved', now()).run();
  let charge = 0, status = 'failed', result;
  try {
    const upstream = await fetch(modelEndpoint.replace(/\/$/, '') + '/chat/completions', {
      method: 'POST',
      headers: { authorization: 'Bearer ' + modelKey, 'content-type': 'application/json' },
      body: JSON.stringify({ model: upstreamModel, messages: body.messages, max_tokens: maxTokens, stream: false }),
      signal: AbortSignal.timeout(90000)
    });
    if (!upstream.ok) throw new Error('Upstream unavailable (' + upstream.status + ')');
    result = await upstream.json();
    const input = result.usage?.prompt_tokens, output = result.usage?.completion_tokens;
    if (!Number.isSafeInteger(input) || !Number.isSafeInteger(output) || input < 0 || output < 0)
      throw new Error('Upstream usage unavailable');
    charge = Math.min(reserve, input * inputPrice + output * outputPrice);
    if (!Number.isSafeInteger(charge)) throw new Error('Invalid usage');
    status = 'completed';
  } catch (e) {
    result = { error: 'Model request failed', detail: String(e.message || e) };
  } finally {
    // Reconcile against the initial reservation; never allow a negative credit balance.
    await env.DB.batch([
      env.DB.prepare('UPDATE accounts SET credits=credits+? WHERE id=?').bind(reserve - charge, user.id),
      env.DB.prepare('UPDATE usage SET charged=?,status=? WHERE id=?').bind(charge, status, id)
    ]);
  }
  return json(result, status === 'completed' ? 200 : 502);
}
async function routeCompletion(request, env, user) {
  const raw = await request.clone().text();
  let body;
  try { body = JSON.parse(raw); } catch { return err('Invalid JSON'); }
  const available = await availableModels(env);
  if (available.some(x => x.id === body.model)) return freeProxy(request, env, user);
  if (body.model === 'dominion/free-auto') {
    const candidates = available.filter(x => x.free).slice(0, 5);
    if (!candidates.length) return err('No healthy free models available', 503);
    for (const candidate of candidates) {
      const attempted = new Request(request, { body: JSON.stringify({ ...body, model: candidate.id }) });
      const response = await freeProxy(attempted, env, user);
      if (response.ok || response.status === 429) return response;
    }
    return err('All free models temporarily unavailable', 503);
  }
  return proxy(request, env, user);
}
export default {
  async scheduled(_event, env, ctx) {
    ctx.waitUntil(availableModels(env, true));
  },
  async fetch(request, env) {
    const origin = request.headers.get('origin');
    const headers = cors(origin, env);
    if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers });
    if (origin && origin !== env.ALLOWED_ORIGIN) return err('Origin not allowed', 403);
    if (new URL(request.url).pathname === '/health' && request.method === 'GET') return json({service:'DOMINION Cloud',status:'online',login_configured:Boolean(env.GOOGLE_CLIENT_ID),models_configured:Boolean(freeModels(env).length)});
    if (!env.DB || !env.ALLOWED_ORIGIN) return err('Service not configured', 503);
    try {
      const path = new URL(request.url).pathname;
      let response;
      if (path === '/v1/auth/google' && request.method === 'POST') response = await login(request, env);
      else if (['/v1/auth/kakao/start','/v1/auth/naver/start'].includes(path) && request.method === 'GET')
        response = await oauthStart(request,env,path.split('/')[3]);
      else if (['/v1/auth/kakao/callback','/v1/auth/naver/callback'].includes(path) && request.method === 'GET')
        response = await oauthCallback(request,env,path.split('/')[3]);
      else if (path === '/v1/models' && request.method === 'GET') {
        const user = await account(request, env);
        response = user ? json({ data: [{ id: 'dominion/free-auto', name: 'DOMINION Free Auto', object: 'model', free: true }, ...(await availableModels(env)).map(x => ({ id: x.id, name: x.name || x.id, object: 'model', free: true })), ...paidModels(env).map(x => ({ id: x.id, name: x.name, object: 'model', free: false, provider: x.provider, input_micro_krw: x.input_micro_krw, output_micro_krw: x.output_micro_krw }))] }) : err('Unauthorized', 401);
      }
      else if (path === '/v1/billing' && request.method === 'GET') {
        const user = await account(request, env);
        response = user ? await billingSummary(env, user) : err('Unauthorized',401);
      } else if (path === '/v1/billing/payments' && request.method === 'GET') {
        const user = await account(request, env);
        response = user ? await paymentHistory(env, user) : err('Unauthorized',401);
      } else if (path === '/v1/identity' && request.method === 'GET') {
        const user=await account(request,env);
        response=user ? json(await identityStatus(env,user)) : err('Unauthorized',401);
      } else if (path === '/v1/identity/start' && request.method === 'POST') {
        const user=await account(request,env);
        response=user ? await identityStart(env,user) : err('Unauthorized',401);
      } else if (path === '/v1/identity/confirm' && request.method === 'POST') {
        const user=await account(request,env);
        response=user ? await identityConfirm(request,env,user) : err('Unauthorized',401);
      } else if (path === '/v1/billing/checkout' && request.method === 'POST') {
        const user = await account(request, env);
        response = user ? await checkout(request,env,user) : err('Unauthorized',401);
      } else if (path === '/v1/billing/confirm' && request.method === 'POST') {
        const user = await account(request, env);
        response = user ? await confirmCheckout(request,env,user) : err('Unauthorized',401);
      } else if (path === '/v1/billing/orders' && request.method === 'GET') {
        const user = await account(request, env);
        if (!user) response = err('Unauthorized', 401);
        else {
          const orders = await env.DB.prepare('SELECT id,plan,amount_krw,status,created_at,paid_at FROM checkout_orders WHERE account_id=? ORDER BY created_at DESC LIMIT 30').bind(user.id).all();
          response = json({ orders: orders.results || [], settlement: 'managed_by_payment_provider' });
        }
      } else if (path === '/v1/me' && request.method === 'GET') {
        const user = await account(request, env);
        response = user ? json({ account_id: user.id, credits_micro_krw: user.credits }) : err('Unauthorized', 401);
      } else if (path === '/v1/chat/completions' && request.method === 'POST') {
        const user = await account(request, env);
        response = user ? await routeCompletion(request, env, user) : err('Unauthorized', 401);
      } else response = err('Not found', 404);
      const out = new Response(response.body, response);
      for (const [k, v] of Object.entries(headers)) out.headers.set(k, v);
      return out;
    } catch (e) {
      return json({ error: 'Request rejected', detail: String(e.message || e) }, 400, headers);
    }
  }
};
