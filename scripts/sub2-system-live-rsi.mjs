/**
 * Sub2 actual-model WHOLE-SYSTEM self-redesign recursive campaign.
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
const timeoutMs = Number(process.env.DOMINION_LIVE_TIMEOUT_MS || 2_400_000);
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const terminal = new Set(["completed", "failed", "cancelled", "needs_review"]);
const receipt = {
  schemaVersion: 1, kind: "dominion.sub2.actual_model.whole_system_recursive_design",
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
    requestedModel: g?.requestedModel || null,
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
    nativeMutation: g?.nativeMutation ?? null,
    sourceDiffBytes: g?.sourceDiffBytes ?? null,
    novelSourceDiffBytes: g?.novelSourceDiffBytes ?? null,
    inheritedPatchUnchanged: g?.inheritedPatchUnchanged === true,
    sourceCaptureError: g?.sourceCaptureError ?? null,
    unverifiedCandidateId: g?.unverifiedCandidateId ?? null,
    staticChecksPassed: g?.staticChecksPassed === true,
    toolsUsed: g?.toolsUsed || [], domain: g?.domain || null,
    focusFiles: g?.focusFiles || [], domainSelection: g?.domainSelection || null,
    systemEvolution: g?.systemEvolution === true,
    systemBlueprintVerified: g?.systemBlueprintVerified === true,
    sourceTreeSha256: g?.systemBlueprint?.sourceTreeSha256 || null,
    systemDesign: g?.systemDesign || null,
    descendantPlan: g?.descendantPlan || null,
    candidateOrigin: g?.candidateOrigin || null,
    pairedRevalidated: g?.pairedRevalidated === true,
    acceptedSourceHashes: g?.acceptedSourceHashes || {},
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
    // Sub3's 3-generation completion used independently judged native source
    // induction rather than spending Qwen calls on already-matched code shapes.
    // The same detached source, compiled binaries and frozen oracle apply.
    nativeInduction: false, stopAfterProof: true, systemEvolution: true,
    request: "DOMINION 지능 시스템 전체를 자기재설계하라. 현재 후손 바이너리가 알고리즘, 실행 하네스, 장기 기억, 학습, 도구, 개선 방법을 모두 코드 수준에서 분석하고, 측정된 실패와 한계를 근거로 기존에 없던 설계 또는 실행 방법을 제안한 뒤 실제 Rust 구현으로 만들어라. 네이티브 고정 최적화 연산자를 사용하지 말고 모델이 rsi_system_plan을 실행해 검증 가능한 가설과 바꿀 소스 파일을 제출한 다음 workspace_read_range와 workspace_replace를 사용해 실제 코드를 수정해야 한다. 채택된 소스에서 컴파일된 다음 후손은 자신이 가진 새로운 알고리즘, 하네스, 기억, 학습, 도구와 개선 방법을 다시 살펴 직접 다음 설계를 고르고 구현해야 한다. 첫 세대의 판단을 복사하지 말고 채택된 시스템 설계와 독립 평가 결과를 계승해 지능 시스템의 다른 부분도 발전시켜라. 모델 기반 새로운 설계, 실제 변경된 모듈, 후손 계획 권한, 재빌드된 바이너리, 독립 A/B 무회귀 증거와 실제 적용 계보를 남겨라. 동결 검증기와 예산은 건드리지 말아라.",
    focusFiles: [],
    maxAttempts: 8, tokenBudget: 120_000, timeBudgetSeconds: 3000,
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
    // Keep real failure evidence visible even when artifact download is
    // unavailable: the tool names, exact edited bytes, independent metric
    // verdict and error are read from durable campaign records, never guessed.
    generations: receipt.history.map(g => ({
      attempt: g.attempt, model: g.model, domain: g.domain,
      focusFiles: g.focusFiles, elapsedMs: g.elapsedMs,
      sourceDiffBytes: g.sourceDiffBytes,
      novelSourceDiffBytes: g.novelSourceDiffBytes,
      toolCalls: g.toolsUsed.map(x => x.tool).slice(0, 20),
      editActions: g.editActions.slice(0, 8),
      nativeMutation: g.nativeMutation && {
        operator: g.nativeMutation.operator,
        origin: g.nativeMutation.origin,
        accepted: g.nativeMutation.accepted,
        candidateId: g.nativeMutation.candidateId,
        acceptedCommit: g.nativeMutation.acceptedCommit,
        qualityGain: g.nativeMutation.qualityGain,
      },
      candidateId: g.candidateId, unverifiedCandidateId: g.unverifiedCandidateId,
      staticChecksPassed: g.staticChecksPassed, sourceCaptureError: g.sourceCaptureError,
      independentVerdict: g.verdict, pairedCases: g.pairedCases,
      metricGains: g.metricGains, improvedHoldouts: g.improvedHoldouts,
      regressions: g.regressions, accepted: g.accepted, error: g.error,
    })),
  }, null, 2));
  // Completion is all-or-nothing. A compiled patch, a single held-out
  // adoption, or a rebuilt descendant is not recursive self-improvement.
  // The native campaign itself requires an accepted 3-generation lineage,
  // genuine improver gain and measured generalization to a new domain.
  if (!receipt.firstMeasuredImprovement
      || !receipt.descendantInheritedImprovement
      || !receipt.capabilityDemonstrated) {
    throw new Error("Full autonomous RSI NOT demonstrated: 3 adopted generations, meta-method improvement and new-domain transfer are required; evidence saved");
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
