import { createModelController } from "./model-controller";

const tests: Array<[string, () => void | Promise<void>]> = [];

test("model controller skips actions while a model operation is active", async () => {
  const host = modelHost({ operationActive: true });
  const controller = createModelController(host);

  await controller.initializeModelStore();
  await controller.importModelManifest();
  await controller.importModelCatalogPackage("demucs", "4.0.1");
  await controller.pullModelWeights("demucs", "4.0.1");
  await controller.installModelPackage("demucs", "4.0.1");
  await controller.removeModelPackage("demucs", "4.0.1");
  await controller.planModelRun("demucs", "4.0.1");
  await controller.runModel("demucs", "4.0.1");
  controller.addModelChainStep("demucs", "4.0.1", "demucs@4.0.1");
  await controller.planModelChain();

  const state = controller.state();
  assertEqual(state.modelStoreInitializing, false, "model store initializing");
  assertEqual(state.modelImporting, false, "model importing");
  assertEqual(state.importingCatalogModelId, null, "catalog import id");
  assertEqual(state.installingModelId, null, "installing id");
  assertEqual(state.planningModelId, null, "planning id");
  assertEqual(state.pullingModelId, null, "pulling id");
  assertEqual(state.removingModelId, null, "removing id");
  assertEqual(state.runningModelId, null, "running id");
  assertEqual(state.planningModelChain, false, "planning model chain");
  assertEqual(state.modelChainSteps.length, 0, "chain step count");
  assertEqual(state.modelNotice, null, "model notice");
  assertEqual(host.clearEventsCount, 0, "cleared event count");
  assertEqual(host.refreshCount, 0, "refresh count");
  assertEqual(host.renderCount, 0, "render count");
  assertEqual(host.runCount, 0, "run count");
});

test("model controller preserves active local action state", async () => {
  const host = modelHost();
  const controller = createModelController(host);

  const install = controller.installModelPackage("demucs", "4.0.1");
  assertEqual(controller.state().installingModelId, "demucs@4.0.1", "installing id");

  await controller.pullModelWeights("whisper", "1.0.0");
  await controller.importModelCatalogPackage("whisper", "1.0.0");
  controller.addModelChainStep("whisper", "1.0.0", "whisper@1.0.0");
  await controller.planModelChain();

  const lockedState = controller.state();
  assertEqual(lockedState.installingModelId, "demucs@4.0.1", "locked installing id");
  assertEqual(lockedState.pullingModelId, null, "locked pulling id");
  assertEqual(lockedState.importingCatalogModelId, null, "locked catalog import id");
  assertEqual(lockedState.modelChainSteps.length, 0, "locked chain step count");
  assertEqual(host.clearEventsCount, 1, "clear events count");
  assertEqual(host.runCount, 1, "run count");
  assertEqual(host.renderCount, 1, "render count while locked");

  host.resolveRun?.({ status: "failed", error: "model install stopped" });
  await install;

  assertEqual(controller.state().installingModelId, null, "installing id cleared");
  assertEqual(
    controller.state().modelNotice?.message ?? null,
    "model install stopped",
    "install failure notice",
  );
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

function modelHost(options: { operationActive?: boolean } = {}) {
  const host = {
    operationActive: options.operationActive ?? false,
    clearEventsCount: 0,
    refreshCount: 0,
    renderCount: 0,
    runCount: 0,
    resolveRun: null as ((value: unknown) => void) | null,
    async refreshSnapshotData() {
      host.refreshCount += 1;
    },
    modelOperationActive() {
      return host.operationActive;
    },
    async runModelOperation<T>(_run: (progressId?: string) => Promise<T>) {
      host.runCount += 1;
      return new Promise<T>((resolve) => {
        host.resolveRun = (value: unknown) => resolve(value as T);
      });
    },
    clearModelEvents() {
      host.clearEventsCount += 1;
    },
    formatError(error: unknown) {
      return errorMessage(error);
    },
    render() {
      host.renderCount += 1;
    },
  };
  return host;
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
