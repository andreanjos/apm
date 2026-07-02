import {
  Activity,
  ArrowRight,
  ExternalLink,
  FilePlus,
  FolderOpen,
  GitBranch,
  HardDrive,
  Info,
  ListPlus,
  Package,
  Play,
  RefreshCw,
  Search,
  ShieldCheck,
  Terminal,
  Trash2,
  Download,
  X,
  createIcons,
} from "lucide";
import type { InstallScope } from "./types";
import type {
  CatalogAccessFilter,
  CatalogAvailabilityFilter,
  OperationScope,
  WorkspaceSection,
} from "./view-model";

type MaybePromise = Promise<void> | void;

let catalogSearchShortcutBound = false;

export type ViewEventHandlers = {
  syncRegistries(): MaybePromise;
  ensureLocalService(): MaybePromise;
  initializeModelStore(): MaybePromise;
  setCatalogSearchQuery(query: string): MaybePromise;
  setCatalogAvailabilityFilter(filter: CatalogAvailabilityFilter): MaybePromise;
  setCatalogProductTypeFilter(productType: string | null): MaybePromise;
  setCatalogAccessFilter(filter: CatalogAccessFilter): MaybePromise;
  reviewInstall(slug: string): MaybePromise;
  openInstallHandoff(slug: string): MaybePromise;
  confirmInstallHandoff(): MaybePromise;
  cancelInstallHandoff(): MaybePromise;
  setInstallScope(scope: InstallScope): MaybePromise;
  chooseArchiveAndInstall(slug: string, format: string): MaybePromise;
  requestUrlInstall(slug: string, format: string): MaybePromise;
  confirmArchiveInstall(scope?: InstallScope): MaybePromise;
  cancelArchiveInstall(): MaybePromise;
  confirmUrlInstall(scope?: InstallScope): MaybePromise;
  cancelUrlInstall(): MaybePromise;
  requestRemovePackage(slug: string): MaybePromise;
  requestUpdateAllPackages(): MaybePromise;
  requestUpdatePackage(slug: string): MaybePromise;
  confirmUpdateAllPackages(): MaybePromise;
  confirmUpdatePackage(): MaybePromise;
  cancelUpdateAllPackages(): MaybePromise;
  cancelUpdatePackage(): MaybePromise;
  setPackagePin(slug: string, pinned: boolean): MaybePromise;
  confirmRemovePackage(): MaybePromise;
  cancelRemovePackage(): MaybePromise;
  cancelActiveOperation(scope: OperationScope): MaybePromise;
  retryOperation(operationId: string): MaybePromise;
  retryRecoveryOperations(): MaybePromise;
  refreshDiagnostics(): MaybePromise;
  scanLibrary(): MaybePromise;
  importModelCatalogPackage(name: string, version: string): MaybePromise;
  importModelManifest(): MaybePromise;
  addModelChainStep(name: string, version: string, packageId: string): MaybePromise;
  clearModelChain(): MaybePromise;
  installModelPackage(name: string, version: string): MaybePromise;
  planModelChain(): MaybePromise;
  planModelRun(name: string, version: string): MaybePromise;
  pullModelWeights(name: string, version: string): MaybePromise;
  removeModelChainStep(stepIndex: number): MaybePromise;
  removeModelPackage(name: string, version: string): MaybePromise;
  runModel(name: string, version: string): MaybePromise;
  setModelSearchQuery(query: string): MaybePromise;
  selectPackage(slug: string): MaybePromise;
  setWorkspaceSection(section: WorkspaceSection): MaybePromise;
};

export function bindViewEvents(
  selectedPackageSlug: string | null,
  handlers: ViewEventHandlers,
) {
  bindWorkspaceNavigation(handlers);
  bindSetupActions(handlers);
  bindCatalogSearch(handlers);
  bindCatalogFilters(handlers);
  bindCatalogActions(selectedPackageSlug, handlers);
  bindInstallDialogs(handlers);
  bindLibraryActions(handlers);
  bindModelActions(handlers);
  bindDiagnosticsActions(handlers);
  bindOperationCancellation(handlers);
  bindOperationRetry(handlers);
  bindPackageSelection(handlers);
  activateViewIcons();
}

function bindWorkspaceNavigation(handlers: ViewEventHandlers) {
  document.querySelectorAll<HTMLButtonElement>("[data-workspace-section]").forEach((button) => {
    button.addEventListener("click", () => {
      const section = button.dataset.workspaceSection;
      if (isWorkspaceSection(section)) {
        void handlers.setWorkspaceSection(section);
      }
    });
  });
}

