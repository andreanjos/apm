import type { OperationControlState, OperationScope } from "./view-model";

export type OperationControls = Record<OperationScope, OperationControlState | null>;

export function createOperationControls(): OperationControls {
  return {
    sync: null,
    lifecycle: null,
    library: null,
    model: null,
  };
}

export function startOperationControl(): OperationControlState {
  return { operationId: null, canceling: false };
}

export function rememberOperationId(
  operation: OperationControlState | null,
  operationId: string,
): OperationControlState {
  if (!operation || operation.operationId === operationId) {
    return operation ?? { operationId, canceling: false };
  }

  return { ...operation, operationId };
}

export function setOperationCanceling(
  controls: OperationControls,
  scope: OperationScope,
  canceling: boolean,
): OperationControls {
  const operation = controls[scope];
  if (!operation) {
    return controls;
  }

  return {
    ...controls,
    [scope]: { ...operation, canceling },
  };
}
