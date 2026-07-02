import type { LifecycleNotice } from "./view-model";

export type DiagnosticsControllerState = {
  diagnosticsNotice: LifecycleNotice | null;
  diagnosticsRefreshing: boolean;
};

type DiagnosticsControllerHost = {
  refreshSnapshotData(): Promise<void>;
  formatError(error: unknown): string;
  render(): void;
};

export function createDiagnosticsController(host: DiagnosticsControllerHost) {
  let diagnosticsNotice: LifecycleNotice | null = null;
  let diagnosticsRefreshing = false;

  function state(): DiagnosticsControllerState {
    return {
      diagnosticsNotice,
      diagnosticsRefreshing,
    };
  }

  async function refreshDiagnostics() {
    if (diagnosticsRefreshing) {
      return;
    }

    diagnosticsRefreshing = true;
    diagnosticsNotice = { tone: "info", message: "Running doctor checks" };
    host.render();

    try {
      await host.refreshSnapshotData();
      diagnosticsNotice = { tone: "success", message: "Doctor checks refreshed" };
    } catch (error) {
      diagnosticsNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      diagnosticsRefreshing = false;
      host.render();
    }
  }

  return {
    refreshDiagnostics,
    state,
  };
}