function bindSetupActions(handlers: ViewEventHandlers) {
  document
    .querySelector<HTMLButtonElement>("[data-setup-service-action]")
    ?.addEventListener("click", () => void handlers.ensureLocalService());
  document
    .querySelector<HTMLButtonElement>("[data-setup-sync-action]")
    ?.addEventListener("click", () => void handlers.syncRegistries());
  document
    .querySelector<HTMLButtonElement>("[data-setup-diagnostics-action]")
    ?.addEventListener("click", () => void handlers.setWorkspaceSection("diagnostics"));
  document
    .querySelector<HTMLButtonElement>("[data-setup-model-store-action]")
    ?.addEventListener("click", () => void handlers.initializeModelStore());
}

function bindCatalogSearch(handlers: ViewEventHandlers) {
  const input = document.querySelector<HTMLInputElement>("#catalog-search");
  input?.addEventListener("input", () => {
    const query = input.value;
    const selectionStart = input.selectionStart ?? query.length;
    const selectionEnd = input.selectionEnd ?? selectionStart;
    void Promise.resolve(handlers.setCatalogSearchQuery(query)).finally(() => {
      restoreCatalogSearchFocus(selectionStart, selectionEnd);
    });
  });

  if (catalogSearchShortcutBound) {
    return;
  }
  document.addEventListener("keydown", (event) => {
    if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "k") {
      return;
    }
    const currentInput = document.querySelector<HTMLInputElement>("#catalog-search");
    if (!currentInput) {
      return;
    }
    event.preventDefault();
    currentInput.focus();
    currentInput.select();
  });
  catalogSearchShortcutBound = true;
}

function restoreCatalogSearchFocus(selectionStart: number, selectionEnd: number) {
  requestAnimationFrame(() => {
    const input = document.querySelector<HTMLInputElement>("#catalog-search");
    if (!input) {
      return;
    }
    input.focus();
    const start = Math.min(selectionStart, input.value.length);
    const end = Math.min(selectionEnd, input.value.length);
    input.setSelectionRange(start, end);
  });
}

function bindCatalogFilters(handlers: ViewEventHandlers) {
  document
    .querySelectorAll<HTMLButtonElement>("[data-catalog-availability-filter]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const filter = button.dataset.catalogAvailabilityFilter;
        if (isCatalogAvailabilityFilter(filter)) {
          void handlers.setCatalogAvailabilityFilter(filter);
        }
      });
    });
  const typeFilter = document.querySelector<HTMLSelectElement>("#catalog-type-filter");
  typeFilter?.addEventListener("change", () => {
    void handlers.setCatalogProductTypeFilter(typeFilter.value || null);
  });
  document
    .querySelectorAll<HTMLButtonElement>("[data-catalog-access-filter]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const filter = button.dataset.catalogAccessFilter;
        if (isCatalogAccessFilter(filter)) {
          void handlers.setCatalogAccessFilter(filter);
        }
      });
    });
}

function bindCatalogActions(
  selectedPackageSlug: string | null,
  handlers: ViewEventHandlers,
) {
  document
    .querySelector<HTMLButtonElement>("#sync-button")
    ?.addEventListener("click", () => void handlers.syncRegistries());
  document
    .querySelector<HTMLButtonElement>("#service-button")
    ?.addEventListener("click", () => void handlers.ensureLocalService());
  document
    .querySelector<HTMLButtonElement>("#review-install")
    ?.addEventListener("click", () => {
      if (selectedPackageSlug) {
        void handlers.reviewInstall(selectedPackageSlug);
      }
    });
  document
    .querySelector<HTMLButtonElement>("#open-handoff")
    ?.addEventListener("click", () => {
      if (selectedPackageSlug) {
        void handlers.openInstallHandoff(selectedPackageSlug);
      }
    });
  document.querySelectorAll<HTMLButtonElement>("[data-install-scope]").forEach((button) => {
    button.addEventListener("click", () => {
      const scope = button.dataset.installScope;
      if (isInstallScope(scope)) {
        void handlers.setInstallScope(scope);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-install-format]").forEach((button) => {
    button.addEventListener("click", () => {
      const slug = button.dataset.installSlug;
      const format = button.dataset.installFormat;
      if (slug && format) {
        void handlers.chooseArchiveAndInstall(slug, format);
      }
    });
  });
  document
    .querySelectorAll<HTMLButtonElement>("[data-install-url-format]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const slug = button.dataset.installUrlSlug;
        const format = button.dataset.installUrlFormat;
        if (slug && format) {
          void handlers.requestUrlInstall(slug, format);
        }
      });
    });
}

