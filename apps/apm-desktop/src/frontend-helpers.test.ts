import {
  archiveInstallCandidate,
  installHandoffCandidate,
  urlInstallCandidate,
} from "./operation-candidates";
import { archiveConfirmDialog, urlConfirmDialog } from "./view-dialogs";
import { fallbackPlanModelChain, fallbackSnapshot } from "./fallback";
import { createDiagnosticsController } from "./diagnostics-controller";
import { createLibraryController } from "./library-controller";
import { createModelController } from "./model-controller";
import { modelProgressLabel, operationProgressScope } from "./operation-events";
import { createOperationController } from "./operation-controller";
import { recoveryRetryActionLabel } from "./recovery-actions";
import {
  retryProgressMessage,
  retryRecoveryStatusMessage,
  retryStatusMessage,
} from "./retry-status";
import type {
  InstallPlanResult,
  DesktopSnapshot,
  OperationRecoverySummary,
  OperationStatus,
  PackageInstallPlan,
} from "./types";
import type {
  InstallEvent,
  LifecycleEvent,
  ModelOperationEvent,
} from "./types";
import type { LifecycleNotice } from "./view-model";

const tests: Array<[string, () => void | Promise<void>]> = [];

test("builds privileged installer handoff candidates", () => {
  const candidate = installHandoffCandidate(
    installPlan({
      status: "privileged_installer_required",
      message: "This package installs shared system components.",
      formats: [
        installFormat({
          format: "pkg",
          install_type: "pkg",
          source: "https://example.test/massive.pkg",
        }),
      ],
    }),
    "massive",
  );

  assertEqual(candidate.actionLabel, "Open PKG", "privileged handoff action label");
  assertEqual(candidate.statusLabel, "Privileged PKG handoff", "privileged handoff status");
  assertEqual(candidate.target, "https://example.test/massive.pkg", "privileged handoff target");
  assertEqual(candidate.privileged, true, "privileged handoff flag");
});

test("prefers installed vendor app paths for vendor handoffs", () => {
  const candidate = installHandoffCandidate(
    installPlan({
      status: "vendor_installer_available",
      installer: {
        key: "native-access",
        name: "Native Access",
        download_url: "https://example.test/native-access.dmg",
        homepage: "https://example.test/native-access",
        installed_app_path: "/Applications/Native Access.app",
      },
    }),
    "massive",
  );

  assertEqual(candidate.actionLabel, "Open vendor app", "vendor handoff action label");
  assertEqual(candidate.target, "/Applications/Native Access.app", "vendor handoff target");
  assertEqual(candidate.privileged, false, "vendor handoff flag");
});

test("rejects handoff candidates without a target", () => {
  assertThrows(
    () =>
      installHandoffCandidate(
        installPlan({
          status: "ready",
          formats: [installFormat({ format: "vst3", source: "" })],
          installer: null,
        }),
        "massive",
      ),
    "Massive does not list a handoff target.",
  );
});

test("builds direct URL install candidates with checksum copy", () => {
  const candidate = urlInstallCandidate(
    installPlan({
      destination: "/Library/Audio/Plug-Ins/VST3",
      formats: [
        installFormat({
          format: "vst3",
          source: "https://example.test/massive.vst3.zip",
          has_checksum: true,
        }),
      ],
    }),
    "massive",
    "VST3",
  );

  assertEqual(candidate.format, "VST3", "url install format preserves requested label");
  assertEqual(candidate.destination, "/Library/Audio/Plug-Ins/VST3", "url install destination");
  assertEqual(candidate.installScope, "system", "url install scope follows plan scope");
  assertEqual(candidate.checksum, "Registry checksum will be verified", "url install checksum");

  const dialog = urlConfirmDialog(candidate);
  assertIncludes(dialog, 'id="url-install-scope"', "url install scope selector");
  assertIncludes(
    dialog,
    '<option value="system" selected>',
    "url install system scope selected",
  );
});

