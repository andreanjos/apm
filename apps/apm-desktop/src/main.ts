import { fallbackSnapshot } from "./fallback";
import "./styles.css";
import "./catalog.css";
import "./package-inspector.css";
import "./diagnostics-summary.css";
import {
  desktopSnapshotCommand,
  packageDetailsCommand,
  syncRegistriesCommand,
} from "./commands";
import type {
  InstallEvent,
  LifecycleEvent,
  ModelOperationEvent,
} from "./types";
import {
  defaultCatalogFilters,
  normalizeCatalogFilters,
  selectedPackageFor,
  visibleCatalogFor,
} from "./catalog-view-model";
import { renderApp } from "./view";
import { bindViewEvents } from "./view-bindings";
import type {
  CatalogAccessFilter,
  CatalogAvailabilityFilter,
  CatalogFilters,
  DesktopViewState,
  LifecycleNotice,
  WorkspaceSection,
} from "./view-model";
import {
  ensureLocalServiceSession,
  isTauriRuntime,
  startingServiceSession,
  unavailableServiceSession,
} from "./service-session";
import { createInstallController } from "./install-controller";
import { createHandoffController } from "./handoff-controller";
import { createRetryController } from "./retry-controller";
import { createOperationController } from "./operation-controller";
import { createModelController } from "./model-controller";
import { modelActionActive } from "./model-action-lock";
import { createLibraryController } from "./library-controller";
import { createDiagnosticsController } from "./diagnostics-controller";
import { createPackageDetailsController } from "./package-details-controller";
import {
  operationKindScope,
  type OperationScopeLocks,
  type OperationProgressScope,
} from "./operation-events";
import "./service.css";
import "./operation-history.css";
import "./model-packages.css";
import "./setup-checklist.css";

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root");
}

const appRoot = app;

let snapshot = fallbackSnapshot;
let selectedSlug =
  fallbackSnapshot.catalog.status === "matches"
    ? fallbackSnapshot.catalog.packages[0]?.slug ?? null
    : null;
let catalogSearchQuery = "";
let catalogFilters = defaultCatalogFilters();
let syncStatus = "Ready";
let serviceSession = fallbackSnapshot.service;
let workspaceSection: WorkspaceSection = "catalog";
let modelEvents: ModelOperationEvent[] = [];
let setInstallLifecycleNotice = (_notice: LifecycleNotice | null) => {};
let appendInstallLifecycleEvent = (_event: InstallEvent) => {};
let clearPendingHandoff = () => {};
let setLibraryNotice = (_notice: LifecycleNotice) => {};
let appendLibraryEvent = (_event: LifecycleEvent) => {};
let clearPendingLibraryUpdate = () => {};
let setModelOperationNotice = (_notice: LifecycleNotice) => {};

const operationController = createOperationController({
  setSyncStatus: (message) => {
    syncStatus = message;
  },
  setLifecycleNotice: (notice) => {
    setInstallLifecycleNotice(notice);
  },
  setLibraryNotice: (notice) => {
    setLibraryNotice(notice);
  },
  setModelNotice: (notice) => {
    setModelOperationNotice(notice);
  },
  appendLifecycleEvent: (event) => {
    appendInstallLifecycleEvent(event);
  },
  appendLibraryEvent: (event) => {
    appendLibraryEvent(event);
  },
  appendModelEvent: (event) => {
    modelEvents = [...modelEvents, event];
  },
  refreshSnapshotAfterOperationError,
  formatError,
  render,
});

const modelController = createModelController({
  refreshSnapshotData,
  modelOperationActive,
  runModelOperation: async (run) => {
    operationController.start("model");
    try {
      return await operationController.runModel(run);
    } finally {
      operationController.clear("model");
    }
  },
  clearModelEvents: () => {
    modelEvents = [];
  },
  formatError,
  render,
});
setModelOperationNotice = modelController.setModelNotice;

const installController = createInstallController({
  snapshot: () => snapshot,
  setSnapshot: (nextSnapshot) => {
    snapshot = nextSnapshot;
  },
  isTauriRuntime,
  lifecycleOperationActive,
  reloadSnapshot: loadSnapshot,
  runInstallOperation: (run) => operationController.runInstall(run),
  startInstallOperation: () => {
    operationController.start("lifecycle");
  },
  clearInstallOperation: () => {
    operationController.clear("lifecycle");
  },
  reportInstallError: (error) => operationController.reportError("lifecycle", error),
  clearPeerInstallDialogs: () => {
    clearPendingHandoff();
    clearPendingLibraryUpdate();
  },
  formatError,
  render,
});
setInstallLifecycleNotice = installController.setLifecycleNotice;
appendInstallLifecycleEvent = installController.appendLifecycleEvent;

