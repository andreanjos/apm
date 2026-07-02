import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { repoRoot } from "./macos-release-github-common.mjs";
import {
  appBundlePayloadErrors,
  mountedDmgAppErrors,
  selectDmgArtifacts,
} from "./macos-verify.mjs";

const tests = [];

test("selects only apm DMG artifacts", () => {
  const dmgs = selectDmgArtifacts(
    ["apm_0.1.1_aarch64.dmg", "apm.app", "other.dmg", "apm_0.1.1.sha256"],
    "/tmp/bundle/dmg",
  );

  assertEqual(dmgs.length, 1, "DMG count");
  assertEqual(dmgs[0], "/tmp/bundle/dmg/apm_0.1.1_aarch64.dmg", "DMG path");
});

test("accepts mounted DMG with app bundle and sidecar", () => {
  withMountedAppFixture((mountPoint) => {
    assertDeepEqual(mountedDmgAppErrors(mountPoint), [], "valid mounted app");
  });
});

test("checks app bundle payload files and executability", () => {
  withMountedAppFixture((mountPoint) => {
    const app = resolve(mountPoint, "apm.app");
    assertDeepEqual(appBundlePayloadErrors(app), [], "valid app payload");

    chmodSync(resolve(app, "Contents/MacOS/apm-cli"), 0o644);
    assertIncludes(
      appBundlePayloadErrors(app).join("\n"),
      "executable",
      "non-executable sidecar error",
    );
    assertIncludes(
      appBundlePayloadErrors(app, { requireIcon: true }).join("\n"),
      "apm.icns",
      "missing icon error",
    );
  });
});

test("rejects mounted DMG without bundled sidecar", () => {
  withMountedAppFixture((mountPoint) => {
    rmSync(resolve(mountPoint, "apm.app/Contents/MacOS/apm-cli"));

    assertIncludes(
      mountedDmgAppErrors(mountPoint).join("\n"),
      "apm-cli",
      "missing sidecar error",
    );
  });
});

test("rejects mounted DMG without Applications install target", () => {
  withMountedAppFixture((mountPoint) => {
    unlinkSync(resolve(mountPoint, "Applications"));

    assertIncludes(
      mountedDmgAppErrors(mountPoint).join("\n"),
      "Applications install target",
      "missing Applications target error",
    );
  });
});

test("rejects mounted DMG with wrong Applications target", () => {
  withMountedAppFixture((mountPoint) => {
    unlinkSync(resolve(mountPoint, "Applications"));
    symlinkSync("/tmp", resolve(mountPoint, "Applications"));

    assertIncludes(
      mountedDmgAppErrors(mountPoint).join("\n"),
      "must target /Applications",
      "wrong Applications target error",
    );
  });
});

test("rejects DMG without root app bundle", () => {
  const mountPoint = mkdtempSync(resolve(tmpdir(), "apm-dmg-test-"));
  try {
    assertIncludes(
      mountedDmgAppErrors(mountPoint).join("\n"),
      "apm.app at its root",
      "missing root app error",
    );
  } finally {
    rmSync(mountPoint, { recursive: true, force: true });
  }
});

test("rejects unknown verifier arguments before artifact checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-verify.mjs"),
    "--mode",
    "preview",
    "--require-dmg=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(result.stderr, "--require-dmg does not accept a value", "flag value error");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

test("prints verifier help without artifact checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-verify.mjs"),
    "--help",
    "--mode",
    "release",
    "--require-dmg=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 0, "exit status");
  assertEqual(result.stderr, "", "stderr");
  assertIncludes(result.stdout, "Usage: npm run verify:macos:preview", "preview usage");
  assertIncludes(result.stdout, "npm run verify:macos:release", "release usage");
  assertIncludes(result.stdout, "without artifact checks", "help safety");
});

runTests();

function withMountedAppFixture(run) {
  const mountPoint = mkdtempSync(resolve(tmpdir(), "apm-dmg-test-"));
  try {
    const contents = resolve(mountPoint, "apm.app/Contents");
    const macos = resolve(contents, "MacOS");
    mkdirSync(macos, { recursive: true });
    writeFileSync(resolve(contents, "Info.plist"), "<plist></plist>");
    for (const binary of ["apm-desktop", "apm-cli"]) {
      const path = resolve(macos, binary);
      writeFileSync(path, "#!/bin/sh\n");
      chmodSync(path, 0o755);
    }
    symlinkSync("/Applications", resolve(mountPoint, "Applications"));
    run(mountPoint);
  } finally {
    rmSync(mountPoint, { recursive: true, force: true });
  }
}

function test(name, run) {
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

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

function assertDeepEqual(actual, expected, message) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function assertIncludes(value, expected, message) {
  if (!value.includes(expected)) {
    throw new Error(`${message}: expected value to include ${expected}`);
  }
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
