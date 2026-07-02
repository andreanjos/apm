import type {
  ModelCatalogListResult,
  ModelInstallResult,
  ModelListResult,
  ModelOperationEvent,
  ModelRunPlanRequest,
  ModelRunResult,
  ModelStoreLayout,
  ModelWeightPullResult,
} from "./model-types";

export type {
  AvailableModelPackage,
  CachedModelPackage,
  DesktopModelInstallResult,
  DesktopModelRunResult,
  DesktopModelWeightPullResult,
  ModelCatalogListResult,
  ModelChainEdgePlan,
  ModelChainExecutionReadiness,
  ModelChainIoBinding,
  ModelChainPlan,
  ModelChainPlanRequest,
  ModelChainPlanStatus,
  ModelChainStepPlan,
  ModelChainStepRequest,
  ModelInstallResult,
  ModelListResult,
  ModelManifestCacheResult,
  ModelManifestSummary,
  ModelOperationEvent,
  ModelParameterSummary,
  ModelRemoveResult,
  ModelRemoveStatus,
  ModelRunExecutionReadiness,
  ModelRunParamBinding,
  ModelRunPlan,
  ModelRunPlanRequest,
  ModelRunPlanStatus,
  ModelRunResult,
  ModelRunStatus,
  ModelRuntimeProvisioning,
  ModelStoreInitResult,
  ModelStoreLayout,
  ModelWeightPullResult,
  ModelWeightPullStatus,
  ModelWeightsSummary,
  RuntimeProvisioningStatus,
} from "./model-types";

export type PackageSearchResult =
  | { status: "catalog_empty" }
  | { status: "matches"; total_matches: number; packages: PackageSummary[] };

export type PackageSummary = {
  slug: string;
  name: string;
  vendor: string;
  version: string;
  product_type: string;
  category: string;
  subcategory?: string | null;
  license: string;
  description: string;
  is_paid: boolean;
  is_installable: boolean;
  installed: boolean;
  installed_version?: string | null;
  formats: PackageFormatSummary[];
};

export type PackageDetails = {
  summary: PackageSummary;
  aliases: string[];
  homepage?: string | null;
  purchase_url?: string | null;
  available_versions: string[];
  bundle_ids: string[];
};

export type PackageDetailsResult =
  | { status: "catalog_empty" }
  | { status: "not_found" }
  | { status: "found"; package: PackageDetails };

export type PackageFormatSummary = {
  format: string;
  install_type: string;
  download_type: string;
  bundle_path?: string | null;
  has_checksum: boolean;
};

export type InstalledPackageSummary = {
  slug: string;
  version: string;
  vendor: string;
  formats: Array<{ format: string; path: string }>;
  source: string;
  pinned: boolean;
  origin: string;
};

export type SetPackagePinResult =
  | { status: "not_installed"; slug: string }
  | { status: "changed"; package: InstalledPackageSummary; pinned: boolean }
  | { status: "unchanged"; package: InstalledPackageSummary; pinned: boolean };

export type PackageUpdateAction = "installable" | "pinned" | "external";

export type PackageUpdateSummary = {
  slug: string;
  vendor: string;
  installed_version: string;
  available_version: string;
  pinned: boolean;
  origin: string;
  action: PackageUpdateAction;
};

export type UpdatePackageResult =
  | { status: "catalog_empty" }
  | { status: "not_installed"; slug: string }
  | { status: "not_found"; slug: string }
  | { status: "up_to_date"; slug: string; version: string }
  | { status: "pinned"; update: PackageUpdateSummary }
  | { status: "external"; update: PackageUpdateSummary }
  | {
      status: "install_unavailable";
      update: PackageUpdateSummary;
      result: InstallPackageResult;
    }
  | {
      status: "updated";
      update: PackageUpdateSummary;
      package: InstalledPackageSummary;
    };

export type AvailableUpdatesResult =
  | { status: "catalog_empty" }
  | {
      status: "ready";
      installed_count: number;
      updates: PackageUpdateSummary[];
      up_to_date_count: number;
      pinned_count: number;
      external_count: number;
      missing_count: number;
    };

export type DiagnosticStatus = "ok" | "warning" | "failure";

export type DiagnosticCheck = {
  name: string;
  status: DiagnosticStatus;
  detail: string;
  hint?: string | null;
};

export type DiagnosticsSummary = {
  ok: number;
  warnings: number;
  failures: number;
};

export type DiagnosticsReport = {
  checks: DiagnosticCheck[];
  summary: DiagnosticsSummary;
};

