import { fallbackSnapshot } from "./fallback-data";
import { modelPackagesSection } from "./model-packages";

const tests: Array<[string, () => void]> = [];

test("renders model operation cancellation control", () => {
  const html = modelPackagesSection(
    fallbackSnapshot.models,
    fallbackSnapshot.model_catalog,
    { modelOperation: { operationId: "op-model", canceling: false } },
  );

  assertIncludes(html, 'data-cancel-operation-scope="model"', "model cancel action");
  assertIncludes(html, "Cancel operation", "model cancel label");
});

test("renders model operation cancellation requested state", () => {
  const html = modelPackagesSection(
    fallbackSnapshot.models,
    fallbackSnapshot.model_catalog,
    { modelOperation: { operationId: "op-model", canceling: true } },
  );

  assertIncludes(html, 'data-cancel-operation-scope="model"', "model cancel action");
  assertIncludes(html, "Cancel requested", "model cancel requested label");
  assertIncludes(html, "disabled", "model cancel disabled state");
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

function assertIncludes(html: string, expected: string, message: string) {
  if (!html.includes(expected)) {
    throw new Error(`${message}: expected rendered HTML to include ${expected}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
