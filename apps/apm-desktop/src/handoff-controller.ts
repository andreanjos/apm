import { openInstallHandoffCommand } from "./commands";
import { installHandoffCandidate } from "./operation-candidates";
import { installPlanStatusLabel } from "./install-plan-labels";
import type { InstallPlanResult } from "./types";
import type {
  InstallHandoffCandidate,
  LifecycleNotice,
} from "./view-model";

export type HandoffControllerState = {
  pendingInstallHandoff: InstallHandoffCandidate | null;
};

type HandoffControllerHost = {
  installPlan(): InstallPlanResult | null;
  setInstallPlan(plan: InstallPlanResult | null): void;
  setLifecycleNotice(notice: LifecycleNotice | null): void;
  lifecycleOperationActive(): boolean;
  clearPeerInstallDialogs(): void;
  formatError(error: unknown): string;
  render(): void;
};

export function createHandoffController(host: HandoffControllerHost) {
  let pendingInstallHandoff: InstallHandoffCandidate | null = null;

  function state(): HandoffControllerState {
    return { pendingInstallHandoff };
  }

  function clearPending() {
    pendingInstallHandoff = null;
  }

  function lifecycleActionLocked() {
    return host.lifecycleOperationActive();
  }

  function openInstallHandoff(slug: string) {
    if (lifecycleActionLocked()) {
      return;
    }

    pendingInstallHandoff = null;
    try {
      pendingInstallHandoff = installHandoffCandidate(host.installPlan(), slug);
      host.clearPeerInstallDialogs();
      host.setLifecycleNotice(null);
    } catch (error) {
      host.setLifecycleNotice({ tone: "error", message: host.formatError(error) });
    }
    host.render();
  }

  async function confirmInstallHandoff() {
    if (lifecycleActionLocked()) {
      return;
    }

    const candidate = pendingInstallHandoff;
    if (!candidate) {
      return;
    }

    pendingInstallHandoff = null;
    host.setLifecycleNotice({ tone: "info", message: "Opening install handoff" });
    host.render();

    try {
      applyInstallHandoffResult(await openInstallHandoffCommand(candidate.slug));
    } catch (error) {
      host.setLifecycleNotice({ tone: "error", message: host.formatError(error) });
    }
    host.render();
  }

  function cancelInstallHandoff() {
    if (lifecycleActionLocked()) {
      return;
    }

    pendingInstallHandoff = null;
    host.setLifecycleNotice({ tone: "info", message: "Install handoff canceled" });
    host.render();
  }

  function applyInstallHandoffResult(
    result: Awaited<ReturnType<typeof openInstallHandoffCommand>>,
  ) {
    if (result.status === "open") {
      host.setInstallPlan({ status: "plan", plan: result.plan });
      host.setLifecycleNotice({ tone: "info", message: result.handoff.message });
    } else if (result.status === "no_handoff") {
      host.setInstallPlan({ status: "plan", plan: result.plan });
      host.setLifecycleNotice({ tone: "info", message: result.reason });
    } else {
      host.setInstallPlan(result.plan);
      host.setLifecycleNotice({
        tone: "info",
        message: installPlanStatusLabel(result.plan),
      });
    }
  }

  return {
    cancelInstallHandoff,
    clearPending,
    confirmInstallHandoff,
    openInstallHandoff,
    state,
  };
}
