import { desktopReleaseReadiness } from "./release-readiness";
import type { DesktopDistribution } from "./types";

const tests: Array<[string, () => void]> = [];

test("hides release readiness for browser previews", () => {
  const readiness = desktopReleaseReadiness({
    channel: "browser_preview",
    app_version: "0.1.1",
    build_profile: "browser",
    sidecar_policy: "sample_data",
    release_gate: "not_applicable",
    signing: "not_checked",
    notarization: "not_checked",
    message: "Browser preview uses sample data and is not an app bundle.",
  });

  assertEqual(readiness, null, "browser preview readiness");
});

test("marks preview bundles as blocked on the public release gate", () => {
  const readiness = requiredReadiness({
    channel: "preview_bundle",
    app_version: "0.1.1",
    build_profile: "release",
    sidecar_policy: "bundled_cli",
    release_gate: "required",
    signing: "not_checked",
    notarization: "not_checked",
    message: "Preview bundle; run the public release gate before distributing.",
  });

  assertEqual(readiness.summary.value, "Public gate required", "preview summary");
  assertEvery(readiness.checks, "warn", "preview check tone");
  assertIncludes(
    readiness.checks.map((item) => item.detail).join("\n"),
    "npm run release:macos:status -- --markdown",
    "preview status command",
  );
});

test("keeps public release channel tied to verifier proof", () => {
  const readiness = requiredReadiness({
    channel: "public_release",
    app_version: "0.1.1",
    build_profile: "release",
    sidecar_policy: "bundled_cli",
    release_gate: "selected",
    signing: "developer_id_required",
    notarization: "required",
    message: "Built through the public macOS release gate.",
  });

  assertEqual(readiness.summary.value, "Verifier proof required", "release summary");
  assertIncludes(
    readiness.checks.map((item) => item.detail).join("\n"),
    "npm run verify:macos:release",
    "release verifier command",
  );
  assertIncludes(
    readiness.checks.map((item) => item.detail).join("\n"),
    "handoff notes before dispatch",
    "release status guidance",
  );
});

runTests();

function requiredReadiness(distribution: DesktopDistribution) {
  const readiness = desktopReleaseReadiness(distribution);
  if (!readiness) {
    throw new Error("expected release readiness");
  }
  return readiness;
}

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

function assertEqual(actual: unknown, expected: unknown, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

function assertEvery(
  items: Array<{ tone: string }>,
  expectedTone: string,
  message: string,
) {
  const mismatch = items.find((item) => item.tone !== expectedTone);
  if (mismatch) {
    throw new Error(`${message}: expected every item to be ${expectedTone}`);
  }
}

function assertIncludes(value: string, expected: string, message: string) {
  if (!value.includes(expected)) {
    throw new Error(`${message}: expected value to include ${expected}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