function bindInstallDialogs(handlers: ViewEventHandlers) {
  document
    .querySelector<HTMLButtonElement>("#confirm-install-handoff")
    ?.addEventListener("click", () => void handlers.confirmInstallHandoff());
  document
    .querySelector<HTMLButtonElement>("#cancel-install-handoff")
    ?.addEventListener("click", () => void handlers.cancelInstallHandoff());
  document
    .querySelector<HTMLButtonElement>("#confirm-archive-install")
    ?.addEventListener(
      "click",
      () => void handlers.confirmArchiveInstall(selectedInstallScope("archive-install-scope")),
    );
  document
    .querySelector<HTMLButtonElement>("#cancel-archive-install")
    ?.addEventListener("click", () => void handlers.cancelArchiveInstall());
  document
    .querySelector<HTMLButtonElement>("#confirm-url-install")
    ?.addEventListener(
      "click",
      () => void handlers.confirmUrlInstall(selectedInstallScope("url-install-scope")),
    );
  document
    .querySelector<HTMLButtonElement>("#cancel-url-install")
    ?.addEventListener("click", () => void handlers.cancelUrlInstall());
}

function selectedInstallScope(id: string): InstallScope | undefined {
  const value = document.querySelector<HTMLSelectElement>(`#${id}`)?.value;
  return isInstallScope(value) ? value : undefined;
}

function bindLibraryActions(handlers: ViewEventHandlers) {
  document
    .querySelector<HTMLButtonElement>("[data-update-all-packages]")
    ?.addEventListener("click", () => void handlers.requestUpdateAllPackages());
  document.querySelectorAll<HTMLButtonElement>("[data-remove-slug]").forEach((button) => {
    button.addEventListener("click", () => {
      const slug = button.dataset.removeSlug;
      if (slug) {
        void handlers.requestRemovePackage(slug);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-update-slug]").forEach((button) => {
    button.addEventListener("click", () => {
      const slug = button.dataset.updateSlug;
      if (slug) {
        void handlers.requestUpdatePackage(slug);
      }
    });
  });
  document
    .querySelector<HTMLButtonElement>("#confirm-update-package")
    ?.addEventListener("click", () => void handlers.confirmUpdatePackage());
  document
    .querySelector<HTMLButtonElement>("#cancel-update-package")
    ?.addEventListener("click", () => void handlers.cancelUpdatePackage());
  document
    .querySelector<HTMLButtonElement>("#confirm-update-all-packages")
    ?.addEventListener("click", () => void handlers.confirmUpdateAllPackages());
  document
    .querySelector<HTMLButtonElement>("#cancel-update-all-packages")
    ?.addEventListener("click", () => void handlers.cancelUpdateAllPackages());
  document.querySelectorAll<HTMLInputElement>("[data-pin-slug]").forEach((input) => {
    input.addEventListener("change", () => {
      const slug = input.dataset.pinSlug;
      if (slug) {
        void handlers.setPackagePin(slug, input.checked);
      }
    });
  });
  document
    .querySelector<HTMLButtonElement>("#confirm-remove-package")
    ?.addEventListener("click", () => void handlers.confirmRemovePackage());
  document
    .querySelector<HTMLButtonElement>("#cancel-remove-package")
    ?.addEventListener("click", () => void handlers.cancelRemovePackage());
}

function bindModelActions(handlers: ViewEventHandlers) {
  const searchInput = document.querySelector<HTMLInputElement>("#model-search");
  searchInput?.addEventListener("input", () => {
    const query = searchInput.value;
    const selectionStart = searchInput.selectionStart ?? query.length;
    const selectionEnd = searchInput.selectionEnd ?? selectionStart;
    void Promise.resolve(handlers.setModelSearchQuery(query)).finally(() => {
      restoreModelSearchFocus(selectionStart, selectionEnd);
    });
  });
  document
    .querySelector<HTMLButtonElement>("[data-import-model-manifest]")
    ?.addEventListener("click", () => void handlers.importModelManifest());
  document
    .querySelector<HTMLButtonElement>("[data-initialize-model-store]")
    ?.addEventListener("click", () => void handlers.initializeModelStore());
  document.querySelectorAll<HTMLButtonElement>("[data-import-catalog-model-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.importCatalogModelName;
      const version = button.dataset.importCatalogModelVersion;
      if (name && version) {
        void handlers.importModelCatalogPackage(name, version);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-pull-model-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.pullModelName;
      const version = button.dataset.pullModelVersion;
      if (name && version) {
        void handlers.pullModelWeights(name, version);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-install-model-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.installModelName;
      const version = button.dataset.installModelVersion;
      if (name && version) {
        void handlers.installModelPackage(name, version);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-plan-model-run-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.planModelRunName;
      const version = button.dataset.planModelRunVersion;
      if (name && version) {
        void handlers.planModelRun(name, version);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-run-model-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.runModelName;
      const version = button.dataset.runModelVersion;
      if (name && version) {
        void handlers.runModel(name, version);
      }
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-add-model-chain-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.addModelChainName;
      const version = button.dataset.addModelChainVersion;
      const packageId = button.dataset.addModelChainPackageId;
      if (name && version && packageId) {
        void handlers.addModelChainStep(name, version, packageId);
      }
    });
  });
  document
    .querySelector<HTMLButtonElement>("[data-plan-model-chain]")
    ?.addEventListener("click", () => void handlers.planModelChain());
  document
    .querySelector<HTMLButtonElement>("[data-clear-model-chain]")
    ?.addEventListener("click", () => void handlers.clearModelChain());
  document
    .querySelectorAll<HTMLButtonElement>("[data-remove-model-chain-index]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const stepIndex = Number(button.dataset.removeModelChainIndex);
        if (Number.isInteger(stepIndex)) {
          void handlers.removeModelChainStep(stepIndex);
        }
      });
    });
  document.querySelectorAll<HTMLButtonElement>("[data-remove-model-name]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.removeModelName;
      const version = button.dataset.removeModelVersion;
      if (name && version) {
        void handlers.removeModelPackage(name, version);
      }
    });
  });
}

