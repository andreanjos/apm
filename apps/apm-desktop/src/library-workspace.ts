import type {
  InstalledPackageSummary,
  LifecycleEvent,
  PackageUpdateSummary,
} from "./types";
import type {
  DesktopViewState,
  LifecycleNotice,
  OperationControlState,
} from "./view-model";
import {
  lifecycleNoticeMarkup,
  lifecycleTimelineMarkup,
  operationCancelMarkup,
} from "./lifecycle-markup";
import { escapeHtml } from "./view-utils";

const LIBRARY_ACTION_LOCKED_LABEL = "Library operation running";

type LibraryWorkspaceRenderData = {
  installedCount: number;
  updateCount: number;
};

export function libraryWorkspaceMarkup(
  state: DesktopViewState,
  data: LibraryWorkspaceRenderData,
) {
  const libraryActionLocked = state.libraryOperation !== null;
  return `
    <section class="panel library-panel">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Local machine</p>
          <h2>Installed library</h2>
        </div>
        <div class="panel-actions">
          <span class="status-pill">${data.installedCount} tracked / ${data.updateCount} updates</span>
          <button class="library-action-button update-all-button" type="button" data-update-all-packages aria-label="Review all ready updates" title="${escapeHtml(updateAllTitle(state))}" ${canUpdateAll(state) ? "" : "disabled"}>
            <i data-lucide="refresh-cw" aria-hidden="true"></i>
            <span>Update ready</span>
          </button>
        </div>
      </div>
      <div class="library-list">
        ${state.snapshot.installed.slice(0, 8).map((item) => installedRow(item, updateFor(state, item.slug), libraryActionLocked)).join("") || emptyLibrary()}
      </div>
      ${libraryOperationMarkup(
        state.libraryNotice,
        state.libraryEvents,
        state.libraryOperation,
      )}
    </section>
  `;
}

function canUpdateAll(state: DesktopViewState) {
  return state.updateAllCount > 0 && !state.libraryOperation;
}

function updateAllTitle(state: DesktopViewState) {
  if (state.libraryOperation) {
    return "Library operation already running";
  }
  return state.updateAllCount > 0
    ? `Review ${state.updateAllCount} ready update${state.updateAllCount === 1 ? "" : "s"}`
    : "No installable updates ready";
}

function installedRow(
  item: InstalledPackageSummary,
  update: PackageUpdateSummary | null,
  libraryActionLocked: boolean,
) {
  const formats = item.formats.map((format) => format.format).join(" + ");
  const canRemove = item.origin === "apm";
  const canReviewUpdate = update?.action === "installable";
  const pinAction = item.pinned ? "Unpin" : "Pin";
  const pinLabel = libraryActionLocked
    ? `${LIBRARY_ACTION_LOCKED_LABEL} for ${item.slug}`
    : `${pinAction} ${item.slug}`;
  const pinInputAttributes = [
    item.pinned ? "checked" : "",
    libraryActionLocked ? "disabled" : "",
  ].filter(Boolean).join(" ");
  const pinToggleTitle = libraryActionLocked ? LIBRARY_ACTION_LOCKED_LABEL : pinTitle(item);
  const updateActionLabel = libraryActionLocked
    ? `${LIBRARY_ACTION_LOCKED_LABEL} for ${item.slug}`
    : `Review update for ${item.slug}`;
  const updateButtonTitle = libraryActionLocked
    ? LIBRARY_ACTION_LOCKED_LABEL
    : updateTitle(update);
  const removeActionLabel = libraryActionLocked
    ? `${LIBRARY_ACTION_LOCKED_LABEL} for ${item.slug}`
    : `Remove ${item.slug}`;
  const removeButtonTitle = libraryActionLocked
    ? LIBRARY_ACTION_LOCKED_LABEL
    : canRemove
      ? `Remove ${item.slug}`
      : "External install; remove with the vendor installer or Finder";
  const health = libraryHealth(item, update);
  return `
    <div class="library-row">
      <div>
        <strong>${escapeHtml(item.slug)}</strong>
        <small>${escapeHtml(libraryVersionLabel(item, update))}</small>
      </div>
      <span>${escapeHtml(formats)}</span>
      <span>${escapeHtml(item.origin)}</span>
      ${libraryHealthBadge(item.slug, health)}
      <label class="pin-toggle" title="${escapeHtml(pinToggleTitle)}">
        <input type="checkbox" data-pin-slug="${escapeHtml(item.slug)}" aria-label="${escapeHtml(pinLabel)}"${pinInputAttributes ? ` ${pinInputAttributes}` : ""}>
        <span>${item.pinned ? "Pinned" : "Pin"}</span>
      </label>
      <button class="library-action-button update-button" type="button" data-update-slug="${escapeHtml(item.slug)}" aria-label="${escapeHtml(updateActionLabel)}" title="${escapeHtml(updateButtonTitle)}" ${canReviewUpdate && !libraryActionLocked ? "" : "disabled"}>
        ${escapeHtml(updateLabel(update))}
      </button>
      <button class="icon-button remove-button" type="button" data-remove-slug="${escapeHtml(item.slug)}" aria-label="${escapeHtml(removeActionLabel)}" title="${escapeHtml(removeButtonTitle)}" ${canRemove && !libraryActionLocked ? "" : "disabled"}>
        <i data-lucide="trash-2" aria-hidden="true"></i>
      </button>
    </div>
  `;
}

