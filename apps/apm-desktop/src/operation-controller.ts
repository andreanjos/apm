import { cancelOperationCommand } from "./commands";
import {
  isInstallEvent,
  isLifecycleEvent,
  isModelEvent,
  isRegistryEvent,
  operationProgressScope,
  type OperationProgress,
  registryProgressLabel,
  withOperationProgress,
} from "./operation-events";
import {
  createOperationControls,
  rememberOperationId,
  setOperationCanceling,
  startOperationControl,
  type OperationControls,
} from "./operation-controls";
import type {
  InstallEvent,
  LifecycleEvent,
  ModelOperationEvent,
} from "./types";
import type { LifecycleNotice, OperationScope } from "./view-model";

type OperationControllerHost = {
  setSyncStatus(message: string): void;
  setLifecycleNotice(notice: LifecycleNotice): void;
  setLibraryNotice(notice: LifecycleNotice): void;
  setModelNotice(notice: LifecycleNotice): void;
  appendLifecycleEvent(event: InstallEvent): void;
  appendLibraryEvent(event: LifecycleEvent): void;
  appendModelEvent(event: ModelOperationEvent): void;
  refreshSnapshotAfterOperationError(originalMessage: string): Promise<void>;
  formatError(error: unknown): string;
  render(): void;
};

export function createOperationController(host: OperationControllerHost) {
  let controls = createOperationControls();

  function state(): OperationControls {
    return {
      sync: cloneOperation(controls.sync),
      lifecycle: cloneOperation(controls.lifecycle),
      library: cloneOperation(controls.library),
      model: cloneOperation(controls.model),
    };
  }

  function start(scope: OperationScope) {
    controls = { ...controls, [scope]: startOperationControl() };
  }

  function clear(scope: OperationScope) {
    controls = { ...controls, [scope]: null };
  }

  async function runRegistry<T>(
    run: (progressId?: string) => Promise<T>,
  ): Promise<T> {
    return withOperationProgress(run, appendRegistryProgress);
  }

  async function runInstall<T>(
    run: (progressId?: string) => Promise<T>,
  ): Promise<T> {
    return withOperationProgress(run, appendInstallProgress);
  }

  async function runLibrary<T>(
    run: (progressId?: string) => Promise<T>,
  ): Promise<T> {
    return withOperationProgress(run, appendLibraryProgress);
  }

  async function runModel<T>(
    run: (progressId?: string) => Promise<T>,
  ): Promise<T> {
    return withOperationProgress(run, appendModelProgress);
  }

  async function cancelActiveOperation(scope: OperationScope) {
    const operation = controls[scope];
    if (!operation?.operationId || operation.canceling) {
      return;
    }

    controls = setOperationCanceling(controls, scope, true);
    host.render();

    try {
      const result = await cancelOperationCommand(operation.operationId);
      const tone = result.accepted ? "info" : "error";
      if (!result.accepted) {
        controls = setOperationCanceling(controls, scope, false);
      }
      setOperationNotice(scope, tone, result.message);
    } catch (error) {
      controls = setOperationCanceling(controls, scope, false);
      setOperationNotice(scope, "error", host.formatError(error));
    }
    host.render();
  }

  async function reportError(scope: OperationScope, error: unknown) {
    const message = host.formatError(error);
    setOperationNotice(scope, "error", message);
    await host.refreshSnapshotAfterOperationError(message);
  }

  function setOperationNotice(
    scope: OperationScope,
    tone: LifecycleNotice["tone"],
    message: string,
  ) {
    switch (scope) {
      case "sync":
        host.setSyncStatus(message);
        break;
      case "lifecycle":
        host.setLifecycleNotice({ tone, message });
        break;
      case "library":
        host.setLibraryNotice({ tone, message });
        break;
      case "model":
        host.setModelNotice({ tone, message });
        break;
    }
  }

  function appendRegistryProgress(progress: OperationProgress) {
    controls = {
      ...controls,
      sync: rememberOperationId(controls.sync, progress.operationId),
    };
    const { event } = progress;
    if (operationProgressScope(progress) !== "sync" || !isRegistryEvent(event)) {
      return;
    }
    host.setSyncStatus(registryProgressLabel(event));
    host.render();
  }

  function appendInstallProgress(progress: OperationProgress) {
    controls = {
      ...controls,
      lifecycle: rememberOperationId(controls.lifecycle, progress.operationId),
    };
    const { event } = progress;
    if (operationProgressScope(progress) !== "lifecycle" || !isInstallEvent(event)) {
      return;
    }
    host.appendLifecycleEvent(event);
    host.render();
  }

  function appendLibraryProgress(progress: OperationProgress) {
    controls = {
      ...controls,
      library: rememberOperationId(controls.library, progress.operationId),
    };
    const { event } = progress;
    if (operationProgressScope(progress) !== "library" || !isLifecycleEvent(event)) {
      return;
    }
    host.appendLibraryEvent(event);
    host.render();
  }

  function appendModelProgress(progress: OperationProgress) {
    controls = {
      ...controls,
      model: rememberOperationId(controls.model, progress.operationId),
    };
    const { event } = progress;
    if (operationProgressScope(progress) !== "model" || !isModelEvent(event)) {
      return;
    }
    host.appendModelEvent(event);
    host.render();
  }

  function cloneOperation(operation: OperationControls[OperationScope]) {
    return operation ? { ...operation } : null;
  }

  return {
    cancelActiveOperation,
    clear,
    reportError,
    runInstall,
    runLibrary,
    runModel,
    runRegistry,
    start,
    state,
  };
}
