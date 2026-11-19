/** Dominion-owned cloud boundary; no dependency on Nous Portal or Hermes. */
export type DominionModel = {
  id: string;
  provider: string;
  upstreamModel: string;
  supportsTools: boolean;
  maxOutputTokens: number;
  /** Price in integer micro-KRW per token; server-owned, never client-controlled. */
  inputMicroKrwPerToken: number;
  outputMicroKrwPerToken: number;
  availability: "free" | "prepaid";
};

export type VerifiedDominionSession = {
  userId: string;
  sessionId: string;
  expiresAt: number;
};

export type CreditReservation = {
  id: string;
  userId: string;
  requestId: string;
  modelId: string;
  reservedMicroKrw: number;
  state: "reserved" | "settled" | "disputed";
};

export type TrustedUsage = {
  promptTokens: number;
  completionTokens: number;
  providerReceiptId?: string;
};

export interface DominionIdentity {
  /** Verify server-side signature, issuer, audience, expiry and session revocation. */
  verifySession(authorization: string): Promise<VerifiedDominionSession>;
}

export interface DominionBilling {
  /** Atomically check user quota and reserve worst-case authorized spend. */
  reserve(session: VerifiedDominionSession, model: DominionModel, requestId: string,
    maxPromptTokens: number, maxOutputTokens: number): Promise<CreditReservation>;
  /** Idempotently reconcile provider-reported usage against a reservation. */
  settle(reservationId: string, usage: TrustedUsage): Promise<void>;
  /** Do not silently refund ambiguous provider outcomes; mark for reconciliation. */
  dispute(reservationId: string, reason: string): Promise<void>;
}

export interface DominionModelRegistry {
  /** Operator-controlled model registry; no arbitrary upstream URLs from clients. */
  find(id: string): Promise<DominionModel | null>;
}

export interface DominionUpstream {
  /** Credentials are resolved from server secrets, not supplied by the client. */
  complete(model: DominionModel, messages: unknown[], maxOutputTokens: number): Promise<{
    response: unknown;
    usage: TrustedUsage;
  }>;
}
