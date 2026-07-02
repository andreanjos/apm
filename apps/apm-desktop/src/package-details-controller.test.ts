import { createPackageDetailsController } from "./package-details-controller";
import type { PackageDetailsResult } from "./types";

const tests: Array<[string, () => Promise<void> | void]> = [];

test("loads details for the current selected package", async () => {
  const renders: PackageDetailsResult[] = [];
  const controller = createPackageDetailsController({
    loadPackageDetails: async (slug) => packageDetails(slug),
    formatError,
    render: () => {
      const state = controller.state("surge-xt");
      if (state.packageDetails) {
        renders.push(state.packageDetails);
      }
    },
  });

  await controller.load("surge-xt");

  const state = controller.state("surge-xt");
  assertEqual(state.packageDetails?.status, "found", "details status");
  assertEqual(state.packageDetailsLoading, false, "loading cleared");
  assertEqual(state.packageDetailsError, null, "error cleared");
  assertEqual(renders.at(-1)?.status, "found", "final render contains details");
});

test("drops stale details when selection changes before request finishes", async () => {
  let resolveFirst!: (result: PackageDetailsResult) => void;
  const controller = createPackageDetailsController({
    loadPackageDetails: (slug) => {
      if (slug === "slow-one") {
        return new Promise<PackageDetailsResult>((resolve) => {
          resolveFirst = resolve;
        });
      }
      return Promise.resolve(packageDetails(slug));
    },
    formatError,
    render: () => {},
  });

  const slowLoad = controller.load("slow-one");
  await controller.load("surge-xt");
  resolveFirst(packageDetails("slow-one"));
  await slowLoad;

  assertEqual(controller.state("slow-one").packageDetails, null, "stale result hidden");
  assertEqual(controller.state("surge-xt").packageDetails?.status, "found", "current result kept");
});

test("clears state when no package is selected", async () => {
  const controller = createPackageDetailsController({
    loadPackageDetails: async (slug) => packageDetails(slug),
    formatError,
    render: () => {},
  });

  await controller.load("surge-xt");
  await controller.load(null);

  const state = controller.state(null);
  assertEqual(state.packageDetails, null, "details cleared");
  assertEqual(state.packageDetailsLoading, false, "loading cleared");
  assertEqual(state.packageDetailsError, null, "error cleared");
});

runTests();

function packageDetails(slug: string): PackageDetailsResult {
  return {
    status: "found",
    package: {
      summary: {
        slug,
        name: slug,
        vendor: "Test",
        version: "1.0.0",
        product_type: "effect",
        category: "utility",
        license: "freeware",
        description: "",
        is_paid: false,
        is_installable: true,
        installed: false,
        formats: [],
      },
      aliases: [],
      homepage: `https://example.test/${slug}`,
      purchase_url: null,
      available_versions: ["1.0.0"],
      bundle_ids: [],
    },
  };
}

function test(name: string, run: () => Promise<void> | void) {
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

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

function formatError(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
