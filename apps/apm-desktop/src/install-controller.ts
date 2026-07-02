import {
  chooseArchivePathCommand,
  installFromArchiveCommand,
  installFromUrlCommand,
  planInstallCommand,
} from "./commands";
import { installPlanStatusLabel } from "./install-plan-labels";
import { archiveInstallCandidate, urlInstallCandidate } from "./operation-candidates";
import { withPreviewInstall } from "./preview-state";
import type {
  DesktopInstallResult,
  DesktopSnapshot,
  InstallEvent,
  InstallPlanResult,
  InstallScope,
} from "./types";
import { formatLabel } from "./view-utils";
import type {
  ArchiveInstallCandidate,
  LifecycleNotice,
  UrlInstallCandidate,
} from "./view-model";

export type InstallControllerState = {
  installPlan: InstallPlanResult | null;
  installScope: InstallScope;
  installStatus: string;
  lifecycleNotice: LifecycleNotice | null;
  pendingArchiveInstall: ArchiveInstallCandidate | null;
  pendingUrlInstall: UrlInstallCandidate | null;
  lifecycleEvents: InstallEvent[];
};

type InstallControllerHost = {
  snapshot(): DesktopSnapshot;
  setSnapshot(snapshot: DesktopSnapshot): void;
  isTauriRuntime(): boolean;
  lifecycleOperationActive(): boolean;
  reloadSnapshot(): Promise<void>;
  runInstallOperation<T>(run: (progressId?: string) => Promise<T>): Promise<T>;
  startInstallOperation(): void;
  clearInstallOperation(): void;
  reportInstallError(error: unknown): Promise<void>;
  clearPeerInstallDialogs(): void;
  formatError(error: unknown): string;
  render(): void;
};

type RunnableInstallCandidate = {
  format: string;
};

const defaultInstallStatus = "No install plan loaded";
const defaultInstallScope: InstallScope = "user";