const handoffController = createHandoffController({
  installPlan: installController.currentInstallPlan,
  setInstallPlan: installController.setInstallPlan,
  setLifecycleNotice: installController.setLifecycleNotice,
  lifecycleOperationActive,
  clearPeerInstallDialogs: () => {
    installController.clearPending();
    clearPendingLibraryUpdate();
  },
  formatError,
  render,
});
clearPendingHandoff = handoffController.clearPending;

function lifecycleOperationActive() {
  return operationController.state().lifecycle !== null;
}

function modelOperationActive() {
  return operationController.state().model !== null;
}

const libraryController = createLibraryController({
  snapshot: () => snapshot,
  setSnapshot: (nextSnapshot) => {
    snapshot = nextSnapshot;
  },
  isTauriRuntime,
  libraryOperationActive: () => operationController.state().library !== null,
  reloadSnapshot: loadSnapshot,
  runLibraryOperation: (run) => operationController.runLibrary(run),
  startLibraryOperation: () => {
    operationController.start("library");
  },
  clearLibraryOperation: () => {
    operationController.clear("library");
  },
  reportLibraryError: (error) => operationController.reportError("library", error),
  clearPeerDialogs: () => {
    handoffController.clearPending();
    installController.clearPending();
  },
  clearInstallStateForRemovedPackage: (slug) => {
    installController.clearForRemovedPackage(slug, selectedSlug);
  },
  formatError,
  render,
});
setLibraryNotice = libraryController.setLibraryNotice;
appendLibraryEvent = libraryController.appendLibraryEvent;
clearPendingLibraryUpdate = libraryController.clearPendingUpdate;

const retryController = createRetryController({
  retryableRecoveryCount: () => snapshot.recovery.retryable_count,
  activeOperationLocks,
  retryOperationScope,
  setSyncStatus: (message) => {
    syncStatus = message;
  },
  setLifecycleNotice: (notice) => {
    installController.setLifecycleNotice(notice);
  },
  setLibraryNotice: libraryController.setLibraryNotice,
  setModelNotice: modelController.setModelNotice,
  appendLifecycleEvent: (event) => {
    installController.appendLifecycleEvent(event);
  },
  appendLibraryEvent: libraryController.appendLibraryEvent,
  appendModelEvent: (event) => {
    modelEvents = [...modelEvents, event];
  },
  refreshSnapshotData,
  formatError,
  render,
});

function activeOperationLocks(): OperationScopeLocks {
  const operationState = operationController.state();
  const modelState = modelController.state();
  return {
    sync: operationState.sync !== null,
    lifecycle: operationState.lifecycle !== null,
    library: operationState.library !== null,
    model: modelActionActive({
      modelOperationActive: operationState.model !== null,
      modelStoreInitializing: modelState.modelStoreInitializing,
      modelImporting: modelState.modelImporting,
      importingCatalogModelId: modelState.importingCatalogModelId,
      installingModelId: modelState.installingModelId,
      planningModelId: modelState.planningModelId,
      pullingModelId: modelState.pullingModelId,
      removingModelId: modelState.removingModelId,
      runningModelId: modelState.runningModelId,
      planningModelChain: modelState.planningModelChain,
    }),
  };
}

function retryOperationScope(operationId: string): OperationProgressScope | null {
  const operation = snapshot.operations.find(
    (candidate) => candidate.operation_id === operationId,
  );
  return operation ? operationKindScope(operation.kind) : null;
}

const diagnosticsController = createDiagnosticsController({
  refreshSnapshotData,
  formatError,
  render,
});

const packageDetailsController = createPackageDetailsController({
  loadPackageDetails: packageDetailsCommand,
  formatError,
  render,
});

render();
void loadSnapshot();

async function ensureLocalService() {
  serviceSession = startingServiceSession(serviceSession);
  render();

  try {
    serviceSession = await ensureLocalServiceSession();
  } catch (error) {
    serviceSession = unavailableServiceSession(serviceSession, formatError(error));
  }
  render();
}

async function loadSnapshot() {
  serviceSession = startingServiceSession(serviceSession);
  render();

  try {
    await refreshSnapshotData();
    syncStatus = isTauriRuntime() ? "Live engine data" : "Preview data";
    await packageDetailsController.load(selectedSlug);
  } catch (error) {
    serviceSession = unavailableServiceSession(serviceSession, formatError(error));
    syncStatus = formatError(error);
  }
  render();
}

