import {
  applyRetryProgress,
  createRetryController,
} from "./retry-controller";
import type {
  OperationProgressScope,
  OperationScopeLocks,
} from "./operation-events";
import type {
  InstallEvent,
  LifecycleEvent,
  ModelOperationEvent,
} from "./types";
import type { LifecycleNotice } from "./view-model";

const tests: Array<[string, () => void | Promise<void>]> = [];

test("skips operation retry when the target lane is active", async () => {
  const host = retryHost({
    locks: { sync: true, lifecycle: false, library: false, model: false },
    operationScopes: new Map([["op-sync", "sync"]]),
  });
  const controller = createRetryController(host);

  await controller.retryOperation("op-sync");

  assertEqual(controller.state().retryingOperationId, null, "retry state");
  assertEqual(host.renderCount, 0, "render count");
  assertEqual(host.refreshCount, 0, "refresh count");
  assertEqual(host.syncStatus, "", "sync status");
});

test("allows operation retry when a different lane is active", async () => {
  const host = retryHost({
    locks: { sync: true, lifecycle: false, library: false, model: false },
    operationScopes: new Map([["op-model", "model"]]),
  });
  const controller = createRetryController(host);

  await controller.retryOperation("op-model");

  assertEqual(controller.state().retryingOperationId, null, "retry state");
  assertEqual(host.refreshCount, 1, "refresh count");
  assertEqual(host.renderCount, 2, "render count");
  assertEqual(host.syncStatus, "Retry completed: package update", "sync status");
});

test("skips stale operation retry while any operation lane is active", async () => {
  const host = retryHost({
    locks: { sync: false, lifecycle: true, library: false, model: false },
  });
  const controller = createRetryController(host);

  await controller.retryOperation("op-stale");

  assertEqual(controller.state().retryingOperationId, null, "retry state");
  assertEqual(host.renderCount, 0, "render count");
  assertEqual(host.refreshCount, 0, "refresh count");
  assertEqual(host.syncStatus, "", "sync status");
});

test("skips recovery retry while any operation lane is active", async () => {
  const host = retryHost({
    locks: { sync: false, lifecycle: false, library: true, model: false },
    retryableRecoveryCount: 2,
  });
  const controller = createRetryController(host);

  await controller.retryRecoveryOperations();

  assertEqual(controller.state().retryingRecovery, false, "recovery retry state");
  assertEqual(host.renderCount, 0, "render count");
  assertEqual(host.refreshCount, 0, "refresh count");
  assertEqual(host.syncStatus, "", "sync status");
});

test("routes model retry progress to model notice and events", () => {
  const host = retryHost({
    locks: { sync: false, lifecycle: false, library: false, model: false },
  });

  applyRetryProgress(
    {
      operationId: "op-model",
      kind: "model_run",
      event: {
        event: "model_run_blocked",
        package_id: "demucs@4.0.1",
        blocker: "adapter_runner_unavailable",
        message: "native-mlx execution is not implemented yet",
      },
    },
    host,
  );

  assertEqual(
    host.modelNotice?.message ?? null,
    "Retrying model operation for demucs@4.0.1",
    "model retry notice",
  );
  assertEqual(host.modelEvents.length, 1, "model event count");
  assertEqual(host.renderCount, 1, "render count");
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

function retryHost(options: {
  locks: OperationScopeLocks;
  operationScopes?: Map<string, OperationProgressScope>;
  retryableRecoveryCount?: number;
}) {
  return {
    syncStatus: "",
    lifecycleNotice: null as LifecycleNotice | null,
    libraryNotice: null as LifecycleNotice | null,
    modelNotice: null as LifecycleNotice | null,
    lifecycleEvents: [] as InstallEvent[],
    libraryEvents: [] as LifecycleEvent[],
    modelEvents: [] as ModelOperationEvent[],
    renderCount: 0,
    refreshCount: 0,
    retryableRecoveryCount() {
      return options.retryableRecoveryCount ?? 0;
    },
    activeOperationLocks() {
      return options.locks;
    },
    retryOperationScope(operationId: string) {
      return options.operationScopes?.get(operationId) ?? null;
    },
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
    async refreshSnapshotData() {
      this.refreshCount += 1;
    },
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      this.renderCount += 1;
    },
  };
}

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
