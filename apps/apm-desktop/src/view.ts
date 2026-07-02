import type { DesktopServiceSession } from "./types";
import type {
  DesktopViewState,
  OperationControlState,
  WorkspaceSection,
} from "./view-model";
import {
  catalogSearchMarkup,
  catalogWorkspaceMarkup,
  catalogWorkspaceRenderData,
} from "./catalog-workspace";
import {
  archiveConfirmDialog,
  handoffConfirmDialog,
  removeConfirmDialog,
  updateAllConfirmDialog,
  updateConfirmDialog,
  urlConfirmDialog,
} from "./view-dialogs";
import {
  lifecycleNoticeMarkup,
  lifecycleTimelineMarkup,
  operationCancelMarkup,
} from "./lifecycle-markup";
import { diagnosticsSummarySection } from "./diagnostics-summary";
import { libraryWorkspaceMarkup } from "./library-workspace";
import { modelActionActiveForView } from "./model-action-lock";
import { modelPackagesSection } from "./model-packages";
import { operationHistorySection } from "./operation-history";
import { setupChecklistSection } from "./setup-checklist";
import type { OperationScopeLocks } from "./operation-events";
import { escapeHtml } from "./view-utils";

export function renderApp(state: DesktopViewState) {
  return `
    <div class="shell">
      <aside class="sidebar" aria-label="Primary">
        <div class="brand">
          <div class="brand-mark" aria-hidden="true"></div>
          <div>
            <div class="brand-name">apm</div>
            <div class="brand-subtitle">Audio Package Manager</div>
          </div>
        </div>
        <nav class="nav-list">
          ${navItem("Package", "Catalog", "catalog", state.workspaceSection)}
          ${navItem("HardDrive", "Library", "library", state.workspaceSection)}
          ${navItem("Activity", "Diagnostics", "diagnostics", state.workspaceSection)}
          ${navItem("Terminal", "Runtime", "runtime", state.workspaceSection)}
        </nav>
        <div class="sync-panel">
          <div class="sync-label">Registry</div>
          <div class="sync-value">${escapeHtml(state.syncStatus)}</div>
          ${syncOperationButton(state.syncOperation)}
        </div>
        ${servicePanel(state.serviceSession)}
      </aside>

      <main class="workspace">
        ${topbarMarkup(state)}
        ${setupChecklistSection(
          state.snapshot,
          state.serviceSession,
          state.syncOperation,
          {
            modelStoreInitializing: state.modelStoreInitializing,
            modelActionLocked: modelActionActiveForView(state),
          },
        )}
        ${workspaceSectionMarkup(state)}
      </main>
    </div>
    ${archiveConfirmDialog(state.pendingArchiveInstall)}
    ${urlConfirmDialog(state.pendingUrlInstall)}
    ${handoffConfirmDialog(state.pendingInstallHandoff)}
    ${updateAllConfirmDialog(state.pendingUpdateAllPackages)}
    ${updateConfirmDialog(state.pendingUpdatePackage)}
    ${removeConfirmDialog(state.pendingRemovePackage)}
  `;
}

function topbarMarkup(state: DesktopViewState) {
  return `
    <header class="topbar">
      <div>
        <p class="eyebrow">${escapeHtml(workspaceEyebrow(state.workspaceSection))}</p>
        <h1>${escapeHtml(workspaceTitle(state.workspaceSection))}</h1>
      </div>
      ${state.workspaceSection === "catalog" ? catalogSearchMarkup(state.catalogSearchQuery) : ""}
    </header>
  `;
}

function workspaceSectionMarkup(state: DesktopViewState) {
  const installedPackageCount = state.snapshot.installed.length;

  switch (state.workspaceSection) {
    case "catalog":
      return catalogWorkspaceMarkup(
        state,
        catalogWorkspaceRenderData(state, installedPackageCount),
      );
    case "library":
      return libraryWorkspaceMarkup(state, {
        installedCount: installedPackageCount,
        updateCount:
          state.snapshot.updates.status === "ready"
            ? state.snapshot.updates.updates.length
            : 0,
      });
    case "diagnostics":
      return diagnosticsWorkspaceMarkup(state);
    case "runtime":
      return runtimeWorkspaceMarkup(state);
  }
}

function runtimeWorkspaceMarkup(state: DesktopViewState) {
  return modelPackagesSection(
    state.snapshot.models,
    state.snapshot.model_catalog,
    {
      notice: state.modelNotice,
      runPlan: state.modelRunPlan,
      chainPlan: state.modelChainPlan,
      chainSteps: state.modelChainSteps,
      modelStore: state.snapshot.model_store,
      modelStoreInitializing: state.modelStoreInitializing,
      modelEvents: state.modelEvents,
      modelOperation: state.modelOperation,
      importing: state.modelImporting,
      importingCatalogModelId: state.importingCatalogModelId,
      installingModelId: state.installingModelId,
      planningModelId: state.planningModelId,
      planningModelChain: state.planningModelChain,
      pullingModelId: state.pullingModelId,
      removingModelId: state.removingModelId,
      runningModelId: state.runningModelId,
      modelSearchQuery: state.modelSearchQuery,
    },
  );
}

