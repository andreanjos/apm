import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import {
  macosPreviewLaunchTarget,
  openMacosPreview,
  optionsFromArgs,
  runPreviewOpenCommand,
} from "./macos-preview-open.mjs";

const tests = [];

test("selects the preview app by default", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);

    const target = macosPreviewLaunchTarget({ appPath });

    assertDeepEqual(target.errors, [], "errors");
    assertEqual(target.value.type, "app", "target type");
    assertEqual(target.value.path, appPath, "target path");
  });
});

test("reports how to build the preview app when it is missing", () => {
  withTempDir((dir) => {
    const target = macosPreviewLaunchTarget({
      appPath: resolve(dir, "missing.app"),
    });

    assertIncludes(target.errors.join("\n"), "missing preview app bundle", "missing app");
    assertIncludes(
      target.errors.join("\n"),
      "run npm run bundle:macos:verified first",
      "build hint",
    );
  });
});

test("selects the newest preview DMG when requested", () => {
  withTempDir((dir) => {
    const olderDmg = resolve(dir, "apm_0.1.10_aarch64.dmg");
    const newerDmg = resolve(dir, "apm_0.1.9_aarch64.dmg");
    writeFileSync(olderDmg, "");
    writeFileSync(newerDmg, "");
    writeFileSync(resolve(dir, "ignored.txt"), "");
    utimesSync(olderDmg, new Date("2026-01-01T00:00:00Z"), new Date("2026-01-01T00:00:00Z"));
    utimesSync(newerDmg, new Date("2026-01-02T00:00:00Z"), new Date("2026-01-02T00:00:00Z"));

    const target = macosPreviewLaunchTarget({ dmg: true, dmgDir: dir });

    assertDeepEqual(target.errors, [], "errors");
    assertEqual(target.value.type, "dmg", "target type");
    assertEqual(
      target.value.path,
      newerDmg,
      "target path",
    );
  });
});

test("reports how to build the preview DMG when it is missing", () => {
  withTempDir((dir) => {
    writeFileSync(resolve(dir, "ignored.txt"), "");

    const target = macosPreviewLaunchTarget({ dmg: true, dmgDir: dir });

    assertIncludes(target.errors.join("\n"), "missing apm preview DMG", "missing DMG");
    assertIncludes(
      target.errors.join("\n"),
      "run npm run bundle:macos:verified first",
      "build hint",
    );
  });
});

test("opens the selected preview artifact on macOS", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);
    const calls = [];

    const result = openMacosPreview({
      appPath,
      platform: "darwin",
      verifyTarget: () => [],
      runCommand: (command, args) => {
        calls.push([command, args]);
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertDeepEqual(result.errors, [], "errors");
    assertDeepEqual(calls, [["open", [appPath]]], "open command");
  });
});

test("does not call open outside macOS", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);
    const calls = [];

    const result = openMacosPreview({
      appPath,
      platform: "linux",
      verifyTarget: () => {
        throw new Error("verification should not run outside macOS");
      },
      runCommand: (command, args) => {
        calls.push([command, args]);
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertIncludes(result.errors.join("\n"), "requires macOS", "platform error");
    assertDeepEqual(calls, [], "open calls");
  });
});

test("verifies the selected preview artifact before opening", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);
    const calls = [];
    const verified = [];

    const result = openMacosPreview({
      appPath,
      platform: "darwin",
      verifyTarget: (target) => {
        verified.push(target);
        return ["preview app codesign verification failed: unsigned"];
      },
      runCommand: (command, args) => {
        calls.push([command, args]);
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertIncludes(
      result.errors.join("\n"),
      "preview app codesign verification failed",
      "verification error",
    );
    assertIncludes(
      result.errors.join("\n"),
      "run npm run bundle:macos:verified first",
      "build hint",
    );
    assertDeepEqual(verified, [{ type: "app", path: appPath }], "verified target");
    assertDeepEqual(calls, [], "open command");
  });
});

test("dry-runs the selected preview artifact without opening it", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);
    const calls = [];
    const verified = [];

    const result = openMacosPreview({
      appPath,
      dryRun: true,
      platform: "darwin",
      verifyTarget: (target) => {
        verified.push(target);
        return [];
      },
      runCommand: (command, args) => {
        calls.push([command, args]);
        return { status: 0, stdout: "", stderr: "" };
      },
    });

    assertDeepEqual(result.errors, [], "errors");
    assertDeepEqual(verified, [{ type: "app", path: appPath }], "verified target");
    assertDeepEqual(calls, [], "open command");
  });
});