test("builds archive install candidates from local paths", () => {
  const candidate = archiveInstallCandidate(
    installPlan({
      destination: null,
      scope: "user",
      formats: [
        installFormat({
          format: "au",
          install_type: "component",
          source: "",
          has_checksum: false,
        }),
      ],
    }),
    "massive",
    "AU",
    "/Users/apm/Downloads/Massive.component.zip",
  );

  assertEqual(candidate.installType, "component", "archive install type");
  assertEqual(candidate.destination, "Format-specific destination", "archive fallback destination");
  assertEqual(candidate.installScope, "user", "archive install scope follows plan scope");
  assertEqual(candidate.archiveName, "Massive.component.zip", "archive file name");
  assertEqual(candidate.checksum, "No registry checksum listed", "archive checksum");

  const dialog = archiveConfirmDialog(candidate);
  assertIncludes(dialog, 'id="archive-install-scope"', "archive install scope selector");
  assertIncludes(
    dialog,
    "User library - ~/Library/Audio/Plug-Ins/Components/",
    "archive user destination option",
  );
  assertIncludes(
    dialog,
    "System library - /Library/Audio/Plug-Ins/Components/",
    "archive system destination option",
  );
});

test("labels retryable recovery counts", () => {
  assertEqual(
    recoveryRetryActionLabel(recovery({ retryable_count: 0 })),
    null,
    "zero recovery retries",
  );
  assertEqual(
    recoveryRetryActionLabel(recovery({ retryable_count: 1 })),
    "Retry ready",
    "single recovery retry",
  );
  assertEqual(
    recoveryRetryActionLabel(recovery({ retryable_count: 3 })),
    "Retry 3 ready",
    "multiple recovery retries",
  );
});

test("summarizes recovery retry status with failure priority", () => {
  assertEqual(
    retryRecoveryStatusMessage([]),
    "No retryable recovery operations.",
    "empty recovery status",
  );
  assertEqual(
    retryRecoveryStatusMessage([
      operationStatus({ state: "succeeded", kind: "install_url" }),
      operationStatus({ state: "failed", kind: "package_remove", error: "state db locked" }),
    ]),
    "state db locked",
    "single failed recovery status",
  );
  assertEqual(
    retryRecoveryStatusMessage([
      operationStatus({ state: "failed", kind: "install_url" }),
      operationStatus({ state: "failed", kind: "package_remove" }),
    ]),
    "2 recovery retries failed.",
    "multiple failed recovery status",
  );
  assertEqual(
    retryRecoveryStatusMessage([
      operationStatus({ state: "succeeded", kind: "install_url" }),
      operationStatus({ state: "running", kind: "package_update" }),
    ]),
    "1 recovery retry still running.",
    "incomplete recovery status",
  );
  assertEqual(
    retryRecoveryStatusMessage([
      operationStatus({ state: "succeeded", kind: "install_url" }),
      operationStatus({ state: "succeeded", kind: "package_remove" }),
    ]),
    "2 recovery retries completed.",
    "completed recovery status",
  );
});

test("labels individual retry statuses and events", () => {
  assertEqual(
    retryStatusMessage(operationStatus({ state: "succeeded", kind: "package_update" })),
    "Retry completed: package update",
    "successful retry status",
  );
  assertEqual(
    retryStatusMessage(operationStatus({ state: "canceled", kind: "registry_sync" })),
    "Retry canceled: registry sync",
    "canceled retry status",
  );
  assertEqual(
    retryProgressMessage({ event: "remove_started", slug: "massive", version: "1.0.0", format_count: 1 }),
    "Retrying remove for massive",
    "remove retry progress",
  );
  assertEqual(
    retryProgressMessage({ event: "registry_sync_finished", source_count: 1, failed_count: 0 }),
    "Retrying registry operation",
    "registry retry progress",
  );
  assertEqual(
    retryStatusMessage(operationStatus({ state: "succeeded", kind: "library_scan" })),
    "Retry completed: library scan",
    "scan retry status",
  );
  assertEqual(
    retryProgressMessage({
      event: "scan_finished",
      scanned_count: 2,
      matched_count: 1,
      adopted_count: 1,
    }),
    "Retrying library scan",
    "scan retry progress",
  );
});