async function refreshSnapshotData() {
  snapshot = await desktopSnapshotCommand();
  serviceSession = snapshot.service;
  ensureSelectedPackage();
}

async function syncRegistries() {
  syncStatus = "Syncing registries";
  operationController.start("sync");
  render();
  try {
    const result = await operationController.runRegistry(syncRegistriesCommand);
    const failed = result.sources.filter((source) => source.status === "error");
    syncStatus =
      failed.length === 0
        ? `Synced ${result.sources.length} source${result.sources.length === 1 ? "" : "s"}`
        : `${failed.length} source${failed.length === 1 ? "" : "s"} failed`;
    await loadSnapshot();
  } catch (error) {
    await operationController.reportError("sync", error);
  } finally {
    operationController.clear("sync");
    render();
  }
}

function selectPackage(slug: string) {
  if (selectedSlug === slug) {
    return;
  }
  selectedSlug = slug;
  installController.clearForPackageSelection();
  clearPendingLibraryUpdate();
  packageDetailsController.clear();
  render();
  void packageDetailsController.load(selectedSlug);
}

function setCatalogSearchQuery(query: string) {
  if (catalogSearchQuery === query) {
    return;
  }
  const previousSlug = selectedSlug;
  catalogSearchQuery = query;
  ensureSelectedPackage();
  render();
  if (selectedSlug !== previousSlug) {
    void packageDetailsController.load(selectedSlug);
  }
}

function setWorkspaceSection(section: WorkspaceSection) {
  if (workspaceSection === section) {
    return;
  }
  workspaceSection = section;
  render();
}

function setCatalogAvailabilityFilter(availability: CatalogAvailabilityFilter) {
  updateCatalogFilters({ ...catalogFilters, availability });
}

function setCatalogProductTypeFilter(productType: string | null) {
  updateCatalogFilters({ ...catalogFilters, productType });
}

function setCatalogAccessFilter(access: CatalogAccessFilter) {
  updateCatalogFilters({ ...catalogFilters, access });
}

function updateCatalogFilters(nextFilters: CatalogFilters) {
  if (
    catalogFilters.availability === nextFilters.availability &&
    catalogFilters.productType === nextFilters.productType &&
    catalogFilters.access === nextFilters.access
  ) {
    return;
  }
  const previousSlug = selectedSlug;
  catalogFilters = nextFilters;
  ensureSelectedPackage();
  render();
  if (selectedSlug !== previousSlug) {
    void packageDetailsController.load(selectedSlug);
  }
}

function ensureSelectedPackage() {
  catalogFilters = normalizeCatalogFilters(snapshot, catalogFilters);
  const catalog = visibleCatalogFor(viewState());
  if (catalog.length === 0) {
    selectedSlug = null;
    packageDetailsController.clear();
    installController.clearForPackageSelection();
    handoffController.clearPending();
    libraryController.clearPending();
    libraryController.clearEvents();
    return;
  }
  if (!catalog.some((item) => item.slug === selectedSlug)) {
    selectedSlug = catalog[0]?.slug ?? null;
    packageDetailsController.clear();
    installController.clearForPackageSelection();
    handoffController.clearPending();
    libraryController.clearPending();
    libraryController.clearEvents();
  }
}

