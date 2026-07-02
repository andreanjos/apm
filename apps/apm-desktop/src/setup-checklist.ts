import type {
  DesktopServiceSession,
  DesktopSnapshot,
} from "./types";
import type { OperationControlState } from "./view-model";
import { escapeHtml } from "./view-utils";

type SetupTone = "good" | "warn" | "bad" | "info";
type SetupAction = "service" | "sync" | "diagnostics" | "model-store";

const MODEL_STORE_CHECK = "Model store";
const MODEL_ACTION_LOCKED_LABEL = "Model action running";

type SetupItem = {
  key: string;
  label: string;
  value: string;
  detail: string;
  tone: SetupTone;
  action?: SetupAction;
};

type SetupChecklistOptions = {
  modelStoreInitializing?: boolean;
  modelActionLocked?: boolean;
};

type SetupActionContext = {
  serviceReady: boolean;
  syncOperation: OperationControlState | null;
  modelStoreInitializing: boolean;
  modelActionLocked: boolean;
};

type SetupButtonState = {
  label: string;
  disabled: boolean;
};

export function setupChecklistSection(
  snapshot: DesktopSnapshot,
  service: DesktopServiceSession,
  syncOperation: OperationControlState | null,
  options: SetupChecklistOptions = {},
) {
  const {
    modelStoreInitializing = false,
    modelActionLocked = false,
  } = options;
  const items = setupItems(snapshot, service);
  const actionContext: SetupActionContext = {
    serviceReady: isServiceReady(service),
    syncOperation,
    modelStoreInitializing,
    modelActionLocked,
  };
  if (items.every((item) => item.tone === "good")) {
    return "";
  }

  return `
    <section class="panel setup-panel" aria-label="Setup status">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Setup</p>
          <h2>Readiness</h2>
        </div>
        <span class="status-pill">${escapeHtml(setupStatusLabel(items))}</span>
      </div>
      <div class="setup-list">
        ${items
          .map((item) => setupItemMarkup(item, actionContext))
          .join("")}
      </div>
    </section>
  `;
}

function setupItems(
  snapshot: DesktopSnapshot,
  service: DesktopServiceSession,
): SetupItem[] {
  return [
    serviceItem(service),
    catalogItem(snapshot),
    diagnosticsItem(snapshot),
    modelStoreItem(snapshot),
  ];
}

function serviceItem(service: DesktopServiceSession): SetupItem {
  if (isServiceReady(service)) {
    return {
      key: "service",
      label: "Local service",
      value: service.status === "started" ? "Started" : "Reused",
      detail: `${service.api_version} / ${service.schema_version}`,
      tone: "good",
    };
  }

  if (isLiveService(service)) {
    return {
      key: "service",
      label: "Local service",
      value: "Token missing",
      detail: service.token_file,
      tone: "bad",
      action: "service",
    };
  }

  if (service.status === "preview") {
    return {
      key: "service",
      label: "Local service",
      value: "Preview",
      detail: "Sample data",
      tone: "info",
      action: "service",
    };
  }

  return {
    key: "service",
    label: "Local service",
    value: service.status === "not_started" ? "Stopped" : "Unavailable",
    detail: service.message,
    tone: service.status === "not_started" ? "warn" : "bad",
    action: "service",
  };
}

function catalogItem(snapshot: DesktopSnapshot): SetupItem {
  if (snapshot.catalog.status === "matches" && snapshot.catalog.total_matches > 0) {
    return {
      key: "catalog",
      label: "Catalog",
      value: `${snapshot.catalog.total_matches.toLocaleString()} packages`,
      detail: `${snapshot.source_count} ${snapshot.source_count === 1 ? "source" : "sources"}`,
      tone: "good",
    };
  }

  return {
    key: "catalog",
    label: "Catalog",
    value: "Empty",
    detail: `${snapshot.source_count} ${snapshot.source_count === 1 ? "source" : "sources"}`,
    tone: "warn",
    action: "sync",
  };
}

function diagnosticsItem(snapshot: DesktopSnapshot): SetupItem {
  const { summary } = snapshot.diagnostics;
  if (summary.failures > 0) {
    return {
      key: "diagnostics",
      label: "Diagnostics",
      value: `${summary.failures} failed`,
      detail: `${summary.warnings} warnings`,
      tone: "bad",
      action: "diagnostics",
    };
  }

  if (summary.warnings > 0) {
    return {
      key: "diagnostics",
      label: "Diagnostics",
      value: `${summary.warnings} warnings`,
      detail: `${summary.ok} checks ok`,
      tone: "warn",
      action: "diagnostics",
    };
  }

  return {
    key: "diagnostics",
    label: "Diagnostics",
    value: "Ready",
    detail: `${summary.ok} checks ok`,
    tone: "good",
  };
}

