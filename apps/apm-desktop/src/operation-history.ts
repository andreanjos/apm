import type {
  ModelRunResult,
  OperationKind,
  OperationRecoveryCandidate,
  OperationRecoverySummary,
  OperationResult,
  OperationState,
  OperationStatus,
} from "./types";
import {
  hasActiveOperationLock,
  operationKindScope,
  type OperationScopeLocks,
  type OperationProgressScope,
} from "./operation-events";
import { recoveryRetryActionLabel } from "./recovery-actions";
import { escapeHtml } from "./view-utils";

const recentOperationTime = new Intl.DateTimeFormat([], {
  hour: "2-digit",
  minute: "2-digit",
});
const RECENT_OPERATION_LIMIT = 3;

export function operationHistorySection(
  operations: OperationStatus[],
  recovery: OperationRecoverySummary,
  retryingOperationId: string | null,
  retryingRecovery: boolean,
  retryLocks: OperationScopeLocks,
) {
  const recoveryByOperation = recoveryCandidatesByOperation(recovery);
  const visibleOperations = visibleOperationHistory(
    operations,
    recoveryByOperation,
  );
  const operationItems = visibleOperations
    .map((operation) =>
      operationHistoryItem(
        operation,
        recoveryByOperation.get(operation.operation_id) ?? null,
        retryingOperationId,
        retryingRecovery,
        retryLocks,
      ),
    )
    .join("");

  return `
    <section class="panel operations-panel">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Diagnostics</p>
          <h2>Operation history</h2>
        </div>
        <div class="operation-history-actions">
          ${recoveryRetryActionMarkup(
            recovery,
            retryingOperationId,
            retryingRecovery,
            retryLocks,
          )}
          <span class="status-pill">${escapeHtml(
            operationHistoryStatus(recovery, operations.length),
          )}</span>
        </div>
      </div>
      <div class="operation-history-list">
        ${
          visibleOperations.length > 0
            ? operationItems
            : `<div class="operation-history-empty">No operations recorded yet.</div>`
        }
      </div>
    </section>
  `;
}

function recoveryRetryActionMarkup(
  recovery: OperationRecoverySummary,
  retryingOperationId: string | null,
  retryingRecovery: boolean,
  retryLocks: OperationScopeLocks,
) {
  const actionLabel = recoveryRetryActionLabel(recovery);
  if (!actionLabel) {
    return "";
  }

  const state = recoveryRetryActionState(
    actionLabel,
    retryingOperationId,
    retryingRecovery,
    retryLocks,
  );

  return `
    <button
      class="operation-history-retry recovery-retry"
      data-retry-recovery="true"
      type="button"
      aria-label="${escapeHtml(state.ariaLabel)}"
      title="${escapeHtml(state.title)}"
      ${state.disabled ? "disabled" : ""}
    >
      <i data-lucide="refresh-cw" aria-hidden="true"></i>
      <span>${escapeHtml(state.label)}</span>
    </button>
  `;
}

function visibleOperationHistory(
  operations: OperationStatus[],
  recoveryByOperation: Map<string, OperationRecoveryCandidate>,
) {
  const visible = operations.slice(-RECENT_OPERATION_LIMIT).reverse();
  if (recoveryByOperation.size === 0) {
    return visible;
  }

  const visibleIds = new Set(visible.map((operation) => operation.operation_id));
  for (const operation of operations.slice().reverse()) {
    if (
      !recoveryByOperation.has(operation.operation_id) ||
      visibleIds.has(operation.operation_id)
    ) {
      continue;
    }

    visible.push(operation);
    visibleIds.add(operation.operation_id);
  }

  return visible;
}

function operationHistoryStatus(
  recovery: OperationRecoverySummary,
  retainedOperationCount: number,
) {
  if (recovery.retryable_count > 0) {
    return `${recovery.retryable_count} retry ready / ${retainedOperationCount} retained`;
  }
  if (recovery.interrupted_count > 0) {
    return `${recovery.interrupted_count} interrupted / ${retainedOperationCount} retained`;
  }

  return `${retainedOperationCount} retained`;
}

