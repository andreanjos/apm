import { fallbackInstallPlan, fallbackSnapshot } from "./fallback";
import { createHandoffController } from "./handoff-controller";
import { createInstallController } from "./install-controller";
import type {
  DesktopSnapshot,
  InstallPlanResult,
} from "./types";
import type { LifecycleNotice } from "./view-model";

const tests: Array<[string, () => void | Promise<void>]> = [];

test("install controller skips review and request while lifecycle operation is active", async () => {
  const host = installHost({ lifecycleActive: true });
  const controller = createInstallController(host);

  controller.setInstallPlan(fallbackInstallPlan("surge-xt"));
  await controller.reviewInstall("surge-xt");
  controller.requestUrlInstall("surge-xt", "AU");
  await controller.chooseArchiveAndInstall("surge-xt", "AU");

  assertEqual(controller.state().installStatus, "No install plan loaded", "install status");
  assertEqual(controller.state().pendingUrlInstall, null, "pending URL install");
  assertEqual(controller.state().pendingArchiveInstall, null, "pending archive install");
  assertEqual(host.peerDialogsCleared, 0, "peer dialogs cleared");
  assertEqual(host.renderCount, 0, "render count");
  assertEqual(host.startCount, 0, "operation start count");
  assertEqual(host.runCount, 0, "operation run count");
});

test("install controller preserves pending URL install while lifecycle operation is active", async () => {
  const host = installHost();
  const controller = createInstallController(host);

  controller.setInstallPlan(fallbackInstallPlan("surge-xt"));
  controller.requestUrlInstall("surge-xt", "AU");
  host.lifecycleActive = true;
  await controller.confirmUrlInstall();
  controller.cancelUrlInstall();

  assertEqual(
    controller.state().pendingUrlInstall?.slug ?? null,
    "surge-xt",
    "pending URL install preserved",
  );
  assertEqual(host.startCount, 0, "operation start count");
  assertEqual(host.runCount, 0, "operation run count");
  assertEqual(host.renderCount, 1, "render count");
});

test("install controller replans ready installs when install scope changes", async () => {
  const host = installHost();
  const controller = createInstallController(host);

  await controller.reviewInstall("surge-xt");

  assertEqual(controller.state().installScope, "user", "initial install scope");
  assertEqual(
    planDestination(controller.state().installPlan),
    "~/Library/Audio/Plug-Ins/",
    "initial plan destination",
  );

  await controller.setInstallScope("system");
  controller.requestUrlInstall("surge-xt", "AU");

  assertEqual(controller.state().installScope, "system", "updated install scope");
  assertEqual(
    planDestination(controller.state().installPlan),
    "/Library/Audio/Plug-Ins/",
    "updated plan destination",
  );
  assertEqual(
    controller.state().pendingUrlInstall?.installScope ?? null,
    "system",
    "pending URL install scope",
  );
  assertEqual(
    controller.state().pendingUrlInstall?.destination ?? null,
    "/Library/Audio/Plug-Ins/",
    "pending URL install destination",
  );
});

test("handoff controller skips open and confirm while lifecycle operation is active", async () => {
  const host = handoffHost(manualInstallPlan());
  const controller = createHandoffController(host);

  host.lifecycleActive = true;
  controller.openInstallHandoff("massive");

  assertEqual(controller.state().pendingInstallHandoff, null, "active open skipped");
  assertEqual(host.renderCount, 0, "active open render count");
  assertEqual(host.peerDialogsCleared, 0, "peer dialogs cleared");

  host.lifecycleActive = false;
  controller.openInstallHandoff("massive");
  host.lifecycleActive = true;
  await controller.confirmInstallHandoff();
  controller.cancelInstallHandoff();

  assertEqual(
    controller.state().pendingInstallHandoff?.slug ?? null,
    "massive",
    "pending handoff preserved",
  );
  assertEqual(host.renderCount, 1, "pending handoff render count");
  assertEqual(host.lifecycleNotice, null, "lifecycle notice");
});

runTests();

function test(name: string, run: () => void | Promise<void>) {
  tests.push([name, run]);
}

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

function installHost(options: { lifecycleActive?: boolean } = {}) {
  let snapshot = cloneSnapshot();
  return {
    lifecycleActive: options.lifecycleActive ?? false,
    peerDialogsCleared: 0,
    renderCount: 0,
    runCount: 0,
    startCount: 0,
    snapshot() {
      return snapshot;
    },
    setSnapshot(nextSnapshot: DesktopSnapshot) {
      snapshot = nextSnapshot;
    },
    isTauriRuntime() {
      return false;
    },
    lifecycleOperationActive() {
      return this.lifecycleActive;
    },
    async reloadSnapshot() {},
    async runInstallOperation<T>(run: (progressId?: string) => Promise<T>) {
      this.runCount += 1;
      return run("preview-operation");
    },
    startInstallOperation() {
      this.startCount += 1;
      this.lifecycleActive = true;
    },
    clearInstallOperation() {
      this.lifecycleActive = false;
    },
    async reportInstallError(error: unknown) {
      throw new Error(errorMessage(error));
    },
    clearPeerInstallDialogs() {
      this.peerDialogsCleared += 1;
    },
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function handoffHost(plan: InstallPlanResult) {
  return {
    lifecycleActive: false,
    lifecycleNotice: null as LifecycleNotice | null,
    peerDialogsCleared: 0,
    renderCount: 0,
    installPlan() {
      return plan;
    },
    setInstallPlan(nextPlan: InstallPlanResult | null) {
      plan = nextPlan ?? plan;
    },
    setLifecycleNotice(notice: LifecycleNotice | null) {
      this.lifecycleNotice = notice;
    },
    lifecycleOperationActive() {
      return this.lifecycleActive;
    },
    clearPeerInstallDialogs() {
      this.peerDialogsCleared += 1;
    },
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function manualInstallPlan(): InstallPlanResult {
  return {
    status: "plan",
    plan: {
      slug: "massive",
      name: "Massive",
      vendor: "Native Instruments",
      version: "1.0.0",
      status: "manual_required",
      destination: null,
      scope: "user",
      installed_version: null,
      formats: [
        {
          format: "AU",
          install_type: "component",
          download_type: "manual",
          source: "https://example.test/massive",
          bundle_path: null,
          has_checksum: false,
        },
      ],
      installer: null,
      message: "Manual download required.",
    },
  };
}

function cloneSnapshot(): DesktopSnapshot {
  return JSON.parse(JSON.stringify(fallbackSnapshot)) as DesktopSnapshot;
}

function planDestination(plan: InstallPlanResult | null) {
  if (!plan || plan.status !== "plan") {
    throw new Error("expected install plan");
  }
  return plan.plan.destination;
}

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${formatValue(expected)}, got ${formatValue(actual)}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatValue(value: unknown) {
  return value === null ? "null" : JSON.stringify(value);
}