function modelStoreItem(snapshot: DesktopSnapshot): SetupItem {
  const check = snapshot.diagnostics.checks.find(
    (candidate) => candidate.name === MODEL_STORE_CHECK,
  );

  if (!check) {
    return {
      key: "model-store",
      label: "Model store",
      value: "Unchecked",
      detail: snapshot.model_store.root,
      tone: "info",
      action: "model-store",
    };
  }

  if (check.status === "ok") {
    return {
      key: "model-store",
      label: "Model store",
      value: "Ready",
      detail: check.detail,
      tone: "good",
    };
  }

  if (check.status === "warning") {
    return {
      key: "model-store",
      label: "Model store",
      value: "Initialize",
      detail: check.detail,
      tone: "warn",
      action: "model-store",
    };
  }

  return {
    key: "model-store",
    label: "Model store",
    value: "Blocked",
    detail: check.detail,
    tone: "bad",
  };
}

function setupItemMarkup(
  item: SetupItem,
  actionContext: SetupActionContext,
) {
  return `
    <article class="setup-item ${item.tone}" data-setup-item="${escapeHtml(item.key)}">
      <div class="setup-marker" aria-hidden="true"></div>
      <div class="setup-body">
        <span>${escapeHtml(item.label)}</span>
        <strong>${escapeHtml(item.value)}</strong>
        <small>${escapeHtml(item.detail)}</small>
      </div>
      ${setupActionMarkup(item.action, actionContext)}
    </article>
  `;
}

function setupActionMarkup(
  action: SetupAction | undefined,
  context: SetupActionContext,
) {
  switch (action) {
    case "service":
      return `
        <button class="icon-button setup-action" data-setup-service-action type="button" aria-label="Start local service" title="Start local service">
          <i data-lucide="terminal" aria-hidden="true"></i>
        </button>
      `;
    case "sync": {
      const state = syncActionState(context);
      return `
        <button class="icon-button setup-action" data-setup-sync-action type="button" aria-label="${state.label}" title="${state.label}" ${state.disabled ? "disabled" : ""}>
          <i data-lucide="refresh-cw" aria-hidden="true"></i>
        </button>
      `;
    }
    case "diagnostics":
      return `
        <button class="icon-button setup-action" data-setup-diagnostics-action type="button" aria-label="Open diagnostics" title="Open diagnostics">
          <i data-lucide="activity" aria-hidden="true"></i>
        </button>
      `;
    case "model-store": {
      const state = modelStoreActionState(context);
      return `
        <button class="icon-button setup-action" data-setup-model-store-action type="button" aria-label="${state.label}" title="${state.label}" ${state.disabled ? "disabled" : ""}>
          <i data-lucide="hard-drive" aria-hidden="true"></i>
        </button>
      `;
    }
    case undefined:
      return "";
  }
}

function syncActionState(context: SetupActionContext): SetupButtonState {
  if (context.syncOperation !== null) {
    return { label: "Registry sync running", disabled: true };
  }
  if (!context.serviceReady) {
    return { label: "Start local service first", disabled: true };
  }
  return { label: "Sync registries", disabled: false };
}

function modelStoreActionState(context: SetupActionContext): SetupButtonState {
  if (context.modelStoreInitializing) {
    return { label: "Initializing model store", disabled: true };
  }
  if (context.modelActionLocked) {
    return { label: MODEL_ACTION_LOCKED_LABEL, disabled: true };
  }
  if (!context.serviceReady) {
    return { label: "Start local service first", disabled: true };
  }
  return { label: "Initialize model store", disabled: false };
}

function isServiceReady(service: DesktopServiceSession) {
  return isLiveService(service) && service.token_available;
}

function isLiveService(service: DesktopServiceSession) {
  return service.status === "started" || service.status === "reused";
}

function setupStatusLabel(items: SetupItem[]) {
  if (items.some((item) => item.tone === "bad")) {
    return "Action needed";
  }
  if (items.some((item) => item.tone === "warn")) {
    return "Check setup";
  }
  return "Preview";
}
