import {
  removePackageCommand,
  scanLibraryCommand,
  setPackagePinCommand,
  updatePackageCommand,
} from "./commands";
import {
  canRunUpdateCandidate,
  updatePackageCandidate,
} from "./operation-candidates";
import {
  withPreviewInstall,
  withPreviewPackagePin,
  withPreviewRemove,
  withPreviewScan,
} from "./preview-state";
import type {
  DesktopRemoveResult,
  DesktopScanResult,
  DesktopSnapshot,
  DesktopUpdateResult,
  InstalledPackageSummary,
  LifecycleEvent,
} from "./types";
import { updateResultNotice } from "./update-notices";
import { formatLabel } from "./view-utils";
import type {
  LifecycleNotice,
  UpdateAllPackagesCandidate,
  UpdatePackageCandidate,
} from "./view-model";

export type LibraryControllerState = {
  pendingUpdateAllPackages: UpdateAllPackagesCandidate | null;
  pendingUpdatePackage: UpdatePackageCandidate | null;
  pendingRemovePackage: InstalledPackageSummary | null;
  libraryNotice: LifecycleNotice | null;
  libraryEvents: LifecycleEvent[];
  updateAllCount: number;
};

type LibraryControllerHost = {
  snapshot(): DesktopSnapshot;
  setSnapshot(snapshot: DesktopSnapshot): void;
  isTauriRuntime(): boolean;
  libraryOperationActive(): boolean;
  reloadSnapshot(): Promise<void>;
  runLibraryOperation<T>(run: (progressId?: string) => Promise<T>): Promise<T>;
  startLibraryOperation(): void;
  clearLibraryOperation(): void;
  reportLibraryError(error: unknown): Promise<void>;
  clearPeerDialogs(): void;
  clearInstallStateForRemovedPackage(slug: string): void;
  formatError(error: unknown): string;
  render(): void;
};