test("routes retry progress by accepted operation kind", () => {
  assertEqual(
    operationProgressScope({
      operationId: "op-update",
      kind: "package_update",
      event: {
        event: "install_started",
        slug: "surge-xt",
        version: "1.3.4",
        format_count: 1,
      },
    }),
    "library",
    "package update retry install events stay in library lane",
  );
  assertEqual(
    operationProgressScope({
      operationId: "op-install",
      kind: "install_url",
      event: {
        event: "install_started",
        slug: "surge-xt",
        version: "1.3.4",
        format_count: 1,
      },
    }),
    "lifecycle",
    "install retry install events stay in lifecycle lane",
  );
  assertEqual(
    operationProgressScope({
      operationId: "op-sync",
      kind: "registry_sync",
      event: { event: "registry_sync_started", source_count: 1 },
    }),
    "sync",
    "registry retry events stay in sync lane",
  );
  assertEqual(
    operationProgressScope({
      operationId: "op-scan",
      kind: "library_scan",
      event: { event: "scan_started" },
    }),
    "library",
    "scan retry events stay in library lane",
  );
  assertEqual(
    operationProgressScope({
      operationId: "op-sync",
      kind: "registry_sync",
      event: {
        event: "install_started",
        slug: "surge-xt",
        version: "1.3.4",
        format_count: 1,
      },
    }),
    null,
    "mismatched kind and event family are ignored",
  );
  assertEqual(
    operationProgressScope({
      operationId: "op-model",
      kind: "model_install",
      event: { event: "model_install_started", package_id: "demucs@4.0.1" },
    }),
    "model",
    "model operation events stay in model lane",
  );
  assertEqual(
    operationProgressScope({
      operationId: "op-model-run",
      kind: "model_run",
      event: { event: "model_run_started", package_id: "demucs@4.0.1" },
    }),
    "model",
    "model run operation events stay in model lane",
  );
});

test("labels blocked model run events", () => {
  assertEqual(
    modelProgressLabel({
      event: "model_run_blocked",
      package_id: "demucs@4.0.1",
      blocker: "adapter_runner_unavailable",
      message: "native-mlx execution is not implemented yet",
    }),
    "demucs@4.0.1 run blocked: native-mlx execution is not implemented yet",
    "blocked model run event label",
  );
});

test("labels completed model run events", () => {
  assertEqual(
    modelProgressLabel({
      event: "model_run_completed",
      package_id: "demucs@4.0.1",
      output_path: "stems/",
      message: "demucs@4.0.1 completed; output written to stems/.",
    }),
    "demucs@4.0.1 run completed: stems/",
    "completed model run event label",
  );
});

test("builds fallback model chain plans", () => {
  const plan = fallbackPlanModelChain({
    input_path: "/Preview Audio/mix.wav",
    output_path: "/Preview Audio/stems",
    steps: [
      {
        name: "demucs",
        version: "4.0.1",
        params: { stems: "6" },
      },
    ],
  });

  assertEqual(plan.status, "planned", "fallback chain status");
  assertEqual(plan.input, "audio", "fallback chain input");
  assertEqual(plan.output, "stems", "fallback chain output");
  assertEqual(plan.steps[0].package_id, "demucs@4.0.1", "fallback chain step package");
  assertEqual(plan.steps[0].params[0].source, "request", "fallback chain requested param");
  assertEqual(plan.execution.blocker, "chain_runner_unavailable", "fallback chain blocker");
});

test("operation controller owns operation control state", () => {
  const host = operationHost();
  const controller = createOperationController(host);

  assertEqual(controller.state().lifecycle, null, "initial lifecycle operation");
  controller.start("lifecycle");
  assertEqual(controller.state().lifecycle?.operationId, null, "started operation id");
  assertEqual(controller.state().lifecycle?.canceling, false, "started canceling state");
  const lifecycleState = controller.state().lifecycle;
  if (!lifecycleState) {
    throw new Error("started lifecycle state should exist");
  }
  lifecycleState.canceling = true;
  assertEqual(controller.state().lifecycle?.canceling, false, "state snapshot immutability");
  controller.clear("lifecycle");
  assertEqual(controller.state().lifecycle, null, "cleared lifecycle operation");
});

