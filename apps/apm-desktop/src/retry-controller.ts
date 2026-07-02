import {
  retryRecoveryOperationsCommand,
  retryOperationCommand,
} from "./commands";
import {
  hasActiveOperationLock,
  isInstallEvent,
  isLifecycleEvent,
  isModelEvent,
  isRegistryEvent,
  operationProgressScope,
  type OperationProgressScope,
  type OperationProgress,
  type OperationScopeLocks,
  registryProgressLabel,
  withOperationProgress,
} from "./operation-events";
import {
  retryProgressMessage,
  retryRecoveryStatusMessage,
  retryStatusMessage,
} from "./retry-status";
import type {
  InstallEvent,
  LifecycleEvent,
  ModelOperationEvent,
} from "./types";
import type { LifecycleNotice } from "./view-model";

export type RetryControllerState = {
  retryingOperationId: string | null;
  retryingRecovery: boolean;
};

type RetryControllerHost = {
  retryableRecoveryCount(): number;
  activeOperationLocks(): OperationScopeLocks;
  retryOperationScope(operationId: string): OperationProgressScope | null;
  setSyncStatus(message: string): void;
  setLifecycleNotice(notice: LifecycleNotice): void;
  setLibraryNotice(notice: LifecycleNotice): void;
  setModelNotice(notice: LifecycleNotice): void;
  appendLifecycleEvent(event: InstallEvent): void;
  appendLibraryEvent(event: LifecycleEvent): void;
  appendModelEvent(event: ModelOperationEvent): void;
  refreshSnapshotData(): Promise<void>;
  formatError(error: unknown): string;
  render(): void;
};

export function createRetryController(host: RetryControllerHost) {
  let retryingOperationId: string | null = null;
  let retryingRecovery = false;

  function state(): RetryControllerState {
    return { retryingOperationId, retryingRecovery };
  }

  async function retryOperation(operationId: string) {
    if (operationRetryBlocked(operationId)) {
      return;
    }

    retryingOperationId = operationId;
    host.render();

    await runRetryOperation(operationId);
    retryingOperationId = null;
    host.render();
  }

  async function retryRecoveryOperations() {
    const retryableCount = host.retryableRecoveryCount();
    if (
      retryableCount === 0 ||
      retryingOperationId ||
      retryingRecovery ||
      hasActiveOperationLock(host.activeOperationLocks())
    ) {
      return;
    }

    retryingRecovery = true;
    host.setSyncStatus(
      `Retrying ${retryableCount} recovery operation${retryableCount === 1 ? "" : "s"}`,
    );
    host.render();

    try {
      const statuses = await withOperationProgress(
        retryRecoveryOperationsCommand,
        appendRetryProgress,
      );
      host.setSyncStatus(retryRecoveryStatusMessage(statuses));
      await host.refreshSnapshotData();
    } catch (error) {
      host.setSyncStatus(host.formatError(error));
    }

    retryingRecovery = false;
    host.render();
  }

  async function runRetryOperation(operationId: string) {
    try {
      host.setSyncStatus("Retrying operation");
      const status = await withOperationProgress(
        (progressId) => retryOperationCommand(operationId, progressId),
        appendRetryProgress,
      );
      host.setSyncStatus(retryStatusMessage(status));
      await host.refreshSnapshotData();
    } catch (error) {
      host.setSyncStatus(host.formatError(error));
    }
  }

  function appendRetryProgress(progress: OperationProgress) {
    applyRetryProgress(progress, host);
  }

  function isOperationScopeLocked(scope: OperationProgressScope) {
    return host.activeOperationLocks()[scope];
  }

  function operationRetryBlocked(operationId: string) {
    if (retryingOperationId || retryingRecovery) {
      return true;
    }

    const scope = host.retryOperationScope(operationId);
    if (scope) {
      return isOperationScopeLocked(scope);
    }

    return hasActiveOperationLock(host.activeOperationLocks());
  }

  return {
    retryOperation,
    retryRecoveryOperations,
    state,
  };
}

export type RetryProgressHost = Pick<
  RetryControllerHost,
  | "setSyncStatus"
  | "setLifecycleNotice"
  | "setLibraryNotice"
  | "setModelNotice"
  | "appendLifecycleEvent"
  | "appendLibraryEvent"
  | "appendModelEvent"
  | "render"
>;

export function applyRetryProgress(
  progress: OperationProgress,
  host: RetryProgressHost,
) {
  const { event } = progress;
  const route = operationProgressScope(progress);
  if (route === "sync" && isRegistryEvent(event)) {
    host.setSyncStatus(`Retry: ${registryProgressLabel(event)}`);
    host.render();
    return;
  }
  if (route === "lifecycle" && isInstallEvent(event)) {
    host.setLifecycleNotice({
      tone: "info",
      message: retryProgressMessage(event),
    });
    host.appendLifecycleEvent(event);
    host.render();
    return;
  }
  if (route === "library" && isLifecycleEvent(event)) {
    host.setLibraryNotice({
      tone: "info",
      message: retryProgressMessage(event),
    });
    host.appendLibraryEvent(event);
    host.render();
    return;
  }
  if (route === "model" && isModelEvent(event)) {
    host.setModelNotice({
      tone: "info",
      message: retryProgressMessage(event),
    });
    host.appendModelEvent(event);
    host.render();
  }
}