function diagnosticsWorkspaceMarkup(state: DesktopViewState) {
  return `
    ${diagnosticsSummarySection(state.snapshot, state.serviceSession)}
    ${diagnosticsScanMarkup(state)}
    ${operationHistorySection(
      state.snapshot.operations,
      state.snapshot.recovery,
      state.retryingOperationId,
      state.retryingRecovery,
      operationRetryLocks(state),
    )}
  `;
}

function operationRetryLocks(state: DesktopViewState): OperationScopeLocks {
  return {
    sync: state.syncOperation !== null,
    lifecycle: state.lifecycleOperation !== null,
    library: state.libraryOperation !== null,
    model: modelActionActiveForView(state),
  };
}

function diagnosticsScanMarkup(state: DesktopViewState) {
  const disabled = state.libraryOperation ? "disabled" : "";
  const doctorDisabled = state.diagnosticsRefreshing ? "disabled" : "";
  return `
    <section class="panel diagnostics-action-panel">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Local checks</p>
          <h2>Diagnostics actions</h2>
        </div>
        <div class="panel-actions">
          <button class="secondary-action" data-refresh-diagnostics-action type="button" ${doctorDisabled}>
            <i data-lucide="activity" aria-hidden="true"></i>
            ${state.diagnosticsRefreshing ? "Running" : "Run doctor"}
          </button>
          <button class="secondary-action" data-scan-library-action type="button" ${disabled}>
            <i data-lucide="refresh-cw" aria-hidden="true"></i>
            Scan library
          </button>
        </div>
      </div>
      ${lifecycleNoticeMarkup(state.diagnosticsNotice)}
      ${lifecycleNoticeMarkup(state.libraryNotice)}
      ${operationCancelMarkup("library", state.libraryOperation)}
      ${lifecycleTimelineMarkup(state.libraryEvents)}
    </section>
  `;
}

function workspaceTitle(section: WorkspaceSection) {
  switch (section) {
    case "catalog":
      return "Package catalog";
    case "library":
      return "Installed library";
    case "diagnostics":
      return "System diagnostics";
    case "runtime":
      return "Audio-AI runtime";
  }
}

function workspaceEyebrow(section: WorkspaceSection) {
  switch (section) {
    case "catalog":
      return "Browse";
    case "library":
      return "Local machine";
    case "diagnostics":
      return "Health";
    case "runtime":
      return "Models";
  }
}

function servicePanel(session: DesktopServiceSession) {
  const canStart =
    session.status === "not_started" ||
    session.status === "unavailable" ||
    session.status === "preview";
  const pid = session.pid ? `pid ${session.pid}` : "no process";
  const contract = `${session.api_version} / ${session.schema_version}`;

  return `
    <div class="service-panel ${session.status}">
      <div class="service-header">
        <div>
          <div class="sync-label">Local service</div>
          <div class="service-state">${escapeHtml(serviceStatusLabel(session))}</div>
        </div>
        <button class="icon-button service-button" id="service-button" type="button" aria-label="${escapeHtml(serviceActionLabel(session))}" title="${escapeHtml(serviceActionLabel(session))}" ${canStart ? "" : "disabled"}>
          <i data-lucide="terminal" aria-hidden="true"></i>
        </button>
      </div>
      <div class="service-message">${escapeHtml(session.message)}</div>
      <div class="service-meta">${escapeHtml(session.url)} / ${escapeHtml(pid)} / ${escapeHtml(contract)}</div>
    </div>
  `;
}

function serviceStatusLabel(session: DesktopServiceSession) {
  switch (session.status) {
    case "started":
      return session.token_available ? "Started" : "Token missing";
    case "reused":
      return session.token_available ? "Reused" : "Token missing";
    case "not_started":
      return "Stopped";
    case "unavailable":
      return "Unavailable";
    case "preview":
      return "Preview";
  }
}

function serviceActionLabel(session: DesktopServiceSession) {
  switch (session.status) {
    case "started":
    case "reused":
      return "Local service is ready";
    case "not_started":
      return "Start local service";
    case "unavailable":
      return "Retry local service";
    case "preview":
      return "Start service in the desktop app";
  }
}

function navItem(
  icon: string,
  label: string,
  section: WorkspaceSection,
  currentSection: WorkspaceSection,
) {
  const active = section === currentSection;
  return `
    <button class="nav-item${active ? " active" : ""}" type="button" data-workspace-section="${section}" ${active ? 'aria-current="page"' : ""}>
      <i data-lucide="${icon}" aria-hidden="true"></i>
      <span>${label}</span>
    </button>
  `;
}

function syncOperationButton(operation: OperationControlState | null) {
  if (!operation) {
    return `
      <button class="icon-button sync-button" id="sync-button" type="button" aria-label="Sync registries" title="Sync registries">
        <i data-lucide="refresh-cw" aria-hidden="true"></i>
      </button>
    `;
  }

  const canCancel = !!operation.operationId && !operation.canceling;
  const label = operation.canceling
    ? "Cancellation requested"
    : operation.operationId
      ? "Cancel registry sync"
      : "Starting registry sync";
  return `
    <button class="icon-button sync-button" data-cancel-operation-scope="sync" type="button" aria-label="${label}" title="${label}" ${canCancel ? "" : "disabled"}>
      <i data-lucide="x" aria-hidden="true"></i>
    </button>
  `;
}
