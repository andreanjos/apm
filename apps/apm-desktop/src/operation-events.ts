import { isTauriRuntime } from "./service-session";
import type {
  EngineEvent,
  InstallEvent,
  LifecycleEvent,
  ModelOperationEvent,
  OperationKind,
  RegistryEvent,
} from "./types";
import { formatBytes } from "./view-utils";

const OPERATION_PROGRESS_EVENT = "apm-operation-progress";

type OperationProgressPayload = {
  progress_id?: string | null;
  operation_id: string;
  kind: OperationKind;
  event: EngineEvent;
};

export type OperationProgress = {
  operationId: string;
  kind: OperationKind;
  event: EngineEvent;
};

export type OperationProgressScope = "sync" | "lifecycle" | "library" | "model";
export type OperationScopeLocks = Readonly<Record<OperationProgressScope, boolean>>;

export async function withOperationProgress<T>(
  run: (progressId?: string) => Promise<T>,
  onProgress: (progress: OperationProgress) => void,
): Promise<T> {
  if (!isTauriRuntime()) {
    return run();
  }

  const progressId = newProgressId();
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<OperationProgressPayload>(
    OPERATION_PROGRESS_EVENT,
    ({ payload }) => {
      if (payload.progress_id === progressId) {
        onProgress({
          operationId: payload.operation_id,
          kind: payload.kind,
          event: payload.event,
        });
      }
    },
  );

  try {
    return await run(progressId);
  } finally {
    unlisten();
  }
}

function newProgressId() {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `progress-${Date.now()}-${Math.random().toString(16).slice(2)}`
  );
}

export function isRegistryEvent(event: EngineEvent): event is RegistryEvent {
  return event.event.startsWith("registry_");
}

export function isInstallEvent(event: EngineEvent): event is InstallEvent {
  return event.event.startsWith("install_");
}

export function isLifecycleEvent(event: EngineEvent): event is LifecycleEvent {
  return (
    isInstallEvent(event) ||
    event.event.startsWith("remove_") ||
    event.event.startsWith("scan_")
  );
}

export function isModelEvent(event: EngineEvent): event is ModelOperationEvent {
  return event.event.startsWith("model_");
}

export function operationProgressScope(
  progress: OperationProgress,
): OperationProgressScope | null {
  const scope = operationKindScope(progress.kind);
  switch (scope) {
    case "sync":
      return isRegistryEvent(progress.event) ? scope : null;
    case "lifecycle":
      return isInstallEvent(progress.event) ? scope : null;
    case "library":
      return isLifecycleEvent(progress.event) ? scope : null;
    case "model":
      return isModelEvent(progress.event) ? scope : null;
  }
}

export function operationKindScope(kind: OperationKind): OperationProgressScope {
  switch (kind) {
    case "registry_sync":
      return "sync";
    case "install_url":
    case "install_archive":
      return "lifecycle";
    case "library_scan":
    case "package_update":
    case "package_remove":
      return "library";
    case "model_weight_pull":
    case "model_install":
    case "model_run":
      return "model";
  }
}

export function hasActiveOperationLock(locks: OperationScopeLocks) {
  return Object.values(locks).some(Boolean);
}

export function registryProgressLabel(event: RegistryEvent) {
  switch (event.event) {
    case "registry_sync_started":
      return `Syncing ${event.source_count} source${event.source_count === 1 ? "" : "s"}`;
    case "registry_source_sync_started":
      return `Syncing ${event.source}`;
    case "registry_source_sync_finished":
      return `${event.source}: ${event.installable_product_count} installable packages`;
    case "registry_source_sync_failed":
      return `${event.source} failed: ${event.error}`;
    case "registry_sync_finished":
      return event.failed_count === 0
        ? `Synced ${event.source_count} source${event.source_count === 1 ? "" : "s"}`
        : `${event.failed_count} source${event.failed_count === 1 ? "" : "s"} failed`;
  }
}

export function modelProgressLabel(event: ModelOperationEvent) {
  switch (event.event) {
    case "model_weight_pull_started":
      return `Checking weights for ${event.package_id}`;
    case "model_weight_pull_progress":
      return modelWeightProgressLabel(event.package_id, event.bytes, event.total_bytes);
    case "model_weight_pull_finished":
      return `${event.package_id} weights ${event.status} (${formatBytes(event.bytes)})`;
    case "model_weight_pull_failed":
      return `${event.package_id} weights failed: ${event.error}`;
    case "model_install_started":
      return `Installing ${event.package_id}`;
    case "model_install_finished":
      return `${event.package_id} ready (${event.adapter} ${event.runtime_status}, weights ${event.weights_status})`;
    case "model_install_failed":
      return `${event.package_id} install failed: ${event.error}`;
    case "model_run_started":
      return `Starting ${event.package_id}`;
    case "model_run_completed":
      return `${event.package_id} run completed: ${event.output_path}`;
    case "model_run_blocked":
      return `${event.package_id} run blocked: ${event.message}`;
    case "model_run_failed":
      return `${event.package_id} run failed: ${event.error}`;
  }
}

function modelWeightProgressLabel(
  packageId: string,
  bytes: number,
  totalBytes: number | null | undefined,
) {
  if (totalBytes && totalBytes > 0) {
    return `${packageId} weights ${formatBytes(bytes)} of ${formatBytes(totalBytes)}`;
  }
  return `${packageId} weights ${formatBytes(bytes)}`;
}