test("operation controller reports scoped errors and refreshes history", async () => {
  const host = operationHost();
  const controller = createOperationController(host);

  await controller.reportError("library", new Error("state db locked"));
  await controller.reportError("model", new Error("model store locked"));

  assertEqual(host.libraryNotice?.tone, "error", "library error tone");
  assertEqual(host.libraryNotice?.message, "state db locked", "library error message");
  assertEqual(host.modelNotice?.tone, "error", "model error tone");
  assertEqual(host.modelNotice?.message, "model store locked", "model error message");
  assertEqual(host.refreshMessage, "model store locked", "history refresh message");
});

test("model controller owns model store initialization state", async () => {
  const host = modelHost();
  const controller = createModelController(host);

  assertEqual(controller.state().modelStoreInitializing, false, "initial model store state");

  const initialization = controller.initializeModelStore();
  assertEqual(controller.state().modelStoreInitializing, true, "model store initializing state");
  assertEqual(
    controller.state().modelNotice?.message ?? null,
    "Initializing model store",
    "initializing notice",
  );

  await initialization;

  assertEqual(host.refreshCount, 1, "model store refresh count");
  assertEqual(controller.state().modelStoreInitializing, false, "model store state cleared");
  assertEqual(controller.state().modelNotice?.tone ?? null, "success", "model store notice tone");
  assertEqual(
    controller.state().modelNotice?.message.includes("Model store ready at ") ?? false,
    true,
    "model store ready notice",
  );
});

test("library controller confirms and applies all direct ready updates", async () => {
  const host = libraryHost(directUpdateSnapshot());
  const controller = createLibraryController(host);

  assertEqual(controller.state().updateAllCount, 1, "initial update-all count");
  controller.requestUpdateAllPackages();

  assertEqual(
    controller.state().pendingUpdateAllPackages?.updates.length ?? 0,
    1,
    "pending update-all count",
  );
  assertEqual(host.peerDialogsCleared, 1, "peer dialogs cleared");

  await controller.confirmUpdateAllPackages();

  assertEqual(controller.state().pendingUpdateAllPackages, null, "update-all pending cleared");
  assertEqual(controller.state().libraryNotice?.tone ?? null, "success", "update-all notice tone");
  assertEqual(
    controller.state().libraryNotice?.message ?? null,
    "Updated 1 package.",
    "update-all notice",
  );
  assertEqual(controller.state().updateAllCount, 0, "remaining update-all count");
  assertEqual(
    host.currentSnapshot.updates.status === "ready"
      ? host.currentSnapshot.updates.updates.length
      : -1,
    0,
    "remaining preview updates",
  );
  assertEqual(
    host.currentSnapshot.installed.find((item) => item.slug === "surge-xt")?.version ?? null,
    "1.3.4",
    "updated package version",
  );
});

test("library scan clears stale library confirmations", async () => {
  const host = libraryHost(directUpdateSnapshot());
  const controller = createLibraryController(host);

  controller.requestUpdateAllPackages();
  assertEqual(
    controller.state().pendingUpdateAllPackages?.updates.length ?? 0,
    1,
    "pending update-all before scan",
  );

  await controller.scanLibrary();

  assertEqual(controller.state().pendingUpdateAllPackages, null, "scan clears update-all");
  assertEqual(controller.state().pendingUpdatePackage, null, "scan clears package update");
  assertEqual(controller.state().pendingRemovePackage, null, "scan clears remove");
  assertEqual(host.peerDialogsCleared, 2, "scan clears peer dialogs");
});

test("library controller skips scan while a library operation is active", async () => {
  const host = libraryHost(directUpdateSnapshot());
  const controller = createLibraryController(host);

  controller.requestUpdateAllPackages();
  host.operationActive = true;
  await controller.scanLibrary();

  assertEqual(
    controller.state().pendingUpdateAllPackages?.updates.length ?? 0,
    1,
    "pending update-all preserved",
  );
  assertEqual(host.startCount, 0, "operation start count");
  assertEqual(host.runCount, 0, "operation run count");
  assertEqual(host.renderCount, 1, "render count");
});