function libraryHealthBadge(slug: string, health: LibraryHealth) {
  return `
    <span class="library-health ${escapeHtml(health.tone)}" data-library-health="${escapeHtml(health.key)}" title="${escapeHtml(health.detail)}" aria-label="${escapeHtml(`${slug} health: ${health.label}`)}">
      ${escapeHtml(health.label)}
    </span>
  `;
}

type LibraryHealth = {
  key: "current" | "update-ready" | "pinned" | "external";
  label: string;
  detail: string;
  tone: "good" | "warn" | "info";
};

function libraryHealth(
  item: InstalledPackageSummary,
  update: PackageUpdateSummary | null,
): LibraryHealth {
  if (update) {
    switch (update.action) {
      case "installable":
        return {
          key: "update-ready",
          label: "Update ready",
          detail: `${update.installed_version} -> ${update.available_version}`,
          tone: "warn",
        };
      case "pinned":
        return {
          key: "pinned",
          label: "Pinned",
          detail: "Skipped during update runs",
          tone: "info",
        };
      case "external":
        return {
          key: "external",
          label: "External",
          detail: "Managed outside apm; update with the vendor installer",
          tone: "info",
        };
    }
  }

  if (item.origin !== "apm") {
    return {
      key: "external",
      label: "External",
      detail: "Tracked from scan; files remain vendor-managed",
      tone: "info",
    };
  }

  return {
    key: "current",
    label: "Current",
    detail: "Tracked by apm with no installable update",
    tone: "good",
  };
}

function pinTitle(item: InstalledPackageSummary) {
  return item.pinned
    ? "Allow this package to update again"
    : "Skip this package during update runs";
}

function updateFor(state: DesktopViewState, slug: string) {
  if (state.snapshot.updates.status !== "ready") {
    return null;
  }
  return state.snapshot.updates.updates.find((update) => update.slug === slug) ?? null;
}

function libraryVersionLabel(
  item: InstalledPackageSummary,
  update: PackageUpdateSummary | null,
) {
  if (!update) {
    return `${item.vendor} / ${item.version}`;
  }
  return `${item.vendor} / ${update.installed_version} -> ${update.available_version}`;
}

function updateLabel(update: PackageUpdateSummary | null) {
  if (!update) {
    return "Current";
  }
  switch (update.action) {
    case "installable":
      return "Update";
    case "pinned":
      return "Pinned";
    case "external":
      return "External";
  }
}

function updateTitle(update: PackageUpdateSummary | null) {
  if (!update) {
    return "No update available";
  }
  switch (update.action) {
    case "installable":
      return `Review update to ${update.available_version}`;
    case "pinned":
      return "Pinned packages are skipped until unpinned";
    case "external":
      return "External packages update through their vendor installer";
  }
}

function libraryOperationMarkup(
  libraryNotice: LifecycleNotice | null,
  libraryEvents: LifecycleEvent[],
  operation: OperationControlState | null,
) {
  if (!libraryNotice && libraryEvents.length === 0 && !operation?.operationId) {
    return "";
  }

  return `
    <div class="library-operation">
      ${lifecycleNoticeMarkup(libraryNotice)}
      ${operationCancelMarkup("library", operation)}
      ${lifecycleTimelineMarkup(libraryEvents)}
    </div>
  `;
}

function emptyLibrary() {
  return `<div class="empty-library">No packages tracked locally yet.</div>`;
}
