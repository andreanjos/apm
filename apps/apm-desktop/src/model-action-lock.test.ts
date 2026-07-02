import { modelActionActive } from "./model-action-lock";

const tests: Array<[string, () => void]> = [];

test("model action lock is inactive when no model work is active", () => {
  assertEqual(modelActionActive(modelLockState()), false, "idle model action lock");
});

test("model action lock includes service and local model actions", () => {
  assertEqual(
    modelActionActive(modelLockState({ modelOperationActive: true })),
    true,
    "service model operation lock",
  );
  assertEqual(
    modelActionActive(modelLockState({ importingCatalogModelId: "demucs@4.0.1" })),
    true,
    "catalog import lock",
  );
  assertEqual(
    modelActionActive(modelLockState({ planningModelChain: true })),
    true,
    "chain planning lock",
  );
});

runTests();

function test(name: string, run: () => void) {
  tests.push([name, run]);
}

function runTests() {
  let failureCount = 0;
  for (const [name, run] of tests) {
    try {
      run();
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

function modelLockState(
  overrides: Partial<Parameters<typeof modelActionActive>[0]> = {},
): Parameters<typeof modelActionActive>[0] {
  return {
    modelOperationActive: false,
    modelStoreInitializing: false,
    modelImporting: false,
    importingCatalogModelId: null,
    installingModelId: null,
    planningModelId: null,
    pullingModelId: null,
    removingModelId: null,
    runningModelId: null,
    planningModelChain: false,
    ...overrides,
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