test("library controller skips update and pin actions while active", async () => {
  const host = libraryHost(directUpdateSnapshot(), { operationActive: true });
  const controller = createLibraryController(host);

  controller.requestUpdatePackage("surge-xt");
  controller.requestUpdateAllPackages();
  controller.requestRemovePackage("surge-xt");
  await controller.setPackagePin("surge-xt", true);
  await controller.confirmUpdatePackage();
  await controller.confirmUpdateAllPackages();
  await controller.confirmRemovePackage();

  assertEqual(controller.state().pendingUpdatePackage, null, "pending update");
  assertEqual(controller.state().pendingUpdateAllPackages, null, "pending update-all");
  assertEqual(controller.state().pendingRemovePackage, null, "pending remove");
  assertEqual(host.startCount, 0, "operation start count");
  assertEqual(host.runCount, 0, "operation run count");
  assertEqual(host.renderCount, 0, "render count");
});

test("diagnostics controller refreshes doctor snapshot state", async () => {
  const host = diagnosticsHost();
  const controller = createDiagnosticsController(host);

  await controller.refreshDiagnostics();

  assertEqual(host.refreshCount, 1, "diagnostics refresh count");
  assertEqual(controller.state().diagnosticsRefreshing, false, "diagnostics refreshing cleared");
  assertEqual(controller.state().diagnosticsNotice?.tone ?? null, "success", "diagnostics success tone");
  assertEqual(
    controller.state().diagnosticsNotice?.message ?? null,
    "Doctor checks refreshed",
    "diagnostics success notice",
  );
});

test("diagnostics controller reports refresh failures", async () => {
  const host = diagnosticsHost(new Error("service unavailable"));
  const controller = createDiagnosticsController(host);

  await controller.refreshDiagnostics();

  assertEqual(host.refreshCount, 1, "diagnostics failed refresh count");
  assertEqual(controller.state().diagnosticsRefreshing, false, "failed refresh cleared");
  assertEqual(controller.state().diagnosticsNotice?.tone ?? null, "error", "diagnostics error tone");
  assertEqual(
    controller.state().diagnosticsNotice?.message ?? null,
    "service unavailable",
    "diagnostics error notice",
  );
});

runTests();

async function runTests() {
  let failureCount = 0;
  for (const [name, run] of tests) {
    try {
      await run();
      console.log(`ok ${name}`);
    } catch (error) {
      failureCount += 1;
      console.error(`not ok ${name}`);
      console.error(errorMessage(error));
    }
  }
  if (failureCount > 0) {
    throw new Error(`${failureCount} unit ${failureCount === 1 ? "test" : "tests"} failed.`);
  }
}

function test(name: string, run: () => void | Promise<void>) {
  tests.push([name, run]);
}

function installPlan(overrides: Partial<PackageInstallPlan> = {}): InstallPlanResult {
  return {
    status: "plan",
    plan: {
      slug: "massive",
      name: "Massive",
      vendor: "Native Instruments",
      version: "1.0.0",
      status: "ready",
      destination: "/Library/Audio/Plug-Ins/Components",
      scope: "system",
      installed_version: null,
      formats: [installFormat()],
      installer: null,
      message: "Ready to install.",
      ...overrides,
    },
  };
}

function installFormat(
  overrides: Partial<PackageInstallPlan["formats"][number]> = {},
): PackageInstallPlan["formats"][number] {
  return {
    format: "au",
    install_type: "archive",
    download_type: "url",
    source: "https://example.test/massive.component.zip",
    bundle_path: null,
    has_checksum: true,
    ...overrides,
  };
}

function operationStatus(overrides: Partial<OperationStatus> = {}): OperationStatus {
  return {
    operation_id: "op-1",
    kind: "install_url",
    request: null,
    state: "running",
    created_at: "2026-06-30T00:00:00Z",
    started_at: null,
    finished_at: null,
    result: null,
    error: null,
    events: [],
    ...overrides,
  };
}

function recovery(
  overrides: Partial<OperationRecoverySummary> = {},
): OperationRecoverySummary {
  return {
    interrupted_count: 0,
    retryable_count: 0,
    candidates: [],
    ...overrides,
  };
}

