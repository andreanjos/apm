import {
  fallbackInstallFromArchive,
  fallbackInstallFromUrl,
  fallbackImportModelCatalogPackage,
  fallbackImportModelManifest,
  fallbackInstallModelPackage,
  fallbackInstallHandoff,
  fallbackInstallPlan,
  fallbackPackageDetails,
  fallbackPlanModelChain,
  fallbackPlanModelRun,
  fallbackPullModelWeights,
  fallbackRemoveModelPackage,
  fallbackRunModel,
  fallbackScanLibrary,
  fallbackRemovePackage,
  fallbackSetPackagePin,
  fallbackSnapshot,
  fallbackUpdatePackage,
} from "./fallback";
import { isTauriRuntime } from "./service-session";
import type {
  DesktopInstallResult,
  DesktopModelInstallResult,
  DesktopModelRunResult,
  DesktopModelWeightPullResult,
  DesktopRemoveResult,
  DesktopScanResult,
  DesktopSnapshot,
  DesktopUpdateResult,
  InstallHandoffResult,
  InstallScope,
  InstallPlanResult,
  PackageDetailsResult,
  ModelManifestCacheResult,
  ModelChainPlan,
  ModelChainPlanRequest,
  ModelStoreInitResult,
  ModelRemoveResult,
  ModelRunPlan,
  OperationCancelResult,
  OperationStatus,
  RegistrySyncResult,
  SetPackagePinResult,
} from "./types";
import type {
  ArchiveInstallCandidate,
  UrlInstallCandidate,
} from "./view-model";

async function invokeTauri<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

export async function desktopSnapshotCommand(): Promise<DesktopSnapshot> {
  if (!isTauriRuntime()) {
    return fallbackSnapshot;
  }
  return invokeTauri("desktop_snapshot");
}

export async function packageDetailsCommand(slug: string): Promise<PackageDetailsResult> {
  if (!isTauriRuntime()) {
    return fallbackPackageDetails(slug);
  }
  return invokeTauri("package_details", { slug });
}

export async function syncRegistriesCommand(
  progressId?: string,
): Promise<RegistrySyncResult> {
  if (!isTauriRuntime()) {
    return {
      sources: [
        {
          status: "ok",
          name: "preview",
          catalog_item_count: 4,
          installable_product_count: 3,
        },
      ],
    };
  }
  return invokeTauri("sync_registries", progressArgs(progressId));
}

export async function planInstallCommand(
  slug: string,
  installScope: InstallScope = "user",
): Promise<InstallPlanResult> {
  if (!isTauriRuntime()) {
    return fallbackInstallPlan(slug, installScope);
  }
  return invokeTauri("plan_install", { slug, scope: installScope });
}

export async function openInstallHandoffCommand(
  slug: string,
): Promise<InstallHandoffResult> {
  if (!isTauriRuntime()) {
    return fallbackInstallHandoff(slug);
  }
  return invokeTauri("open_install_handoff", { slug });
}

export async function installFromArchiveCommand(
  candidate: ArchiveInstallCandidate,
  progressId?: string,
): Promise<DesktopInstallResult> {
  if (!isTauriRuntime()) {
    return fallbackInstallFromArchive(
      candidate.slug,
      candidate.format,
      candidate.installScope,
    );
  }
  return invokeTauri("install_from_archive", {
    slug: candidate.slug,
    archivePath: candidate.archivePath,
    format: candidate.format,
    scope: candidate.installScope,
    ...progressArgs(progressId),
  });
}

export async function installFromUrlCommand(
  candidate: UrlInstallCandidate,
  progressId?: string,
): Promise<DesktopInstallResult> {
  if (!isTauriRuntime()) {
    return fallbackInstallFromUrl(
      candidate.slug,
      candidate.format,
      candidate.installScope,
    );
  }
  return invokeTauri("install_from_url", {
    slug: candidate.slug,
    format: candidate.format,
    scope: candidate.installScope,
    ...progressArgs(progressId),
  });
}

export async function removePackageCommand(
  slug: string,
  progressId?: string,
): Promise<DesktopRemoveResult> {
  if (!isTauriRuntime()) {
    return fallbackRemovePackage(slug);
  }
  return invokeTauri("remove_package", { slug, ...progressArgs(progressId) });
}

export async function setPackagePinCommand(
  slug: string,
  pinned: boolean,
): Promise<SetPackagePinResult> {
  if (!isTauriRuntime()) {
    return fallbackSetPackagePin(slug, pinned);
  }
  return invokeTauri("set_package_pin", { slug, pinned });
}

export async function updatePackageCommand(
  slug: string,
  format: string | null,
  progressId?: string,
): Promise<DesktopUpdateResult> {
  if (!isTauriRuntime()) {
    return fallbackUpdatePackage(slug, format);
  }
  return invokeTauri("update_package", { slug, format, ...progressArgs(progressId) });
}

export async function scanLibraryCommand(
  progressId?: string,
): Promise<DesktopScanResult> {
  if (!isTauriRuntime()) {
    return fallbackScanLibrary();
  }
  return invokeTauri("scan_library", progressArgs(progressId));
}

export async function cancelOperationCommand(
  operationId: string,
): Promise<OperationCancelResult> {
  if (!isTauriRuntime()) {
    return {
      operation_id: operationId,
      state: "canceled",
      accepted: true,
      message: "Preview operation canceled.",
    };
  }
  return invokeTauri("cancel_operation", { operationId });
}

export async function retryOperationCommand(
  operationId: string,
  progressId?: string,
): Promise<OperationStatus> {
  if (!isTauriRuntime()) {
    return previewRetryStatus(operationId);
  }
  return invokeTauri("retry_operation", { operationId, ...progressArgs(progressId) });
}

