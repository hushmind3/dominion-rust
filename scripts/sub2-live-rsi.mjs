/**
 * Sub2 live-model RSI campaign probe.
 *
 * Uses an actual local Ollama model and the actual DOMINION HTTP engine.
 * Never fabricates acceptance, baseline measurements, or descendant execution.
 * Writes a receipt even when the model cannot edit code or the campaign fails.
 * Non-zero exit means the first independently measured improvement was NOT proved.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";

const base = process.env.DOMINION_LIVE_BASE || "http://127.0.0.1:44327";
const ollama = process.env.DOMINION_LIVE_OLLAMA || "http://127.0.0.1:11434";
const model = process.env.DOMINION_LIVE_MODEL || "qwen3:4b-instruct";
const output = resolve(process.env.DOMINION_LIVE_REPORT || "benchmark/sub2-live-rsi-latest.json");
const started = new Date().toISOString();
const originalCommit = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
const timeoutMs = Number(process.env.DOMINION_LIVE_TIMEOUT_MS || 1_800_000);
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const terminal = new Set(["completed", "failed", "cancelled", "needs_review"]);
const receipt = {
  schemaVersion: 1, kind: "dominion.sub2.actual_model.rsi",
  started, originalCommit, branch: "dominion-sub2", model,
  source: "live_ollama_and_native_dominion_engine",
  preflight: null, providerId: null, campaignId: null, status: "initializing",
  completedAt: null, history: [], firstMeasuredImprovement: false,
  descendantInheritedImprovement: false, capabilityDemonstrated: false,
  acceptedGenerations: 0, error: null
};

function persist() {
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, JSON.stringify(receipt, null, 2) + "\n");
}
async function json(url, method = "GET", body, timeout = 25_000) {
  const response = await fetch(url, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(timeout),
  });
  const text = await response.text();
  let value;
  try { value = JSON.parse(text); } catch { value = { body: text.slice(0, 2000) }; }
  if (!response.ok) throw new Error(`${method} ${new URL(url).pathname}: ${response.status} ${JSON.stringify(value).slice(0,1000)}`);
  return value;
}
async function healthy() {
  for (let n = 0; n < 90; n++) {
    try { const response = await json(base + "/api/health"); if (response) return; }
    catch { await wait(1500); }
  }
  throw new Error("DOMINION native engine did not become available");
}
function observedGeneration(g) {
  const comparison = g?.evaluation || {};
  const metrics = comparison?.metrics || {};
  const pairedProof = g?.accepted === true
    && g?.acceptedCommit && g?.candidateId
    && typeof comparison?.evidenceSha256 === "string"
    && /^[a-f0-9]{64}$/i.test(comparison.evidenceSha256)
    && comparison?.verdict === "improved"
    && metrics?.eligible === true
    && Number(metrics?.improvedHoldouts || 0) > 0
    && Number(metrics?.regressions || 0) === 0
    && Number(metrics?.pairedCases || 0) >= 8;
  return {
    attempt: g?.attempt, generatorCommit: g?.generatorCommit || null,
    generatorBinarySha256: g?.generatorBinarySha256 || null,
    workerMarker: g?.workerMarker || null, model: g?.generatorModel || null,
    tokens: g?.tokens ?? null, elapsedMs: g?.elapsedMs ?? null,
    tokenUsage: g?.tokenUsage ?? null,
    sourceContextChars: g?.sourceContextChars ?? null,
    totalPromptChars: g?.totalPromptChars ?? null,
    actionDirective: g?.actionDirective ?? null,
    actionObservation: g?.actionObservation ?? null,
    selfRepair: g?.selfRepair ?? null,
    methodExecutions: (g?.methodExecutions || []).slice(0, 12).map((entry) => ({
      methodId: entry?.methodId ?? null,
      revision: entry?.revision ?? null,
      operationIds: entry?.operationIds ?? [],
      verified: entry?.result?.verified === true,
      followed: entry?.result?.followed === true,
      status: entry?.result?.status ?? null,
    })),
    editActions: g?.editActions || [],
    sourceDiffBytes: g?.sourceDiffBytes ?? null,
    novelSourceDiffBytes: g?.novelSourceDiffBytes ?? null,
    inheritedPatchUnchanged: g?.inheritedPatchUnchanged === true,
    sourceCaptureError: g?.sourceCaptureError ?? null,
    unverifiedCandidateId: g?.unverifiedCandidateId ?? null,
    staticChecksPassed: g?.staticChecksPassed === true,
    toolsUsed: g?.toolsUsed || [], domain: g?.domain || null,
    focusFiles: g?.focusFiles || [], domainSelection: g?.domainSelection || null,
    candidateId: g?.candidateId || null, accepted: g?.accepted === true,
    acceptedCommit: g?.acceptedCommit || null,
    verdict: comparison?.verdict || null,
    pairedCases: metrics?.pairedCases ?? null, improvedHoldouts: metrics?.improvedHoldouts ?? null,
    metricGains: metrics?.metricGains ?? null, regressions: metrics?.regressions ?? null,
    evidenceSha256: comparison?.evidenceSha256 || null, measuredGain: Boolean(pairedProof),
    error: g?.error || null, stopReason: g?.stopReason || null,
    nextTrial: g?.trialOutcome || null,
  };
}
async function main() {
  persist();
  await healthy();
  const tags = await json(ollama + "/api/tags");
  const installed = (tags.models || []).find((m) => m.name === model || m.model === model);
  if (!installed) throw new Error(`Model ${model} is not installed in Ollama`);
  receipt.modelDigest = installed.digest || null;
  // Separate tool-call preflight: report what the REAL model did.
  // A failed tool-call capability is diagnostic, never auto-repaired with a fake response.
  const preflight = await json(ollama + "/api/chat", "POST", {
    model, stream: false, think: false,
    options: { temperature: 0, num_ctx: 16384 },
    messages: [{ role: "user", content: "Find the Rust symbol named memory_retrieve. Use code_search with query memory_retrieve before answering." }],
    tools: [{ type: "function", function: {
      name: "code_search", description: "Find source code symbols",
      parameters: { type: "object", properties: { query: { type: "string" } }, required: ["query"] }
    }}]
  }, 180_000);
  const nativeCalls = preflight?.message?.tool_calls || [];
  const resident = await json(ollama + "/api/ps");
  const activeModel = (resident.models || []).find((item) =>
    item.name === model || item.model === model);
  const contextLength = activeModel?.context_length ?? null;
  receipt.preflight = {
    serverContextLength: contextLength,
    modelReturnedToolCalls: nativeCalls.length,
    totalDurationNs: preflight?.total_duration ?? null,
    evalCount: preflight?.eval_count ?? null,
    responseContentPresent: Boolean(preflight?.message?.content?.trim()),
    // Never describe generated text as an actual executed tool call.
  };
  // The model just demonstrated native structured tool calls against the
  // actual Ollama API. Do not downgrade it to a text-only tool manual:
  // DOMINION would omit the OpenAI tools array and lose structured execution.
  // Keep the compatibility path for models that did not demonstrate tools.
  receipt.providerToolMode = nativeCalls.some((call) =>
    call?.function?.name === "code_search") ? "native" : "compat";
  persist();
  if (typeof contextLength === "number" && contextLength < 8192) {
    throw new Error(`Ollama effective context is only ${contextLength} tokens; need at least 8192 for indexed code tools`);
  }

  const provider = await json(base + "/api/providers", "POST", {
    name: "Sub2 isolated live Ollama", kind: "ollama", local: true, enabled: true,
    baseUrl: ollama, model, toolMode: receipt.providerToolMode, contextTokens: 16384,
  });
  receipt.providerId = provider.id;
  if (!receipt.providerId) throw new Error("Provider was not persisted by DOMINION");

  // A REAL first-improvement attempt: the native worktree coordinator,
  // the model's actual tool calls, and the built-in frozen native suite.
  const job = await json(base + "/api/rsi/start", "POST", {
    mode: "continuous", confirmation: "START_RSI_EVOLUTION",
    providerId: receipt.providerId, model, selectionPolicy: "automatic",
    request: "src/engine/memory.rs의 bm25_scores 함수를 실제로 최적화하세요. 현재 구현은 query_terms 각 항목마다 documents 전체를 두 번 순회하고 term별 Vec<f64>를 할당한 뒤 fold합니다. 정확히 같은 BM25 결과와 모든 기억 회상 기능을 유지하면서 중간 Vec 할당을 줄이고 계산을 재사용하는 작은 Rust 수정 1개를 구현하세요. 제공된 코드 인덱스에서 bm25_scores 위치를 찾아 현재 함수 원문을 정확히 읽고, workspace_replace의 path/startLine/endLine/newText/expectedSha256 인자로 실제 파일을 수정하세요. 네이티브 함수 호출이 불가능하면 정확히 동일한 인자들의 단일 JSON 객체로 출력하세요. 설명이나 조사로 완료하지 마세요. 저장 후 Rust 컴파일과 동결 비교 품질 판정은 엔진이 독립 실행합니다.",
    focusFiles: ["src/engine/memory.rs"],
    maxAttempts: 2, tokenBudget: 85_000, timeBudgetSeconds: 1800,
  }, 120_000);
  receipt.campaignId = job.id;
  if (!receipt.campaignId) throw new Error("No native RSI job ID returned");
  receipt.status = "running";
  persist();

  const deadline = Date.now() + timeoutMs;
  let campaign = null;
  while (Date.now() < deadline) {
    try {
      campaign = await json(base + "/api/rsi/campaigns/" + encodeURIComponent(receipt.campaignId));
      receipt.status = campaign.status || "running";
      receipt.acceptedGenerations = Number(campaign.acceptedGenerations || 0);
      receipt.tokens = campaign.tokens ?? null;
      receipt.history = (campaign.history || []).map(observedGeneration);
      receipt.capabilityDemonstrated = campaign.capabilityDemonstrated === true;
      receipt.error = campaign.error || null;
      persist();
      if (terminal.has(receipt.status)) break;
    } catch (error) { receipt.lastPollError = String(error); persist(); }
    await wait(10_000);
  }
  if (!terminal.has(receipt.status)) {
    receipt.status = "timeout";
    receipt.error = "Live native experiment did not reach a terminal state before the deadline";
  }
  receipt.firstMeasuredImprovement = receipt.history.some((g) => g.measuredGain);
  receipt.descendantInheritedImprovement = receipt.history.some((g, i) => g.measuredGain
    && receipt.history.slice(i + 1).some((next) =>
      next.generatorCommit === g.acceptedCommit
      && next.generatorBinarySha256
      && next.generatorBinarySha256 !== g.generatorBinarySha256));
  receipt.completedAt = new Date().toISOString();
  persist();
  console.log(JSON.stringify({
    campaignId: receipt.campaignId, model, preflight: receipt.preflight,
    providerToolMode: receipt.providerToolMode,
    status: receipt.status, attempts: receipt.history.length,
    acceptedGenerations: receipt.acceptedGenerations,
    firstMeasuredImprovement: receipt.firstMeasuredImprovement,
    descendantInheritedImprovement: receipt.descendantInheritedImprovement,
    capabilityDemonstrated: receipt.capabilityDemonstrated, error: receipt.error,
  }, null, 2));
  if (!receipt.firstMeasuredImprovement) {
    throw new Error("Actual-model independently measured first RSI improvement NOT demonstrated; evidence saved");
  }
}
try { await main(); }
catch (error) {
  receipt.error = String(error);
  if (!terminal.has(receipt.status) && receipt.status !== "timeout") receipt.status = "probe_failed";
  receipt.completedAt = new Date().toISOString();
  persist();
  console.error(receipt.error);
  process.exitCode = 1;
}