export type DesktopSnapshot = {
  service: DesktopServiceSession;
  distribution: DesktopDistribution;
  source_count: number;
  catalog: PackageSearchResult;
  installed: InstalledPackageSummary[];
  updates: AvailableUpdatesResult;
  models: ModelListResult;
  model_catalog: ModelCatalogListResult;
  model_store: ModelStoreLayout;
  diagnostics: DiagnosticsReport;
  recovery: OperationRecoverySummary;
  operations: OperationStatus[];
};

export type DesktopDistributionChannel =
  | "browser_preview"
  | "development"
  | "preview_bundle"
  | "public_release";

export type DesktopDistribution = {
  channel: DesktopDistributionChannel;
  app_version: string;
  build_profile: "browser" | "debug" | "release";
  sidecar_policy: "sample_data" | "external_or_bundled_cli" | "bundled_cli";
  release_gate: "not_applicable" | "required" | "selected";
  signing: "not_checked" | "developer_id_required";
  notarization: "not_checked" | "required";
  message: string;
};

export type DesktopServiceStatus =
  | "not_started"
  | "reused"
  | "started"
  | "unavailable"
  | "preview";

export type DesktopServiceSession = {
  status: DesktopServiceStatus;
  url: string;
  pid?: number | null;
  api_version: string;
  schema_version: string;
  token_header: string;
  token_file: string;
  token_available: boolean;
  privileged_install_policy: PrivilegedInstallPolicy;
  pending_runtime_work: string[];
  message: string;
};

export type PrivilegedInstallExecution = "external_handoff_only";

export type PrivilegedInstallPrerequisiteId =
  | "helper_or_escalation_design"
  | "explicit_user_consent"
  | "package_verification"
  | "audit_trail"
  | "rollback_plan";

export type PrivilegedInstallPrerequisiteStatus = "missing" | "designed" | "required";

export type PrivilegedInstallPrerequisite = {
  id: PrivilegedInstallPrerequisiteId;
  status: PrivilegedInstallPrerequisiteStatus;
  message: string;
};

export type PrivilegedInstallPolicy = {
  execution: PrivilegedInstallExecution;
  handoff_kind: "privileged_installer";
  requires_user_confirmation: boolean;
  runs_pkg_installers: boolean;
  design: PrivilegedInstallDesign;
  prerequisites: PrivilegedInstallPrerequisite[];
  message: string;
};

export type PrivilegedInstallDesign = {
  helper_strategy: "signed_helper_deferred";
  helper: PrivilegedInstallHelperDesign;
  rollback_strategy: "receipt_backed_uninstall_deferred";
  rollback: PrivilegedInstallRollbackDesign;
  execution_gate: string;
};

export type PrivilegedInstallDesignStatus = "designed";

export type PrivilegedInstallHelperDesign = {
  status: PrivilegedInstallDesignStatus;
  label: string;
  bundle_identifier: string;
  mach_service_name: string;
  install_path: string;
  launchd_plist_path: string;
  required_signing_identity: "Developer ID Application";
  requires_authorization: boolean;
};

export type PrivilegedInstallRollbackDesign = {
  status: PrivilegedInstallDesignStatus;
  receipt_store_relative_path: string;
  receipt_required_before_mutation: boolean;
  preflight_snapshot_required: boolean;
  uninstall_requires_receipt: boolean;
  message: string;
};

export type RegistrySyncResult = {
  sources: Array<
    | {
        status: "ok";
        name: string;
        catalog_item_count: number;
        installable_product_count: number;
      }
    | { status: "error"; name: string; error: string }
  >;
};

export type ScanMatchMethod = "bundle_id" | "name_vendor" | "name_only";

export type InstallScope = "user" | "system";

export type ScannedPackageSummary = {
  name: string;
  version: string;
  vendor: string;
  format: "au" | "vst3";
  scope: InstallScope;
  path: string;
  tracked_by_apm: boolean;
  origin?: "apm" | "external" | null;
  registry_slug?: string | null;
  match_method?: ScanMatchMethod | null;
};

export type ScanPackagesResult = {
  scanned_count: number;
  visible_count: number;
  matched_count: number;
  tracked_count: number;
  adopted_count: number;
  learned_bundle_id_count: number;
  au_count: number;
  vst3_count: number;
  plugins: ScannedPackageSummary[];
};

export type OperationState =
  | "queued"
  | "running"
  | "cancel_requested"
  | "canceled"
  | "succeeded"
  | "failed";

