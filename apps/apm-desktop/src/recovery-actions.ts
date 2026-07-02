import type { OperationRecoverySummary } from "./types";

export function recoveryRetryActionLabel(recovery: OperationRecoverySummary) {
  const count = recovery.retryable_count;
  if (count === 0) {
    return null;
  }

  return count === 1 ? "Retry ready" : `Retry ${count} ready`;
}
