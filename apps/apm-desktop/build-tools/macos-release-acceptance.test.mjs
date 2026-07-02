import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { repoRoot } from "./macos-release-github-common.mjs";
import {
  checksumManifestText,
  releaseEvidenceManifest,
} from "./macos-release-assets.mjs";
import {
  releaseArtifactInventoryErrors,
  runReleaseArtifactAcceptanceCommand,
} from "./macos-release-acceptance.mjs";

const tests = [];

test("accepts complete release artifact inventory", () => {
  withReleaseArtifacts((dir) => {
    assertDeepEqual(
      releaseArtifactInventoryErrors({ artifactsDir: dir, version: "0.1.1" }),
      [],
      "inventory errors",
    );
  });
});

test("rejects missing release evidence manifest", () => {
  withReleaseArtifacts((dir) => {
    rmSync(resolve(dir, "apm-0.1.1-desktop-release-evidence.json"));

    assertIncludes(
      releaseArtifactInventoryErrors({ artifactsDir: dir, version: "0.1.1" }).join("\n"),
      "missing release evidence manifest",
      "missing evidence error",
    );
  });
});

test("rejects stale release artifact versions", () => {
  withReleaseArtifacts((dir) => {
    assertIncludes(
      releaseArtifactInventoryErrors({ artifactsDir: dir, version: "0.1.2" }).join("\n"),
      "missing app zip",
      "missing app zip version error",
    );
    assertIncludes(
      releaseArtifactInventoryErrors({ artifactsDir: dir, version: "0.1.2" }).join("\n"),
      "version 0.1.1 must match 0.1.2",
      "stale DMG version error",
    );
  });
});

test("rejects unexpected release artifact files", () => {
  withReleaseArtifacts((dir) => {
    writeFileSync(resolve(dir, "debug.log"), "extra");

    assertIncludes(
      releaseArtifactInventoryErrors({ artifactsDir: dir, version: "0.1.1" }).join("\n"),
      "unexpected release artifact",
      "unexpected artifact error",
    );
  });
});

test("rejects release DMGs missing checksum and evidence coverage", () => {
  withReleaseArtifacts((dir) => {
    writeFileSync(resolve(dir, "apm_0.1.1_extra.dmg"), "extra dmg");

    const errors = releaseArtifactInventoryErrors({
      artifactsDir: dir,
      version: "0.1.1",
    }).join("\n");
    assertIncludes(
      errors,
      "checksum manifest must include release artifact: apm_0.1.1_extra.dmg",
      "checksum coverage error",
    );
    assertIncludes(
      errors,
      "release evidence must include release artifact: apm_0.1.1_extra.dmg",
      "evidence coverage error",
    );
  });
});

test("shows release acceptance help without artifact checks", () => {
  const output = [];
  const errors = [];
  const status = runReleaseArtifactAcceptanceCommand([
    "--help",
    "--artifacts-dir",
    "/definitely/missing/apm-release-artifacts",
  ], {
    log: (line) => output.push(line),
    error: (line) => errors.push(line),
  });

  assertEqual(status, 0, "help status");
  assertIncludes(output.join("\n"), "Usage: npm run accept:macos:release", "help usage");
  assertDeepEqual(errors, [], "help errors");
});

test("rejects unknown release acceptance arguments before artifact checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-acceptance.mjs"),
    "--version",
    "0.1.1",
    "--artifacts-dir",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "unknown argument status");
  assertIncludes(result.stderr, "--artifacts-dir requires a value", "missing artifact dir");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

runTests();

function withReleaseArtifacts(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-release-acceptance-test-"));
  try {
    mkdirSync(dir, { recursive: true });
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const checksums = resolve(dir, "apm-0.1.1-desktop.sha256");
    const evidence = resolve(dir, "apm-0.1.1-desktop-release-evidence.json");

    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(checksums, checksumManifestText([appZip, dmg]));
    writeFileSync(
      evidence,
      `${JSON.stringify(
        releaseEvidenceManifest({
          version: "0.1.1",
          appZip,
          dmgs: [dmg],
          checksumManifest: checksums,
          generatedAt: "2026-07-01T00:00:00.000Z",
        }),
        null,
        2,
      )}\n`,
    );

    run(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
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

function assertDeepEqual(actual, expected, message) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
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