export type OperationKind =
  | "registry_sync"
  | "library_scan"
  | "install_url"
  | "install_archive"
  | "package_update"
  | "package_remove"
  | "model_weight_pull"
  | "model_install"
  | "model_run";

export type OperationResult =
  | { kind: "registry_sync"; result: RegistrySyncResult }
  | { kind: "library_scan"; result: ScanPackagesResult }
  | { kind: "install_package"; result: InstallPackageResult }
  | { kind: "update_package"; result: UpdatePackageResult }
  | { kind: "remove_package"; result: RemovePackageResult }
  | { kind: "model_weight_pull"; result: ModelWeightPullResult }
  | { kind: "model_install"; result: ModelInstallResult }
  | { kind: "model_run"; result: ModelRunResult };

export type InstallPackageRequest = {
  slug: string;
  version?: string | null;
  format?: string | null;
  scope?: string | null;
  archive_path?: string | null;
};

export type PackageUpdateBody = {
  format?: string | null;
  scope?: string | null;
};

export type PackageRemoveBody = {
  dry_run?: boolean;
};

export type OperationRequest =
  | { kind: "registry_sync" }
  | { kind: "library_scan" }
  | { kind: "install_url"; request: InstallPackageRequest }
  | { kind: "install_archive"; request: InstallPackageRequest }
  | { kind: "package_update"; slug: string; body: PackageUpdateBody }
  | { kind: "package_remove"; slug: string; body: PackageRemoveBody }
  | { kind: "model_weight_pull"; name: string; version: string }
  | { kind: "model_install"; name: string; version: string }
  | { kind: "model_run"; name: string; version: string; request: ModelRunPlanRequest };

export type OperationStatus = {
  operation_id: string;
  kind: OperationKind;
  request?: OperationRequest | null;
  state: OperationState;
  created_at: string;
  started_at?: string | null;
  finished_at?: string | null;
  result?: OperationResult | null;
  error?: string | null;
  events: EngineEvent[];
};

export type OperationCancelResult = {
  operation_id: string;
  state: OperationState;
  accepted: boolean;
  message: string;
};

export type OperationRetryResult = {
  original_operation_id: string;
  operation: {
    operation_id: string;
    kind: OperationKind;
    status_url: string;
  };
  message: string;
};

export type OperationRecoveryCandidate = {
  operation_id: string;
  kind: OperationKind;
  created_at: string;
  finished_at?: string | null;
  retryable: boolean;
  reason: string;
};

export type OperationRecoverySummary = {
  interrupted_count: number;
  retryable_count: number;
  candidates: OperationRecoveryCandidate[];
};

export type InstallPlanResult =
  | { status: "catalog_empty" }
  | { status: "not_found"; query: string; suggestions: string[] }
  | {
      status: "not_installable";
      slug: string;
      name: string;
      product_type: string;
    }
  | {
      status: "version_not_found";
      slug: string;
      requested_version: string;
      available_versions: string[];
    }
  | {
      status: "format_unavailable";
      slug: string;
      requested_format?: string | null;
      available_formats: string[];
    }
  | { status: "plan"; plan: PackageInstallPlan };

export type InstallPlanStatus =
  | "ready"
  | "already_installed"
  | "manual_required"
  | "privileged_installer_required"
  | "app_store_required"
  | "vendor_installer_available"
  | "vendor_installer_required";

export type PackageInstallPlan = {
  slug: string;
  name: string;
  vendor: string;
  version: string;
  status: InstallPlanStatus;
  destination?: string | null;
  scope: InstallScope;
  installed_version?: string | null;
  formats: InstallPlanFormat[];
  installer?: VendorInstallerPlan | null;
  message: string;
};

export type InstallPlanFormat = {
  format: string;
  install_type: string;
  download_type: string;
  source: string;
  bundle_path?: string | null;
  has_checksum: boolean;
};

export type VendorInstallerPlan = {
  key: string;
  name: string;
  download_url: string;
  homepage: string;
  installed_app_path?: string | null;
};

export type InstallHandoffResult =
  | {
      status: "open";
      plan: PackageInstallPlan;
      handoff: InstallHandoff;
    }
  | {
      status: "no_handoff";
      plan: PackageInstallPlan;
      reason: string;
    }
  | {
      status: "plan_unavailable";
      plan: InstallPlanResult;
    };

export type InstallHandoff = {
  kind:
    | "manual_download"
    | "privileged_installer"
    | "app_store"
    | "vendor_app"
    | "vendor_download";
  label: string;
  target: InstallHandoffTarget;
  message: string;
};