function operationHistoryItem(
  operation: OperationStatus,
  recovery: OperationRecoveryCandidate | null,
  retryingOperationId: string | null,
  retryingRecovery: boolean,
  retryLocks: OperationScopeLocks,
) {
  const stateDetail = operationStateDetail(operation, recovery);
  const time = operationTime(operation);
  const eventCount = operation.events.length;
  const retryable = isRetryableOperation(operation);
  const retryActionState = operationRetryActionState(
    operation,
    retryingOperationId,
    retryingRecovery,
    retryLocks,
  );
  return `
    <article class="operation-history-item ${operationStateClass(operation.state, recovery)}">
      <div class="operation-history-marker" aria-hidden="true"></div>
      <div class="operation-history-main">
        <div class="operation-history-title">
          <span>${escapeHtml(operationKindLabel(operation.kind))}</span>
          <strong>${escapeHtml(stateDetail)}</strong>
        </div>
        <div class="operation-history-meta">
          <span>${escapeHtml(shortOperationId(operation.operation_id))}</span>
          <span>${eventCount} event${eventCount === 1 ? "" : "s"}</span>
          <span>${escapeHtml(time)}</span>
        </div>
        ${operationDetailMarkup(operation)}
        ${retryable ? retryButtonMarkup(operation, retryActionState) : ""}
      </div>
    </article>
  `;
}

function operationDetailMarkup(operation: OperationStatus) {
  const message =
    operationResultMessage(operation.result) ?? operation.error?.trim();
  if (!message) {
    return "";
  }

  return `<div class="operation-history-detail">${escapeHtml(message)}</div>`;
}

function operationResultMessage(result?: OperationResult | null) {
  if (!result) {
    return null;
  }

  switch (result.kind) {
    case "model_run":
      return modelRunResultMessage(result.result);
    case "library_scan":
      return `Scanned ${result.result.scanned_count}; matched ${result.result.matched_count}; adopted ${result.result.adopted_count}`;
    case "registry_sync":
    case "install_package":
    case "update_package":
    case "remove_package":
    case "model_weight_pull":
    case "model_install":
      return null;
  }
}

function modelRunResultMessage(result: ModelRunResult) {
  switch (result.status) {
    case "completed":
      return `${result.package_id}: ${result.message}`;
    case "blocked":
      return `${result.package_id}: ${result.message}`;
  }
}

type RetryActionState = {
  disabled: boolean;
  label: string;
  title: string;
  ariaLabel: string;
};

function recoveryRetryActionState(
  actionLabel: string,
  retryingOperationId: string | null,
  retryingRecovery: boolean,
  retryLocks: OperationScopeLocks,
): RetryActionState {
  if (retryingRecovery) {
    return retryingActionState();
  }
  if (retryingOperationId !== null) {
    return retryBusyActionState();
  }
  if (hasActiveOperationLock(retryLocks)) {
    return activeRecoveryOperationActionState();
  }

  return {
    disabled: false,
    label: actionLabel,
    title: actionLabel,
    ariaLabel: actionLabel,
  };
}

function operationRetryActionState(
  operation: OperationStatus,
  retryingOperationId: string | null,
  retryingRecovery: boolean,
  retryLocks: OperationScopeLocks,
): RetryActionState {
  if (retryingRecovery || retryingOperationId === operation.operation_id) {
    return retryingActionState();
  }
  if (retryingOperationId !== null) {
    return retryBusyActionState();
  }

  const scope = operationKindScope(operation.kind);
  if (retryLocks[scope]) {
    return activeOperationActionState(operationScopeLockLabel(scope));
  }

  return enabledRetryActionState(`Retry ${operationKindLabel(operation.kind)} operation`);
}

function enabledRetryActionState(ariaLabel: string): RetryActionState {
  return {
    disabled: false,
    label: "Retry",
    title: "Retry operation",
    ariaLabel,
  };
}