function operationHost() {
  return {
    syncStatus: "",
    lifecycleNotice: null as LifecycleNotice | null,
    libraryNotice: null as LifecycleNotice | null,
    modelNotice: null as LifecycleNotice | null,
    lifecycleEvents: [] as InstallEvent[],
    libraryEvents: [] as LifecycleEvent[],
    modelEvents: [] as ModelOperationEvent[],
    refreshMessage: "",
    renderCount: 0,
    setSyncStatus(message: string) {
      this.syncStatus = message;
    },
    setLifecycleNotice(notice: LifecycleNotice) {
      this.lifecycleNotice = notice;
    },
    setLibraryNotice(notice: LifecycleNotice) {
      this.libraryNotice = notice;
    },
    setModelNotice(notice: LifecycleNotice) {
      this.modelNotice = notice;
    },
    appendLifecycleEvent(event: InstallEvent) {
      this.lifecycleEvents.push(event);
    },
    appendLibraryEvent(event: LifecycleEvent) {
      this.libraryEvents.push(event);
    },
    appendModelEvent(event: ModelOperationEvent) {
      this.modelEvents.push(event);
    },
    async refreshSnapshotAfterOperationError(message: string) {
      this.refreshMessage = message;
    },
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function libraryHost(
  initialSnapshot: DesktopSnapshot = cloneSnapshot(),
  options: { operationActive?: boolean } = {},
) {
  let currentSnapshot = initialSnapshot;
  return {
    get currentSnapshot() {
      return currentSnapshot;
    },
    operationActive: options.operationActive ?? false,
    peerDialogsCleared: 0,
    runCount: 0,
    startCount: 0,
    clearCount: 0,
    renderCount: 0,
    snapshot() {
      return currentSnapshot;
    },
    setSnapshot(nextSnapshot: DesktopSnapshot) {
      currentSnapshot = nextSnapshot;
    },
    isTauriRuntime() {
      return false;
    },
    libraryOperationActive() {
      return this.operationActive;
    },
    async reloadSnapshot() {},
    async runLibraryOperation<T>(run: (progressId?: string) => Promise<T>) {
      this.runCount += 1;
      return run("preview-operation");
    },
    startLibraryOperation() {
      this.startCount += 1;
      this.operationActive = true;
    },
    clearLibraryOperation() {
      this.clearCount += 1;
      this.operationActive = false;
    },
    async reportLibraryError(error: unknown) {
      throw new Error(errorMessage(error));
    },
    clearPeerDialogs() {
      this.peerDialogsCleared += 1;
    },
    clearInstallStateForRemovedPackage() {},
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function modelHost() {
  return {
    operationActive: false,
    refreshCount: 0,
    renderCount: 0,
    async refreshSnapshotData() {
      this.refreshCount += 1;
    },
    modelOperationActive() {
      return this.operationActive;
    },
    async runModelOperation<T>(run: (progressId?: string) => Promise<T>) {
      return run("preview-model-operation");
    },
    clearModelEvents() {},
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function diagnosticsHost(refreshError: Error | null = null) {
  return {
    refreshCount: 0,
    renderCount: 0,
    async refreshSnapshotData() {
      this.refreshCount += 1;
      if (refreshError) {
        throw refreshError;
      }
    },
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function directUpdateSnapshot() {
  const snapshot = cloneSnapshot();
  return {
    ...snapshot,
    updates:
      snapshot.updates.status === "ready"
        ? {
            ...snapshot.updates,
            updates: snapshot.updates.updates.filter((update) => update.slug === "surge-xt"),
          }
        : snapshot.updates,
  };
}

function cloneSnapshot(): DesktopSnapshot {
  return JSON.parse(JSON.stringify(fallbackSnapshot)) as DesktopSnapshot;
}

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${formatValue(expected)}, got ${formatValue(actual)}`);
  }
}

function assertIncludes(value: string, expected: string, message: string) {
  if (!value.includes(expected)) {
    throw new Error(`${message}: expected ${formatValue(value)} to include ${expected}`);
  }
}

function assertThrows(run: () => void, expectedMessage: string) {
  try {
    run();
  } catch (error) {
    assertEqual(errorMessage(error), expectedMessage, "error message");
    return;
  }

  throw new Error(`expected function to throw ${expectedMessage}`);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatValue(value: unknown) {
  return value === null ? "null" : JSON.stringify(value);
}
