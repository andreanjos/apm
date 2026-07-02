import type {
  DesktopServiceSession,
  DesktopSnapshot,
  InstallEvent,
  InstallPlanResult,
  InstallScope,
  InstalledPackageSummary,
  LifecycleEvent,
  ModelChainPlan,
  ModelRunPlan,
  ModelOperationEvent,
  PackageDetailsResult,
  PackageUpdateSummary,
} from "./types";

export type { InstallScope } from "./types";

export type LifecycleNotice = {
  tone: "info" | "success" | "error";
  message: string;
};

export type OperationControlState = {
  operationId: string | null;
  canceling: boolean;
};

export type OperationScope = "sync" | "lifecycle" | "library" | "model";

export type WorkspaceSection = "catalog" | "library" | "diagnostics" | "runtime";

export type ModelChainDraftStep = {
  name: string;
  version: string;
  packageId: string;
};

export type CatalogAvailabilityFilter = "all" | "installed" | "available";

export type CatalogAccessFilter = "all" | "free" | "paid";

export type CatalogFilters = {
  availability: CatalogAvailabilityFilter;
  productType: string | null;
  access: CatalogAccessFilter;
};

export type ArchiveInstallCandidate = {
  slug: string;
  name: string;
  format: string;
  installType: string;
  version: string;
  destination: string;
  installScope: InstallScope;
  archivePath: string;
  archiveName: string;
  checksum: string;
};

export type UrlInstallCandidate = {
  slug: string;
  name: string;
  format: string;
  version: string;
  destination: string;
  installScope: InstallScope;
  source: string;
  checksum: string;
};

export type InstallHandoffCandidate = {
  slug: string;
  name: string;
  vendor: string;
  version: string;
  statusLabel: string;
  target: string;
  message: string;
  actionLabel: string;
  privileged: boolean;
};

export type UpdatePackageCandidate = PackageUpdateSummary & {
  formats: string[];
  updateFormat: string | null;
};

export type UpdateAllPackagesCandidate = {
  updates: UpdatePackageCandidate[];
};

export type DesktopViewState = {
  serviceSession: DesktopServiceSession;
  snapshot: DesktopSnapshot;
  workspaceSection: WorkspaceSection;
  selectedSlug: string | null;
  catalogSearchQuery: string;
  catalogFilters: CatalogFilters;
  packageDetails: PackageDetailsResult | null;
  packageDetailsLoading: boolean;
  packageDetailsError: string | null;
  installPlan: InstallPlanResult | null;
  installScope: InstallScope;
  installStatus: string;
  lifecycleNotice: LifecycleNotice | null;
  syncStatus: string;
  pendingArchiveInstall: ArchiveInstallCandidate | null;
  pendingUrlInstall: UrlInstallCandidate | null;
  pendingInstallHandoff: InstallHandoffCandidate | null;
  pendingUpdateAllPackages: UpdateAllPackagesCandidate | null;
  pendingUpdatePackage: UpdatePackageCandidate | null;
  pendingRemovePackage: InstalledPackageSummary | null;
  updateAllCount: number;
  syncOperation: OperationControlState | null;
  lifecycleOperation: OperationControlState | null;
  libraryOperation: OperationControlState | null;
  lifecycleEvents: InstallEvent[];
  libraryNotice: LifecycleNotice | null;
  libraryEvents: LifecycleEvent[];
  diagnosticsNotice: LifecycleNotice | null;
  diagnosticsRefreshing: boolean;
  modelEvents: ModelOperationEvent[];
  modelOperation: OperationControlState | null;
  modelNotice: LifecycleNotice | null;
  modelStoreInitializing: boolean;
  modelRunPlan: ModelRunPlan | null;
  modelChainPlan: ModelChainPlan | null;
  modelChainSteps: ModelChainDraftStep[];
  planningModelChain: boolean;
  modelImporting: boolean;
  importingCatalogModelId: string | null;
  installingModelId: string | null;
  planningModelId: string | null;
  pullingModelId: string | null;
  removingModelId: string | null;
  runningModelId: string | null;
  modelSearchQuery: string;
  retryingOperationId: string | null;
  retryingRecovery: boolean;
};