export function createLibraryController(host: LibraryControllerHost) {
  let pendingUpdateAllPackages: UpdateAllPackagesCandidate | null = null;
  let pendingUpdatePackage: UpdatePackageCandidate | null = null;
  let pendingRemovePackage: InstalledPackageSummary | null = null;
  let libraryNotice: LifecycleNotice | null = null;
  let libraryEvents: LifecycleEvent[] = [];

  function state(): LibraryControllerState {
    return {
      pendingUpdateAllPackages,
      pendingUpdatePackage,
      pendingRemovePackage,
      libraryNotice,
      libraryEvents,
      updateAllCount: installableUpdates().length,
    };
  }

  function setLibraryNotice(notice: LifecycleNotice) {
    libraryNotice = notice;
  }

  function appendLibraryEvent(event: LifecycleEvent) {
    libraryEvents = [...libraryEvents, event];
  }

  function clearPending() {
    pendingUpdateAllPackages = null;
    pendingUpdatePackage = null;
    pendingRemovePackage = null;
  }

  function clearPendingUpdate() {
    pendingUpdateAllPackages = null;
    pendingUpdatePackage = null;
  }

  function clearEvents() {
    libraryEvents = [];
  }

  function installableUpdates() {
    return installableUpdatesForSnapshot(host.snapshot());
  }

  function libraryActionLocked() {
    return host.libraryOperationActive();
  }

  async function scanLibrary() {
    if (libraryActionLocked()) {
      return;
    }

    clearPending();
    host.clearPeerDialogs();
    libraryNotice = { tone: "info", message: "Scanning local plugin folders" };
    libraryEvents = [];
    host.startLibraryOperation();
    host.render();

    try {
      const scanResult = await host.runLibraryOperation(scanLibraryCommand);
      libraryEvents = scanResult.events;
      if (scanResult.status === "failed") {
        await host.reportLibraryError(scanResult.error);
        host.render();
        return;
      }
      await handleScanResult(scanResult);
    } catch (error) {
      await host.reportLibraryError(error);
    } finally {
      host.clearLibraryOperation();
      host.render();
    }
  }

  async function handleScanResult(
    scanResult: Extract<DesktopScanResult, { status: "completed" }>,
  ) {
    const { result } = scanResult;
    libraryNotice = {
      tone: "success",
      message: `Scanned ${result.scanned_count} bundle${result.scanned_count === 1 ? "" : "s"}; tracked ${result.adopted_count} new external package${result.adopted_count === 1 ? "" : "s"}.`,
    };
    if (host.isTauriRuntime()) {
      await host.reloadSnapshot();
    } else {
      host.setSnapshot(withPreviewScan(host.snapshot(), result));
      host.render();
    }
  }

  function requestUpdatePackage(slug: string) {
    if (libraryActionLocked()) {
      return;
    }

    const update = updateForSnapshot(host.snapshot(), slug);
    if (!update) {
      libraryNotice = { tone: "info", message: `${slug} is already current.` };
      libraryEvents = [];
      host.render();
      return;
    }
    if (update.action !== "installable") {
      libraryNotice = {
        tone: "info",
        message:
          update.action === "pinned"
            ? `${slug} is pinned. Unpin it before updating.`
            : `${slug} is managed outside apm.`,
      };
      libraryEvents = [];
      host.render();
      return;
    }

    pendingUpdateAllPackages = null;
    pendingUpdatePackage = updatePackageCandidate(host.snapshot().installed, update);
    host.clearPeerDialogs();
    pendingRemovePackage = null;
    libraryNotice = null;
    libraryEvents = [];
    host.render();
  }

  function cancelUpdatePackage() {
    pendingUpdatePackage = null;
    libraryNotice = { tone: "info", message: "Update canceled" };
    libraryEvents = [];
    host.render();
  }

  async function confirmUpdatePackage() {
    if (libraryActionLocked()) {
      return;
    }

    const update = pendingUpdatePackage;
    if (!update) {
      return;
    }

    if (!canRunUpdateCandidate(update)) {
      pendingUpdatePackage = null;
      libraryNotice = {
        tone: "info",
        message: `${update.slug} has no tracked format to update.`,
      };
      libraryEvents = [];
      host.render();
      return;
    }

    const format = update.updateFormat;
    pendingUpdatePackage = null;
    host.startLibraryOperation();
    libraryNotice = {
      tone: "info",
      message: format ? `Updating ${update.slug} ${formatLabel(format)}` : `Updating ${update.slug}`,
    };
    libraryEvents = [];
    host.render();

    try {
      const updateResult = await host.runLibraryOperation((progressId) =>
        updatePackageCommand(update.slug, format, progressId),
      );
      libraryEvents = updateResult.events;
      if (updateResult.status === "failed") {
        await host.reportLibraryError(updateResult.error);
        host.render();
        return;
      }
      await handleUpdateResult(updateResult);
    } catch (error) {
      await host.reportLibraryError(error);
    } finally {
      host.clearLibraryOperation();
      host.render();
    }
  }

  function requestUpdateAllPackages() {
    if (libraryActionLocked()) {
      return;
    }

    const updates = installableUpdates();
    if (updates.length === 0) {
      libraryNotice = { tone: "info", message: "No installable updates ready." };
      libraryEvents = [];
      host.render();
      return;
    }

    pendingUpdateAllPackages = { updates };
    pendingUpdatePackage = null;
    pendingRemovePackage = null;
    host.clearPeerDialogs();
    libraryNotice = null;
    libraryEvents = [];
    host.render();
  }

  function cancelUpdateAllPackages() {
    pendingUpdateAllPackages = null;
    libraryNotice = { tone: "info", message: "Update canceled" };
    libraryEvents = [];
    host.render();
  }

  async function confirmUpdateAllPackages() {
    if (libraryActionLocked()) {
      return;
    }

    const candidate = pendingUpdateAllPackages;
    if (!candidate) {
      return;
    }

    pendingUpdateAllPackages = null;
    await runUpdateBatch(candidate.updates);
  }

  async function runUpdateBatch(updates: UpdatePackageCandidate[]) {
    host.startLibraryOperation();
    libraryNotice = {
      tone: "info",
      message: `Updating ${updates.length} package${updates.length === 1 ? "" : "s"}`,
    };
    libraryEvents = [];
    host.render();

    let updatedCount = 0;
    try {
      for (const update of updates) {
        const updateResult = await host.runLibraryOperation((progressId) =>
          updatePackageCommand(update.slug, update.updateFormat, progressId),
        );
        libraryEvents = [...libraryEvents, ...updateResult.events];
        if (updateResult.status === "failed") {
          await host.reportLibraryError(updateResult.error);
          host.render();
          return;
        }
        if (updateResult.result.status !== "updated") {
          libraryNotice = updateResultNotice(updateResult.result);
          host.render();
          return;
        }
        updatedCount += 1;
        applyUpdatedPackage(updateResult.result.package);
      }

      libraryNotice = {
        tone: "success",
        message: `Updated ${updatedCount} package${updatedCount === 1 ? "" : "s"}.`,
      };
      if (host.isTauriRuntime()) {
        await host.reloadSnapshot();
      } else {
        host.render();
      }
    } catch (error) {
      await host.reportLibraryError(error);
    } finally {
      host.clearLibraryOperation();
      host.render();
    }
  }

  function requestRemovePackage(slug: string) {
    if (libraryActionLocked()) {
      return;
    }

    const packageItem = host.snapshot().installed.find((item) => item.slug === slug);
    if (!packageItem) {
      libraryNotice = { tone: "error", message: `${slug} is not tracked locally.` };
      libraryEvents = [];
      host.render();
      return;
    }

    pendingRemovePackage = packageItem;
    host.clearPeerDialogs();
    pendingUpdateAllPackages = null;
    pendingUpdatePackage = null;
    libraryNotice = null;
    libraryEvents = [];
    host.render();
  }

  function cancelRemovePackage() {
    pendingRemovePackage = null;
    libraryNotice = { tone: "info", message: "Remove canceled" };
    libraryEvents = [];
    host.render();
  }

  async function confirmRemovePackage() {
    if (libraryActionLocked()) {
      return;
    }

    const packageItem = pendingRemovePackage;
    if (!packageItem) {
      return;
    }

    pendingRemovePackage = null;
    host.startLibraryOperation();
    libraryNotice = { tone: "info", message: `Removing ${packageItem.slug}` };
    host.render();

    try {
      const removeResult = await host.runLibraryOperation((progressId) =>
        removePackageCommand(packageItem.slug, progressId),
      );
      libraryEvents = removeResult.events;
      if (removeResult.status === "failed") {
        await host.reportLibraryError(removeResult.error);
        host.render();
        return;
      }
      await handleRemoveResult(removeResult);
    } catch (error) {
      await host.reportLibraryError(error);
    } finally {
      host.clearLibraryOperation();
      host.render();
    }
  }

  async function setPackagePin(slug: string, pinned: boolean) {
    if (libraryActionLocked()) {
      return;
    }

    libraryNotice = {
      tone: "info",
      message: `${pinned ? "Pinning" : "Unpinning"} ${slug}`,
    };
    libraryEvents = [];
    host.render();

    try {
      const result = await setPackagePinCommand(slug, pinned);
      switch (result.status) {
        case "not_installed":
          libraryNotice = { tone: "error", message: `${result.slug} is not installed.` };
          break;
        case "changed":
        case "unchanged":
          libraryNotice = {
            tone: "success",
            message: `${result.package.slug} ${result.pinned ? "pinned" : "unpinned"}.`,
          };
          if (host.isTauriRuntime()) {
            await host.reloadSnapshot();
            host.render();
          } else {
            host.setSnapshot(
              withPreviewPackagePin(host.snapshot(), result.package.slug, result.pinned),
            );
            host.render();
          }
          return;
      }
    } catch (error) {
      libraryNotice = { tone: "error", message: host.formatError(error) };
    }
    host.render();
  }

  async function handleUpdateResult(
    updateResult: Extract<DesktopUpdateResult, { status: "completed" }>,
  ) {
    const { result } = updateResult;
    if (result.status !== "updated") {
      libraryNotice = updateResultNotice(result);
      host.render();
      return;
    }

    libraryNotice = {
      tone: "success",
      message: `${result.package.slug} updated to ${result.package.version}.`,
    };
    if (host.isTauriRuntime()) {
      await host.reloadSnapshot();
    } else {
      applyUpdatedPackage(result.package);
      host.render();
    }
  }

  function applyUpdatedPackage(packageItem: InstalledPackageSummary) {
    if (!host.isTauriRuntime()) {
      host.setSnapshot(withPreviewInstall(host.snapshot(), packageItem));
    }
  }

  async function handleRemoveResult(
    removeResult: Extract<DesktopRemoveResult, { status: "completed" }>,
  ) {
    const { result } = removeResult;
    switch (result.status) {
      case "removed":
        host.clearInstallStateForRemovedPackage(result.package.slug);
        libraryNotice = {
          tone: "success",
          message: result.state_only
            ? `${result.package.slug} stale state entry removed.`
            : `${result.package.slug} removed from this Mac.`,
        };
        if (host.isTauriRuntime()) {
          await host.reloadSnapshot();
        } else {
          host.setSnapshot(withPreviewRemove(host.snapshot(), result.package.slug));
          host.render();
        }
        return;
      case "not_installed":
        libraryNotice = { tone: "info", message: `${result.slug} is not installed.` };
        break;
      case "external_install_present":
        libraryNotice = { tone: "error", message: result.reason };
        break;
      case "dry_run":
        libraryNotice = { tone: "info", message: "Dry-run remove completed." };
        break;
    }
    host.render();
  }

  return {
    appendLibraryEvent,
    cancelRemovePackage,
    cancelUpdateAllPackages,
    cancelUpdatePackage,
    clearEvents,
    clearPending,
    clearPendingUpdate,
    confirmRemovePackage,
    confirmUpdateAllPackages,
    confirmUpdatePackage,
    requestRemovePackage,
    requestUpdateAllPackages,
    requestUpdatePackage,
    scanLibrary,
    setLibraryNotice,
    setPackagePin,
    state,
  };
}

function updateForSnapshot(snapshot: DesktopSnapshot, slug: string) {
  if (snapshot.updates.status !== "ready") {
    return null;
  }
  return snapshot.updates.updates.find((update) => update.slug === slug) ?? null;
}

function installableUpdatesForSnapshot(snapshot: DesktopSnapshot) {
  if (snapshot.updates.status !== "ready") {
    return [];
  }
  return snapshot.updates.updates
    .filter((update) => update.action === "installable")
    .map((update) => updatePackageCandidate(snapshot.installed, update))
    .filter(canRunUpdateCandidate);
}