export function createInstallController(host: InstallControllerHost) {
  let installPlan: InstallPlanResult | null = null;
  let installScope = defaultInstallScope;
  let installStatus = defaultInstallStatus;
  let lifecycleNotice: LifecycleNotice | null = null;
  let pendingArchiveInstall: ArchiveInstallCandidate | null = null;
  let pendingUrlInstall: UrlInstallCandidate | null = null;
  let lifecycleEvents: InstallEvent[] = [];

  function state(): InstallControllerState {
    return {
      installPlan,
      installScope,
      installStatus,
      lifecycleNotice,
      pendingArchiveInstall,
      pendingUrlInstall,
      lifecycleEvents,
    };
  }

  function currentInstallPlan() {
    return installPlan;
  }

  function setInstallPlan(plan: InstallPlanResult | null) {
    installPlan = plan;
  }

  function setLifecycleNotice(notice: LifecycleNotice | null) {
    lifecycleNotice = notice;
  }

  function appendLifecycleEvent(event: InstallEvent) {
    lifecycleEvents = [...lifecycleEvents, event];
  }

  function clearPending() {
    pendingArchiveInstall = null;
    pendingUrlInstall = null;
  }

  function clearForPackageSelection() {
    installPlan = null;
    installScope = defaultInstallScope;
    installStatus = defaultInstallStatus;
    lifecycleNotice = null;
    clearPending();
    lifecycleEvents = [];
  }

  function clearForRemovedPackage(slug: string, selectedSlug: string | null) {
    if (selectedSlug !== slug) {
      return;
    }
    installPlan = null;
    installScope = defaultInstallScope;
    installStatus = defaultInstallStatus;
    lifecycleNotice = { tone: "info", message: `${slug} was removed.` };
  }

  function lifecycleActionLocked() {
    return host.lifecycleOperationActive();
  }

  async function reviewInstall(slug: string) {
    if (lifecycleActionLocked()) {
      return;
    }

    installPlan = null;
    installStatus = "Reviewing install";
    lifecycleNotice = null;
    clearPending();
    host.clearPeerInstallDialogs();
    lifecycleEvents = [];
    host.render();

    try {
      installPlan = await planInstallCommand(slug, installScope);
      installStatus = installPlanStatusLabel(installPlan);
    } catch (error) {
      installPlan = null;
      installStatus = host.formatError(error);
    }
    host.render();
  }

  async function setInstallScope(scope: InstallScope) {
    if (lifecycleActionLocked() || installScope === scope) {
      return;
    }

    installScope = scope;
    clearPending();
    host.clearPeerInstallDialogs();

    if (!installPlan || installPlan.status !== "plan") {
      host.render();
      return;
    }

    const { slug } = installPlan.plan;
    installStatus = "Reviewing install";
    lifecycleNotice = null;
    lifecycleEvents = [];
    host.render();

    try {
      installPlan = await planInstallCommand(slug, installScope);
      installStatus = installPlanStatusLabel(installPlan);
    } catch (error) {
      installPlan = null;
      installStatus = host.formatError(error);
    }
    host.render();
  }

  function requestUrlInstall(slug: string, format: string) {
    if (lifecycleActionLocked()) {
      return;
    }

    try {
      pendingUrlInstall = urlInstallCandidate(installPlan, slug, format);
      pendingArchiveInstall = null;
      host.clearPeerInstallDialogs();
      lifecycleNotice = null;
      lifecycleEvents = [];
    } catch (error) {
      lifecycleNotice = { tone: "error", message: host.formatError(error) };
    }
    host.render();
  }

  function cancelUrlInstall() {
    if (lifecycleActionLocked()) {
      return;
    }

    pendingUrlInstall = null;
    lifecycleNotice = { tone: "info", message: "Download install canceled" };
    lifecycleEvents = [];
    host.render();
  }

  async function confirmUrlInstall(scope?: InstallScope) {
    if (lifecycleActionLocked()) {
      return;
    }

    const candidate = pendingUrlInstall;
    if (!candidate) {
      return;
    }

    const scopedCandidate = { ...candidate, installScope: scope ?? candidate.installScope };
    pendingUrlInstall = null;
    await runInstallCandidate(
      scopedCandidate,
      {
        tone: "info",
        message: `Downloading ${formatLabel(scopedCandidate.format)} for ${scopedCandidate.name}`,
      },
      (progressId) => installFromUrlCommand(scopedCandidate, progressId),
    );
  }

  async function chooseArchiveAndInstall(slug: string, format: string) {
    if (lifecycleActionLocked()) {
      return;
    }

    const selectedFormat =
      installPlan?.status === "plan"
        ? installPlan.plan.formats.find(
            (candidate) => candidate.format.toLowerCase() === format.toLowerCase(),
          )
        : null;
    const archiveType = selectedFormat?.install_type ?? "zip";
    lifecycleNotice = {
      tone: "info",
      message: `Choose ${formatLabel(format)} ${archiveType.toUpperCase()} archive`,
    };
    host.render();

    try {
      const archivePath = await chooseArchivePathCommand(archiveType);
      if (!archivePath) {
        lifecycleNotice = { tone: "info", message: "Archive selection canceled" };
        host.render();
        return;
      }

      pendingArchiveInstall = archiveInstallCandidate(
        installPlan,
        slug,
        format,
        archivePath,
      );
      pendingUrlInstall = null;
      host.clearPeerInstallDialogs();
      lifecycleNotice = null;
      host.render();
    } catch (error) {
      lifecycleNotice = { tone: "error", message: host.formatError(error) };
      host.render();
    }
  }

  async function confirmArchiveInstall(scope?: InstallScope) {
    if (lifecycleActionLocked()) {
      return;
    }

    const candidate = pendingArchiveInstall;
    if (!candidate) {
      return;
    }

    const scopedCandidate = { ...candidate, installScope: scope ?? candidate.installScope };
    pendingArchiveInstall = null;
    await runInstallCandidate(
      scopedCandidate,
      {
        tone: "info",
        message: `Installing ${formatLabel(scopedCandidate.format)} from ${scopedCandidate.archiveName}`,
      },
      (progressId) => installFromArchiveCommand(scopedCandidate, progressId),
    );
  }

  async function runInstallCandidate(
    candidate: RunnableInstallCandidate,
    notice: LifecycleNotice,
    install: (progressId?: string) => Promise<DesktopInstallResult>,
  ) {
    host.startInstallOperation();
    lifecycleNotice = notice;
    host.render();
    try {
      const installResult = await host.runInstallOperation(install);
      lifecycleEvents = installResult.events;
      if (installResult.status === "failed") {
        await host.reportInstallError(installResult.error);
        host.render();
        return;
      }
      await handleInstallResult(installResult, candidate.format);
    } catch (error) {
      await host.reportInstallError(error);
    } finally {
      host.clearInstallOperation();
      host.render();
    }
  }

  function cancelArchiveInstall() {
    if (lifecycleActionLocked()) {
      return;
    }

    pendingArchiveInstall = null;
    lifecycleNotice = { tone: "info", message: "Archive install canceled" };
    lifecycleEvents = [];
    host.render();
  }

  async function handleInstallResult(
    installResult: Extract<DesktopInstallResult, { status: "completed" }>,
    format: string,
  ) {
    const { result } = installResult;
    switch (result.status) {
      case "installed":
        installPlan = null;
        installStatus = `Installed ${formatLabel(format)}`;
        lifecycleNotice = {
          tone: "success",
          message: `${result.package.slug} ${formatLabel(format)} installed and recorded.`,
        };
        if (host.isTauriRuntime()) {
          await host.reloadSnapshot();
        } else {
          host.setSnapshot(withPreviewInstall(host.snapshot(), result.package));
          host.render();
        }
        return;
      case "plan_unavailable":
        installPlan = result.plan;
        lifecycleNotice = {
          tone: "error",
          message: installPlanStatusLabel(result.plan),
        };
        break;
      case "already_installed":
        installPlan = { status: "plan", plan: result.plan };
        lifecycleNotice = {
          tone: "info",
          message: `${result.plan.name} is already installed.`,
        };
        break;
      case "external_handoff_required":
      case "format_required":
      case "archive_required":
      case "unsupported_install_type":
        installPlan = { status: "plan", plan: result.plan };
        lifecycleNotice = { tone: "error", message: result.reason };
        break;
    }
    host.render();
  }

  return {
    appendLifecycleEvent,
    cancelArchiveInstall,
    cancelUrlInstall,
    chooseArchiveAndInstall,
    clearForPackageSelection,
    clearForRemovedPackage,
    clearPending,
    confirmArchiveInstall,
    confirmUrlInstall,
    currentInstallPlan,
    requestUrlInstall,
    reviewInstall,
    setInstallScope,
    setInstallPlan,
    setLifecycleNotice,
    state,
  };
}