export async function retryRecoveryOperationsCommand(
  progressId?: string,
): Promise<OperationStatus[]> {
  if (!isTauriRuntime()) {
    return [previewRetryStatus("recovery")];
  }
  return invokeTauri("retry_recovery_operations", progressArgs(progressId));
}

export async function importModelManifestCommand(
  manifestPath: string,
): Promise<ModelManifestCacheResult> {
  if (!isTauriRuntime()) {
    return fallbackImportModelManifest();
  }
  return invokeTauri("import_model_manifest", { manifestPath });
}

export async function initializeModelStoreCommand(): Promise<ModelStoreInitResult> {
  if (!isTauriRuntime()) {
    return { layout: fallbackSnapshot.model_store };
  }
  return invokeTauri("initialize_model_store");
}

export async function importModelCatalogPackageCommand(
  name: string,
  version: string,
): Promise<ModelManifestCacheResult> {
  if (!isTauriRuntime()) {
    return fallbackImportModelCatalogPackage(name, version);
  }
  return invokeTauri("import_model_catalog_package", { name, version });
}

export async function pullModelWeightsCommand(
  name: string,
  version: string,
  progressId?: string,
): Promise<DesktopModelWeightPullResult> {
  if (!isTauriRuntime()) {
    return fallbackPullModelWeights(name, version);
  }
  return invokeTauri("pull_model_weights", { name, version, ...progressArgs(progressId) });
}

export async function installModelPackageCommand(
  name: string,
  version: string,
  progressId?: string,
): Promise<DesktopModelInstallResult> {
  if (!isTauriRuntime()) {
    return fallbackInstallModelPackage(name, version);
  }
  return invokeTauri("install_model_package", { name, version, ...progressArgs(progressId) });
}

export async function removeModelPackageCommand(
  name: string,
  version: string,
): Promise<ModelRemoveResult> {
  if (!isTauriRuntime()) {
    return fallbackRemoveModelPackage(name, version);
  }
  return invokeTauri("remove_model_package", { name, version });
}

export async function planModelRunCommand(
  name: string,
  version: string,
  inputPath: string,
  outputPath: string,
): Promise<ModelRunPlan> {
  if (!isTauriRuntime()) {
    return fallbackPlanModelRun(name, version, inputPath, outputPath);
  }
  return invokeTauri("plan_model_run", { name, version, inputPath, outputPath });
}

export async function runModelCommand(
  name: string,
  version: string,
  inputPath: string,
  outputPath: string,
  progressId?: string,
): Promise<DesktopModelRunResult> {
  if (!isTauriRuntime()) {
    return fallbackRunModel(name, version, inputPath, outputPath);
  }
  return invokeTauri("run_model", {
    name,
    version,
    inputPath,
    outputPath,
    ...progressArgs(progressId),
  });
}

export async function planModelChainCommand(
  request: ModelChainPlanRequest,
): Promise<ModelChainPlan> {
  if (!isTauriRuntime()) {
    return fallbackPlanModelChain(request);
  }
  return invokeTauri("plan_model_chain", { request });
}

export async function chooseModelManifestPathCommand() {
  if (!isTauriRuntime()) {
    return "/Preview Manifests/demucs.toml";
  }

  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: "Choose model manifest",
    multiple: false,
    directory: false,
    fileAccessMode: "scoped",
    filters: [{ name: "TOML manifests", extensions: ["toml"] }],
  });

  return typeof selected === "string" ? selected : null;
}

export async function chooseModelRunInputPathCommand() {
  if (!isTauriRuntime()) {
    return "/Preview Audio/mix.wav";
  }

  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: "Choose model input audio",
    multiple: false,
    directory: false,
    fileAccessMode: "scoped",
    filters: [{ name: "Audio files", extensions: ["wav", "aiff", "aif", "flac", "mp3"] }],
  });
  return typeof selected === "string" ? selected : null;
}

export async function chooseModelRunOutputPathCommand() {
  if (!isTauriRuntime()) {
    return "/Preview Audio/stems";
  }

  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: "Choose model output folder",
    multiple: false,
    directory: true,
    fileAccessMode: "scoped",
  });
  return typeof selected === "string" ? selected : null;
}

export async function chooseArchivePathCommand(installType: string) {
  const archiveType = installType.toLowerCase();
  if (!isTauriRuntime()) {
    return `/Preview Downloads/apm-preview.${archiveType === "dmg" ? "dmg" : "zip"}`;
  }

  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    title: `Choose plugin ${archiveType.toUpperCase()} archive`,
    multiple: false,
    directory: false,
    fileAccessMode: "scoped",
    filters: [archiveFilter(archiveType)],
  });

  return typeof selected === "string" ? selected : null;
}

function archiveFilter(archiveType: string) {
  if (archiveType === "dmg") {
    return { name: "DMG archives", extensions: ["dmg"] };
  }
  return { name: "ZIP archives", extensions: ["zip"] };
}

function progressArgs(progressId: string | undefined) {
  return progressId ? { progressId } : {};
}

function previewRetryStatus(operationId: string): OperationStatus {
  const timestamp = new Date().toISOString();
  return {
    operation_id: `preview-retry-${operationId}`,
    kind: "package_update",
    request: {
      kind: "package_update",
      slug: "surge-xt",
      body: { format: "VST3" },
    },
    state: "succeeded",
    created_at: timestamp,
    started_at: timestamp,
    finished_at: timestamp,
    result: null,
    error: null,
    events: [],
  };
}