test("reports open command failures", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);

    const result = openMacosPreview({
      appPath,
      platform: "darwin",
      verifyTarget: () => [],
      runCommand: () => ({ status: 1, stdout: "", stderr: "Launch failed" }),
    });

    assertIncludes(result.errors.join("\n"), "open failed for apm.app", "open failure");
    assertIncludes(result.errors.join("\n"), "Launch failed", "open stderr");
  });
});

test("parses the DMG flag", () => {
  assertDeepEqual(
    optionsFromArgs(["--dmg"]),
    { dmg: true, dryRun: false, help: false, errors: [] },
    "bare flag",
  );
  assertDeepEqual(
    optionsFromArgs(["--dmg=false"]),
    { dmg: false, dryRun: false, help: false, errors: [] },
    "false flag",
  );
  assertDeepEqual(
    optionsFromArgs(["--dmg", "no"]),
    { dmg: false, dryRun: false, help: false, errors: [] },
    "separate false value",
  );
});

test("parses the dry-run flag", () => {
  assertDeepEqual(
    optionsFromArgs(["--dry-run"]),
    { dmg: false, dryRun: true, help: false, errors: [] },
    "bare dry-run flag",
  );
  assertDeepEqual(
    optionsFromArgs(["--dry-run=false"]),
    { dmg: false, dryRun: false, help: false, errors: [] },
    "false dry-run flag",
  );
  assertDeepEqual(
    optionsFromArgs(["--dry-run", "yes"]),
    { dmg: false, dryRun: true, help: false, errors: [] },
    "separate true dry-run value",
  );
});

test("parses help without selecting a launch target", () => {
  assertDeepEqual(
    optionsFromArgs(["--help"]),
    { dmg: false, dryRun: false, help: true, errors: [] },
    "help flag",
  );
});

test("rejects unknown preview open arguments", () => {
  const parsed = optionsFromArgs(["--bogus", "--dmg=maybe", "--dry-run=nah"]);

  assertEqual(parsed.dmg, false, "dmg flag");
  assertEqual(parsed.dryRun, false, "dry-run flag");
  assertEqual(parsed.help, false, "help flag");
  assertIncludes(parsed.errors.join("\n"), "unknown argument: --bogus", "unknown arg");
  assertIncludes(
    parsed.errors.join("\n"),
    "invalid boolean value for --dmg: maybe",
    "invalid boolean",
  );
  assertIncludes(
    parsed.errors.join("\n"),
    "invalid boolean value for --dry-run: nah",
    "invalid dry-run boolean",
  );
});

test("prints help without opening the preview artifact", () => {
  const logs = [];
  const errors = [];
  const status = runPreviewOpenCommand(["--help"], {
    log: (message) => logs.push(message),
    error: (message) => errors.push(message),
    openOptions: {
      runCommand: () => {
        throw new Error("open should not run for help");
      },
    },
  });

  assertEqual(status, 0, "exit status");
  assertIncludes(logs.join("\n"), "Usage:", "usage output");
  assertDeepEqual(errors, [], "errors");
});

test("dry-runs the preview open command without opening the artifact", () => {
  withTempDir((dir) => {
    const appPath = resolve(dir, "apm.app");
    mkdirSync(appPath);
    const logs = [];
    const errors = [];
    const status = runPreviewOpenCommand(["--dry-run"], {
      log: (message) => logs.push(message),
      error: (message) => errors.push(message),
      openOptions: {
        appPath,
        platform: "darwin",
        verifyTarget: () => [],
        runCommand: () => {
          throw new Error("open should not run for dry-run");
        },
      },
    });

    assertEqual(status, 0, "exit status");
    assertIncludes(logs.join("\n"), "Verified app preview", "dry-run log");
    assertDeepEqual(errors, [], "errors");
  });
});

test("fails unknown arguments without opening the preview artifact", () => {
  const logs = [];
  const errors = [];
  const status = runPreviewOpenCommand(["--bogus"], {
    log: (message) => logs.push(message),
    error: (message) => errors.push(message),
    openOptions: {
      runCommand: () => {
        throw new Error("open should not run for invalid args");
      },
    },
  });

  assertEqual(status, 1, "exit status");
  assertDeepEqual(logs, [], "logs");
  assertIncludes(errors.join("\n"), "unknown argument: --bogus", "unknown arg");
});

runTests();

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-preview-open-test-"));
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
      console.error(error instanceof Error ? error.message : String(error));
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
