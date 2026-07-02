import type { LifecycleEvent } from "./types";
import type {
  LifecycleNotice,
  OperationControlState,
  OperationScope,
} from "./view-model";
import { escapeHtml, formatBytes, formatLabel } from "./view-utils";

export function operationCancelMarkup(
  scope: OperationScope,
  operation: OperationControlState | null,
) {
  if (!operation?.operationId) {
    return "";
  }

  return `
    <button class="secondary-action" data-cancel-operation-scope="${scope}" type="button" ${operation.canceling ? "disabled" : ""}>
      ${operation.canceling ? "Cancel requested" : "Cancel operation"}
    </button>
  `;
}

export function lifecycleTimelineMarkup(events: LifecycleEvent[]) {
  if (events.length === 0) {
    return "";
  }

  return `
    <div class="event-timeline" aria-label="Lifecycle progress">
      ${events.map((event) => `<div class="event-step">${escapeHtml(eventLabel(event))}</div>`).join("")}
    </div>
  `;
}

export function lifecycleNoticeMarkup(lifecycleNotice: LifecycleNotice | null) {
  return lifecycleNotice
    ? `<p class="lifecycle-notice ${lifecycleNotice.tone}">${escapeHtml(lifecycleNotice.message)}</p>`
    : "";
}

function eventLabel(event: LifecycleEvent) {
  switch (event.event) {
    case "install_started":
      return `Started ${event.slug} ${event.version}`;
    case "install_format_started":
      return `Preparing ${formatLabel(event.format)}`;
    case "install_download_started":
      return `Downloading ${formatLabel(event.format)} archive`;
    case "install_download_progress":
      return downloadProgressLabel(event.bytes, event.total_bytes);
    case "install_download_finished":
      return `Downloaded ${formatBytes(event.bytes)} archive`;
    case "install_archive_install_started":
      return `Installing ${event.install_type.toUpperCase()} archive for ${formatLabel(event.format)}`;
    case "install_archive_verified":
      return `Verified ${formatLabel(event.format)} archive`;
    case "install_quarantine_removal_started":
      return `Removing quarantine from ${formatLabel(event.format)} bundle`;
    case "install_format_placed":
      return `Placed ${formatLabel(event.format)} bundle`;
    case "install_state_recording_started":
      return `Recording ${event.slug} in local state`;
    case "install_state_recorded":
      return `Recorded ${event.slug} in local state`;
    case "install_rolled_back":
      return `Rolled back ${formatLabel(event.format)} bundle`;
    case "install_finished":
      return `Finished ${event.installed_format_count} format${event.installed_format_count === 1 ? "" : "s"}`;
    case "install_failed":
      return `Failed: ${event.error}`;
    case "remove_started":
      return `Started removing ${event.slug} ${event.version}`;
    case "remove_format_removed":
      return `Removed ${formatLabel(event.format)} bundle`;
    case "remove_format_missing":
      return `${formatLabel(event.format)} bundle was already missing`;
    case "remove_state_recorded":
      return `Recorded ${event.slug} removal in local state`;
    case "remove_finished":
      return `Finished removing ${event.removed_format_count} format${event.removed_format_count === 1 ? "" : "s"}`;
    case "remove_failed":
      return `Failed: ${event.error}`;
    case "scan_started":
      return "Scanning local plugin folders";
    case "scan_finished":
      return `Found ${event.scanned_count} bundle${event.scanned_count === 1 ? "" : "s"} / matched ${event.matched_count} / tracked ${event.adopted_count}`;
  }
}

function downloadProgressLabel(bytes: number, totalBytes: number | null | undefined) {
  if (totalBytes && totalBytes > 0) {
    return `Downloaded ${formatBytes(bytes)} of ${formatBytes(totalBytes)}`;
  }
  return `Downloaded ${formatBytes(bytes)}`;
}