function retryingActionState(): RetryActionState {
  return {
    disabled: true,
    label: "Retrying",
    title: "Retry in progress",
    ariaLabel: "Retry in progress",
  };
}

function retryBusyActionState(): RetryActionState {
  return {
    disabled: true,
    label: "Retry",
    title: "Retry already running",
    ariaLabel: "Retry already running",
  };
}

function activeOperationActionState(lockLabel: string): RetryActionState {
  return {
    disabled: true,
    label: "Retry",
    title: `${lockLabel} running`,
    ariaLabel: `${lockLabel} running; retry unavailable`,
  };
}

function activeRecoveryOperationActionState(): RetryActionState {
  return {
    disabled: true,
    label: "Retry",
    title: "Operation running",
    ariaLabel: "Operation running; retry unavailable",
  };
}

function operationScopeLockLabel(scope: OperationProgressScope) {
  switch (scope) {
    case "sync":
      return "Sync operation";
    case "lifecycle":
      return "Install operation";
    case "library":
      return "Library operation";
    case "model":
      return "Model action";
  }
}

function retryButtonMarkup(operation: OperationStatus, actionState: RetryActionState) {
  return `
    <button
      class="operation-history-retry"
      data-retry-operation-id="${escapeHtml(operation.operation_id)}"
      type="button"
      aria-label="${escapeHtml(actionState.ariaLabel)}"
      title="${escapeHtml(actionState.title)}"
      ${actionState.disabled ? "disabled" : ""}
    >
      <i data-lucide="refresh-cw" aria-hidden="true"></i>
      <span>${escapeHtml(actionState.label)}</span>
    </button>
  `;
}

function recoveryCandidatesByOperation(recovery: OperationRecoverySummary) {
  return new Map(
    recovery.candidates.map((candidate) => [candidate.operation_id, candidate]),
  );
}

function operationStateDetail(
  operation: OperationStatus,
  recovery: OperationRecoveryCandidate | null,
) {
  if (recovery?.retryable) {
    return "Interrupted / retry ready";
  }
  if (recovery) {
    return "Interrupted / manual review";
  }

  const state = operationStateLabel(operation.state);
  return operation.request ? `${state} / request saved` : state;
}

function operationKindLabel(kind: OperationKind) {
  switch (kind) {
    case "registry_sync":
      return "Registry sync";
    case "library_scan":
      return "Library scan";
    case "install_url":
      return "Direct install";
    case "install_archive":
      return "Archive install";
    case "package_update":
      return "Package update";
    case "package_remove":
      return "Package remove";
    case "model_weight_pull":
      return "Model weight pull";
    case "model_install":
      return "Model install";
    case "model_run":
      return "Model run";
  }
}

function operationStateLabel(state: OperationState) {
  switch (state) {
    case "queued":
      return "Queued";
    case "running":
      return "Running";
    case "cancel_requested":
      return "Cancel requested";
    case "canceled":
      return "Canceled";
    case "succeeded":
      return "Succeeded";
    case "failed":
      return "Failed";
  }
}

function operationStateClass(
  state: OperationState,
  recovery: OperationRecoveryCandidate | null,
) {
  if (recovery) {
    return "recoverable";
  }

  switch (state) {
    case "succeeded":
      return "succeeded";
    case "failed":
    case "canceled":
      return "failed";
    case "running":
    case "queued":
    case "cancel_requested":
      return "active";
  }
}

function operationTime(operation: OperationStatus) {
  const timestamp =
    operation.finished_at ?? operation.started_at ?? operation.created_at;
  const date = new Date(timestamp);
  return Number.isNaN(date.valueOf())
    ? timestamp
    : recentOperationTime.format(date);
}

function isRetryableOperation(operation: OperationStatus) {
  return (
    !!operation.request &&
    (operation.state === "failed" || operation.state === "canceled")
  );
}

function shortOperationId(operationId: string) {
  return operationId.length > 16 ? operationId.slice(-16) : operationId;
}