function render() {
  const state = viewState();
  const selectedPackage = selectedPackageFor(state);
  appRoot.innerHTML = renderApp(state);

  bindViewEvents(selectedPackage?.slug ?? null, {
    syncRegistries,
    ensureLocalService,
    initializeModelStore: modelController.initializeModelStore,
    reviewInstall: installController.reviewInstall,
    openInstallHandoff: handoffController.openInstallHandoff,
    confirmInstallHandoff: handoffController.confirmInstallHandoff,
    cancelInstallHandoff: handoffController.cancelInstallHandoff,
    setInstallScope: installController.setInstallScope,
    chooseArchiveAndInstall: installController.chooseArchiveAndInstall,
    requestUrlInstall: installController.requestUrlInstall,
    confirmArchiveInstall: installController.confirmArchiveInstall,
    cancelArchiveInstall: installController.cancelArchiveInstall,
    confirmUrlInstall: installController.confirmUrlInstall,
    cancelUrlInstall: installController.cancelUrlInstall,
    requestRemovePackage: libraryController.requestRemovePackage,
    requestUpdateAllPackages: libraryController.requestUpdateAllPackages,
    requestUpdatePackage: libraryController.requestUpdatePackage,
    confirmUpdateAllPackages: libraryController.confirmUpdateAllPackages,
    confirmUpdatePackage: libraryController.confirmUpdatePackage,
    cancelUpdateAllPackages: libraryController.cancelUpdateAllPackages,
    cancelUpdatePackage: libraryController.cancelUpdatePackage,
    setPackagePin: libraryController.setPackagePin,
    confirmRemovePackage: libraryController.confirmRemovePackage,
    cancelRemovePackage: libraryController.cancelRemovePackage,
    cancelActiveOperation: operationController.cancelActiveOperation,
    retryOperation: retryController.retryOperation,
    retryRecoveryOperations: retryController.retryRecoveryOperations,
    refreshDiagnostics: diagnosticsController.refreshDiagnostics,
    scanLibrary: libraryController.scanLibrary,
    addModelChainStep: modelController.addModelChainStep,
    clearModelChain: modelController.clearModelChain,
    importModelCatalogPackage: modelController.importModelCatalogPackage,
    importModelManifest: modelController.importModelManifest,
    installModelPackage: modelController.installModelPackage,
    planModelChain: modelController.planModelChain,
    planModelRun: modelController.planModelRun,
    pullModelWeights: modelController.pullModelWeights,
    removeModelChainStep: modelController.removeModelChainStep,
    removeModelPackage: modelController.removeModelPackage,
    runModel: modelController.runModel,
    setModelSearchQuery: modelController.setModelSearchQuery,
    selectPackage,
    setWorkspaceSection,
    setCatalogSearchQuery,
    setCatalogAvailabilityFilter,
    setCatalogProductTypeFilter,
    setCatalogAccessFilter,
  });
}

async function refreshSnapshotAfterOperationError(originalMessage: string) {
  if (!isTauriRuntime()) {
    return;
  }
  try {
    await refreshSnapshotData();
  } catch (refreshError) {
    serviceSession = unavailableServiceSession(
      serviceSession,
      `${originalMessage}; history refresh failed: ${formatError(refreshError)}`,
    );
  }
}

function viewState(): DesktopViewState {
  const retryState = retryController.state();
  const installState = installController.state();
  const handoffState = handoffController.state();
  const libraryState = libraryController.state();
  const diagnosticsState = diagnosticsController.state();
  const operationState = operationController.state();
  const modelState = modelController.state();
  return {
    serviceSession,
    snapshot,
    workspaceSection,
    selectedSlug,
    catalogSearchQuery,
    catalogFilters,
    ...packageDetailsController.state(selectedSlug),
    installPlan: installState.installPlan,
    installScope: installState.installScope,
    installStatus: installState.installStatus,
    lifecycleNotice: installState.lifecycleNotice,
    syncStatus,
    pendingArchiveInstall: installState.pendingArchiveInstall,
    pendingUrlInstall: installState.pendingUrlInstall,
    pendingInstallHandoff: handoffState.pendingInstallHandoff,
    pendingUpdateAllPackages: libraryState.pendingUpdateAllPackages,
    pendingUpdatePackage: libraryState.pendingUpdatePackage,
    pendingRemovePackage: libraryState.pendingRemovePackage,
    updateAllCount: libraryState.updateAllCount,
    syncOperation: operationState.sync,
    lifecycleOperation: operationState.lifecycle,
    libraryOperation: operationState.library,
    lifecycleEvents: installState.lifecycleEvents,
    libraryNotice: libraryState.libraryNotice,
    libraryEvents: libraryState.libraryEvents,
    diagnosticsNotice: diagnosticsState.diagnosticsNotice,
    diagnosticsRefreshing: diagnosticsState.diagnosticsRefreshing,
    modelEvents,
    modelOperation: operationState.model,
    modelNotice: modelState.modelNotice,
    modelStoreInitializing: modelState.modelStoreInitializing,
    modelRunPlan: modelState.modelRunPlan,
    modelChainPlan: modelState.modelChainPlan,
    modelChainSteps: modelState.modelChainSteps,
    planningModelChain: modelState.planningModelChain,
    modelImporting: modelState.modelImporting,
    importingCatalogModelId: modelState.importingCatalogModelId,
    installingModelId: modelState.installingModelId,
    planningModelId: modelState.planningModelId,
    pullingModelId: modelState.pullingModelId,
    removingModelId: modelState.removingModelId,
    runningModelId: modelState.runningModelId,
    modelSearchQuery: modelState.modelSearchQuery,
    retryingOperationId: retryState.retryingOperationId,
    retryingRecovery: retryState.retryingRecovery,
  };
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