function restoreModelSearchFocus(selectionStart: number, selectionEnd: number) {
  requestAnimationFrame(() => {
    const input = document.querySelector<HTMLInputElement>("#model-search");
    if (!input) {
      return;
    }
    input.focus();
    const start = Math.min(selectionStart, input.value.length);
    const end = Math.min(selectionEnd, input.value.length);
    input.setSelectionRange(start, end);
  });
}

function bindDiagnosticsActions(handlers: ViewEventHandlers) {
  document
    .querySelector<HTMLButtonElement>("[data-refresh-diagnostics-action]")
    ?.addEventListener("click", () => void handlers.refreshDiagnostics());
  document
    .querySelector<HTMLButtonElement>("[data-scan-library-action]")
    ?.addEventListener("click", () => void handlers.scanLibrary());
}

function bindOperationCancellation(handlers: ViewEventHandlers) {
  document
    .querySelectorAll<HTMLButtonElement>("[data-cancel-operation-scope]")
    .forEach((button) => {
      button.addEventListener("click", () => {
        const scope = button.dataset.cancelOperationScope;
        if (isOperationScope(scope)) {
          void handlers.cancelActiveOperation(scope);
        }
      });
    });
}

function bindOperationRetry(handlers: ViewEventHandlers) {
  document
    .querySelector<HTMLButtonElement>("[data-retry-recovery]")
    ?.addEventListener("click", () => void handlers.retryRecoveryOperations());
  document.querySelectorAll<HTMLButtonElement>("[data-retry-operation-id]").forEach((button) => {
    button.addEventListener("click", () => {
      const operationId = button.dataset.retryOperationId;
      if (operationId) {
        void handlers.retryOperation(operationId);
      }
    });
  });
}

function bindPackageSelection(handlers: ViewEventHandlers) {
  document.querySelectorAll<HTMLTableRowElement>("[data-package-slug]").forEach((row) => {
    row.addEventListener("click", () => {
      const slug = row.dataset.packageSlug;
      if (slug) {
        void handlers.selectPackage(slug);
      }
    });
    row.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") {
        return;
      }
      event.preventDefault();
      const slug = row.dataset.packageSlug;
      if (slug) {
        void handlers.selectPackage(slug);
      }
    });
  });
}

function activateViewIcons() {
  createIcons({
    icons: {
      Activity,
      ArrowRight,
      Download,
      ExternalLink,
      FilePlus,
      FolderOpen,
      GitBranch,
      HardDrive,
      Info,
      ListPlus,
      Package,
      Play,
      RefreshCw,
      Search,
      ShieldCheck,
      Terminal,
      Trash2,
      X,
    },
  });
}

function isOperationScope(value: string | undefined): value is OperationScope {
  return (
    value === "sync" ||
    value === "lifecycle" ||
    value === "library" ||
    value === "model"
  );
}

function isWorkspaceSection(value: string | undefined): value is WorkspaceSection {
  return (
    value === "catalog" ||
    value === "library" ||
    value === "diagnostics" ||
    value === "runtime"
  );
}

function isCatalogAvailabilityFilter(
  value: string | undefined,
): value is CatalogAvailabilityFilter {
  return value === "all" || value === "installed" || value === "available";
}

function isCatalogAccessFilter(value: string | undefined): value is CatalogAccessFilter {
  return value === "all" || value === "free" || value === "paid";
}

function isInstallScope(value: string | undefined): value is InstallScope {
  return value === "user" || value === "system";
}
