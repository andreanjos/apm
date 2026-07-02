import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { repoRoot } from "./macos-release-github-common.mjs";
import {
  preparePreviewArtifacts,
  rebuildPreviewDmgArtifacts,
  signPreviewAppBundle,
} from "./macos-preview-sign.mjs";
import {
  previewSignatureErrors,
  strictCodeSignatureErrors,
} from "./macos-verify.mjs";

const tests = [];

test("signs preview app bundles ad-hoc and verifies the result", () => {
  withTempDir((dir) => {
    const app = resolve(dir, "apm.app");
    mkdirSync(app);
    const calls = [];

    signPreviewAppBundle(app, {
      runCommand(command, args) {
        calls.push([command, args]);
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertDeepEqual(
      calls,
      [
        ["codesign", ["--force", "--deep", "--sign", "-", app]],
        ["codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]],
      ],
      "codesign calls",
    );
  });
});

test("prepare step signs the app and rebuilds existing preview DMGs", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const app = resolve(macosDir, "apm.app");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const tempDmg = resolve(dmgDir, ".tmp-test-apm_0.1.1_aarch64.dmg");
    const scratchDmg = resolve(dmgDir, "rw.42..tmp-test-apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(app, { recursive: true });
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");
    const calls = [];

    preparePreviewArtifacts({
      appBundlePath: app,
      dmgDir,
      macosBundleDir: macosDir,
      bundleDmgScript,
      tempToken: "test",
      runCommand(command, args) {
        calls.push([command, args]);
        if (command === "bash") {
          writeFileSync(tempDmg, "rebuilt dmg");
          writeFileSync(scratchDmg, "builder scratch dmg");
        }
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertEqual(existsSync(tempDmg), false, "temporary DMG renamed away");
    assertEqual(existsSync(scratchDmg), false, "builder scratch DMG cleaned");
    assertEqual(readFileSync(dmg, "utf8"), "rebuilt dmg", "rebuilt DMG replaces stale DMG");
    assertDeepEqual(
      calls,
      [
        ["codesign", ["--force", "--deep", "--sign", "-", app]],
        ["codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]],
        [
          "bash",
          [
            bundleDmgScript,
            "--volname",
            "apm",
            "--window-size",
            "500",
            "350",
            "--icon-size",
            "128",
            "--icon",
            "apm.app",
            "140",
            "170",
            "--app-drop-link",
            "360",
            "170",
            "--no-internet-enable",
            tempDmg,
            macosDir,
          ],
        ],
      ],
      "prepare calls",
    );
  });
});

test("skips DMG rebuilds when no preview DMG exists", () => {
  withTempDir((dir) => {
    const dmgDir = resolve(dir, "dmg");
    mkdirSync(dmgDir);
    const calls = [];

    rebuildPreviewDmgArtifacts({
      dmgDir,
      runCommand(command, args) {
        calls.push([command, args]);
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertDeepEqual(calls, [], "rebuild calls");
  });
});

test("retries preview DMG rebuilds after transient builder failures", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const tempDmg = resolve(dmgDir, ".tmp-retry-apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(macosDir);
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");
    let attempts = 0;

    rebuildPreviewDmgArtifacts({
      dmgDir,
      macosBundleDir: macosDir,
      bundleDmgScript,
      tempToken: "retry",
      runCommand(command) {
        assertEqual(command, "bash", "builder command");
        attempts += 1;
        if (attempts === 1) {
          writeFileSync(tempDmg, "partial failed dmg");
          return { status: 1, stdout: "", stderr: "Finder got an error" };
        }
        assertEqual(existsSync(tempDmg), false, "failed temporary DMG cleaned before retry");
        writeFileSync(tempDmg, "rebuilt dmg");
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertEqual(attempts, 2, "builder attempts");
    assertEqual(readFileSync(dmg, "utf8"), "rebuilt dmg", "rebuilt DMG replaces stale DMG");
    assertEqual(existsSync(tempDmg), false, "temporary DMG renamed away");
  });
});

test("retries timed-out preview DMG rebuilds and removes builder scratch files", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const tempDmg = resolve(dmgDir, ".tmp-timeout-apm_0.1.1_aarch64.dmg");
    const scratchDmg = resolve(dmgDir, "rw.1234..tmp-timeout-apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(macosDir);
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");
    const builderOptions = [];
    let attempts = 0;

    rebuildPreviewDmgArtifacts({
      dmgDir,
      macosBundleDir: macosDir,
      bundleDmgScript,
      tempToken: "timeout",
      rebuildTimeoutMs: 23,
      runCommand(command, _args, options) {
        assertEqual(command, "bash", "builder command");
        builderOptions.push(options);
        attempts += 1;
        if (attempts === 1) {
          writeFileSync(tempDmg, "partial timed-out dmg");
          writeFileSync(scratchDmg, "timed-out scratch dmg");
          return {
            status: null,
            stdout: "",
            stderr: "",
            error: { code: "ETIMEDOUT" },
          };
        }
        assertEqual(existsSync(tempDmg), false, "timed-out temporary DMG cleaned before retry");
        assertEqual(existsSync(scratchDmg), false, "timed-out scratch DMG cleaned before retry");
        writeFileSync(tempDmg, "rebuilt dmg");
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertEqual(attempts, 2, "builder attempts");
    assertDeepEqual(
      builderOptions,
      [
        { timeout: 23, killSignal: "SIGTERM" },
        { timeout: 23, killSignal: "SIGTERM" },
      ],
      "builder timeout options",
    );
    assertEqual(readFileSync(dmg, "utf8"), "rebuilt dmg", "rebuilt DMG replaces stale DMG");
    assertEqual(existsSync(tempDmg), false, "temporary DMG renamed away");
    assertEqual(existsSync(scratchDmg), false, "scratch DMG cleaned");
  });
});

test("reports final preview DMG rebuild failure after retry attempts", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const tempDmg = resolve(dmgDir, ".tmp-retry-fail-apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(macosDir);
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");
    let attempts = 0;

    assertThrows(
      () => rebuildPreviewDmgArtifacts({
        dmgDir,
        macosBundleDir: macosDir,
        bundleDmgScript,
        tempToken: "retry-fail",
        runCommand(command) {
          assertEqual(command, "bash", "builder command");
          attempts += 1;
          writeFileSync(tempDmg, `partial failed dmg ${attempts}`);
          return { status: 1, stdout: "", stderr: `Finder got an error ${attempts}` };
        },
      }),
      "Finder got an error 2",
      "final builder error",
    );

    assertEqual(attempts, 2, "builder attempts");
    assertEqual(readFileSync(dmg, "utf8"), "stale dmg", "stale DMG remains on failure");
    assertEqual(existsSync(tempDmg), false, "failed temporary DMG cleaned");
  });
});

test("reports timed-out preview DMG rebuild failures with the configured timeout", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const tempDmg = resolve(dmgDir, ".tmp-timeout-fail-apm_0.1.1_aarch64.dmg");
    const scratchDmg = resolve(dmgDir, "rw.5678..tmp-timeout-fail-apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(macosDir);
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");

    assertThrows(
      () => rebuildPreviewDmgArtifacts({
        dmgDir,
        macosBundleDir: macosDir,
        bundleDmgScript,
        rebuildAttempts: 1,
        rebuildTimeoutMs: 31,
        tempToken: "timeout-fail",
        runCommand() {
          writeFileSync(tempDmg, "partial timed-out dmg");
          writeFileSync(scratchDmg, "timed-out scratch dmg");
          return {
            status: null,
            stdout: "",
            stderr: "",
            error: { code: "ETIMEDOUT" },
          };
        },
      }),
      "bundle_dmg.sh timed out after 31ms",
      "timeout error",
    );

    assertEqual(readFileSync(dmg, "utf8"), "stale dmg", "stale DMG remains on timeout");
    assertEqual(existsSync(tempDmg), false, "timed-out temporary DMG cleaned");
    assertEqual(existsSync(scratchDmg), false, "timed-out scratch DMG cleaned");
  });
});

test("rejects invalid preview DMG retry counts before rebuilding", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(macosDir);
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");
    const calls = [];

    assertThrows(
      () => rebuildPreviewDmgArtifacts({
        dmgDir,
        macosBundleDir: macosDir,
        bundleDmgScript,
        rebuildAttempts: 0,
        runCommand(command, args) {
          calls.push([command, args]);
          return { status: 0, stdout: "", stderr: "" };
        },
      }),
      "invalid preview DMG rebuild attempts: 0",
      "invalid retry count",
    );

    assertDeepEqual(calls, [], "builder calls");
  });
});

test("rejects invalid preview DMG rebuild timeouts before rebuilding", () => {
  withTempDir((dir) => {
    const macosDir = resolve(dir, "macos");
    const dmgDir = resolve(dir, "dmg");
    const dmg = resolve(dmgDir, "apm_0.1.1_aarch64.dmg");
    const bundleDmgScript = resolve(dmgDir, "bundle_dmg.sh");
    mkdirSync(macosDir);
    mkdirSync(dmgDir);
    writeFileSync(dmg, "stale dmg");
    writeFileSync(bundleDmgScript, "#!/bin/sh\n");
    const calls = [];

    assertThrows(
      () => rebuildPreviewDmgArtifacts({
        dmgDir,
        macosBundleDir: macosDir,
        bundleDmgScript,
        rebuildTimeoutMs: 0,
        runCommand(command, args) {
          calls.push([command, args]);
          return { status: 0, stdout: "", stderr: "" };
        },
      }),
      "invalid preview DMG rebuild timeout: 0",
      "invalid timeout",
    );

    assertDeepEqual(calls, [], "builder calls");
  });
});

test("rejects missing preview app bundles before signing", () => {
  withTempDir((dir) => {
    assertThrows(
      () => signPreviewAppBundle(resolve(dir, "missing.app")),
      "missing app bundle",
      "missing bundle error",
    );
  });
});

test("reports preview signature verification failures with a preview label", () => {
  const errors = previewSignatureErrors("/tmp/apm.app", {
    runCommand: () => ({ status: 1, stdout: "", stderr: "resource envelope is obsolete" }),
  }).join("\n");

  assertIncludes(errors, "preview app codesign verification failed", "preview label");
  assertIncludes(errors, "resource envelope is obsolete", "codesign stderr");
});

test("accepts strict code signatures when codesign verify succeeds", () => {
  assertDeepEqual(
    strictCodeSignatureErrors("/tmp/apm.app", {
      runCommand: () => ({ status: 0, stdout: "", stderr: "" }),
    }),
    [],
    "signature errors",
  );
});

test("rejects unknown preview signing arguments before signing", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-preview-sign.mjs"),
    "--skip-dmg=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(result.stderr, "--skip-dmg does not accept a value", "flag value error");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

runTests();

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-preview-sign-test-"));
  try {
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

function assertThrows(run, expected, message) {
  try {
    run();
  } catch (error) {
    assertIncludes(errorMessage(error), expected, message);
    return;
  }
  throw new Error(`${message}: expected function to throw`);
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
