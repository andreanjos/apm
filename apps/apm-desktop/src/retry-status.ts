import type { EngineEvent, OperationStatus } from "./types";

export function retryProgressMessage(event: EngineEvent) {
  switch (event.event) {
    case "install_started":
    case "install_format_started":
    case "install_download_started":
    case "install_download_progress":
    case "install_download_finished":
    case "install_archive_install_started":
    case "install_archive_verified":
    case "install_quarantine_removal_started":
    case "install_format_placed":
    case "install_state_recording_started":
    case "install_state_recorded":
    case "install_rolled_back":
    case "install_finished":
    case "install_failed":
      return `Retrying ${event.slug}`;
    case "remove_started":
    case "remove_format_removed":
    case "remove_format_missing":
    case "remove_state_recorded":
    case "remove_finished":
    case "remove_failed":
      return `Retrying remove for ${event.slug}`;
    case "registry_sync_started":
    case "registry_source_sync_started":
    case "registry_source_sync_finished":
    case "registry_source_sync_failed":
    case "registry_sync_finished":
      return "Retrying registry operation";
    case "scan_started":
    case "scan_finished":
      return "Retrying library scan";
    case "model_weight_pull_started":
    case "model_weight_pull_progress":
    case "model_weight_pull_finished":
    case "model_weight_pull_failed":
    case "model_install_started":
    case "model_install_finished":
    case "model_install_failed":
    case "model_run_started":
    case "model_run_completed":
    case "model_run_blocked":
    case "model_run_failed":
      return `Retrying model operation for ${event.package_id}`;
  }
}

export function retryStatusMessage(status: OperationStatus) {
  const action = operationKindStatusLabel(status.kind);
  switch (status.state) {
    case "succeeded":
      return `Retry completed: ${action}`;
    case "failed":
      return status.error ?? `Retry failed: ${action}`;
    case "canceled":
      return status.error ?? `Retry canceled: ${action}`;
    case "queued":
    case "running":
    case "cancel_requested":
      return `Retry still running: ${action}`;
  }
}

export function retryRecoveryStatusMessage(statuses: OperationStatus[]) {
  if (statuses.length === 0) {
    return "No retryable recovery operations.";
  }

  const failed = statuses.filter((status) => status.state === "failed");
  if (failed.length > 0) {
    return failed.length === 1
      ? retryStatusMessage(failed[0])
      : `${failed.length} recovery retries failed.`;
  }

  const incomplete = statuses.filter(
    (status) =>
      status.state === "queued" ||
      status.state === "running" ||
      status.state === "cancel_requested",
  );
  if (incomplete.length > 0) {
    return `${recoveryRetryCount(incomplete.length)} still running.`;
  }

  return statuses.length === 1
    ? retryStatusMessage(statuses[0])
    : `${recoveryRetryCount(statuses.length)} completed.`;
}

function operationKindStatusLabel(kind: OperationStatus["kind"]) {
  switch (kind) {
    case "registry_sync":
      return "registry sync";
    case "library_scan":
      return "library scan";
    case "install_url":
      return "direct install";
    case "install_archive":
      return "archive install";
    case "package_update":
      return "package update";
    case "package_remove":
      return "package remove";
    case "model_weight_pull":
      return "model weight pull";
    case "model_install":
      return "model install";
    case "model_run":
      return "model run";
  }
}

function recoveryRetryCount(count: number) {
  return count === 1 ? "1 recovery retry" : `${count} recovery retries`;
}