export type InstallHandoffTarget =
  | { kind: "app"; path: string }
  | { kind: "url"; url: string };

export type InstallPackageResult =
  | { status: "plan_unavailable"; plan: InstallPlanResult }
  | { status: "already_installed"; plan: PackageInstallPlan }
  | {
      status: "external_handoff_required";
      plan: PackageInstallPlan;
      reason: string;
    }
  | {
      status: "format_required";
      plan: PackageInstallPlan;
      available_formats: string[];
      reason: string;
    }
  | {
      status: "archive_required";
      plan: PackageInstallPlan;
      reason: string;
    }
  | {
      status: "unsupported_install_type";
      plan: PackageInstallPlan;
      format: string;
      install_type: string;
      reason: string;
    }
  | { status: "installed"; package: InstalledPackageSummary };

export type RegistryEvent =
  | { event: "registry_sync_started"; source_count: number }
  | { event: "registry_source_sync_started"; source: string }
  | {
      event: "registry_source_sync_finished";
      source: string;
      catalog_item_count: number;
      installable_product_count: number;
    }
  | { event: "registry_source_sync_failed"; source: string; error: string }
  | { event: "registry_sync_finished"; source_count: number; failed_count: number };

export type InstallEvent =
  | { event: "install_started"; slug: string; version: string; format_count: number }
  | { event: "install_format_started"; slug: string; format: string }
  | { event: "install_download_started"; slug: string; format: string; url: string }
  | {
      event: "install_download_progress";
      slug: string;
      format: string;
      bytes: number;
      total_bytes?: number | null;
    }
  | {
      event: "install_download_finished";
      slug: string;
      format: string;
      path: string;
      bytes: number;
    }
  | {
      event: "install_archive_install_started";
      slug: string;
      format: string;
      install_type: string;
      path: string;
    }
  | {
      event: "install_archive_verified";
      slug: string;
      format: string;
      path: string;
      sha256: string;
    }
  | {
      event: "install_quarantine_removal_started";
      slug: string;
      format: string;
      path: string;
    }
  | { event: "install_format_placed"; slug: string; format: string; path: string }
  | { event: "install_state_recording_started"; slug: string }
  | { event: "install_state_recorded"; slug: string }
  | { event: "install_rolled_back"; slug: string; format: string; path: string }
  | { event: "install_finished"; slug: string; installed_format_count: number }
  | { event: "install_failed"; slug: string; error: string };

export type RemoveFormatSummary = {
  format: string;
  path: string;
  existed: boolean;
};

export type RemovePackageResult =
  | { status: "not_installed"; slug: string }
  | {
      status: "external_install_present";
      package: InstalledPackageSummary;
      reason: string;
    }
  | {
      status: "dry_run";
      package: InstalledPackageSummary;
      formats: RemoveFormatSummary[];
      would_delete_files: boolean;
      reason?: string | null;
    }
  | {
      status: "removed";
      package: InstalledPackageSummary;
      removed_formats: RemoveFormatSummary[];
      state_only: boolean;
    };

export type RemoveEvent =
  | { event: "remove_started"; slug: string; version: string; format_count: number }
  | { event: "remove_format_removed"; slug: string; format: string; path: string }
  | { event: "remove_format_missing"; slug: string; format: string; path: string }
  | { event: "remove_state_recorded"; slug: string }
  | { event: "remove_finished"; slug: string; removed_format_count: number }
  | { event: "remove_failed"; slug: string; error: string };

export type ScanEvent =
  | { event: "scan_started" }
  | {
      event: "scan_finished";
      scanned_count: number;
      matched_count: number;
      adopted_count: number;
    };

export type LifecycleEvent = InstallEvent | RemoveEvent | ScanEvent;

export type EngineEvent = RegistryEvent | LifecycleEvent | ModelOperationEvent;

export type DesktopInstallResult =
  | { status: "completed"; result: InstallPackageResult; events: InstallEvent[] }
  | { status: "failed"; error: string; events: InstallEvent[] };

export type DesktopRemoveResult =
  | { status: "completed"; result: RemovePackageResult; events: RemoveEvent[] }
  | { status: "failed"; error: string; events: RemoveEvent[] };

export type DesktopUpdateResult =
  | { status: "completed"; result: UpdatePackageResult; events: InstallEvent[] }
  | { status: "failed"; error: string; events: InstallEvent[] };

export type DesktopScanResult =
  | { status: "completed"; result: ScanPackagesResult; events: ScanEvent[] }
  | { status: "failed"; error: string; events: ScanEvent[] };
