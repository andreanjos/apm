import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import {
  requiredV3LocalCheckpointPackageScripts,
  requiredV3LocalCheckpointSupportFiles,
  runV3LocalCheckpoint,
  runV3LocalCheckpointCommand,
  untrackedFileWhitespaceErrors,
  v3LocalCheckpointSupportErrors,
  v3LocalCheckpointSteps,
} from "./v3-local-checkpoint.mjs";
import { errorMessage } from "./macos-release-github-common.mjs";

const tests = [];

test("builds the canonical v3 local checkpoint step list", () => {
  const steps = v3LocalCheckpointSteps({ version: "0.1.1" });

  assertDeepEqual(
    steps.map((step) => [step.label, step.command, step.args]),
    [
      ["cargo workspace tests", "cargo", ["test", "--workspace"]],
      ["desktop release preflight", "npm", ["run", "release:macos:check"]],
      ["desktop verified preview bundle", "npm", ["run", "bundle:macos:verified"]],
      [
        "desktop preview app launch smoke",
        "npm",
        ["run", "open:macos:preview", "--", "--dry-run"],
      ],
      [
        "desktop preview DMG launch smoke",
        "npm",
        ["run", "open:macos:preview:dmg", "--", "--dry-run"],
      ],
      [
        "desktop release asset evidence",
        "node",
        ["build-tools/macos-release-assets.mjs", "--version", "0.1.1"],
      ],
      [
        "desktop release checksum manifest",
        "shasum",
        ["-a", "256", "-c", "apm-0.1.1-desktop.sha256"],
      ],
      ["git diff whitespace check", "git", ["diff", "--check"]],
    ],
    "checkpoint steps",
  );
});

test("reads the desktop package version for release evidence checks", () => {
  withTempDir((dir) => {
    const packageJson = resolve(dir, "package.json");
    writeFileSync(packageJson, `${JSON.stringify({ version: "0.2.3" })}\n`);

    const steps = v3LocalCheckpointSteps({ desktopPackageJsonPath: packageJson });

    assertDeepEqual(
      steps.find((step) => step.label === "desktop release asset evidence").args,
      ["build-tools/macos-release-assets.mjs", "--version", "0.2.3"],
      "release asset args",
    );
    assertDeepEqual(
      steps.find((step) => step.label === "desktop release checksum manifest").args,
      ["-a", "256", "-c", "apm-0.2.3-desktop.sha256"],
      "checksum args",
    );
  });
});

test("accepts complete v3 local checkpoint support files and package scripts", () => {
  withTempDir((dir) => {
    writeCheckpointSupportFiles(dir);

    assertDeepEqual(
      v3LocalCheckpointSupportErrors({
        desktopRoot: dir,
        desktopPackage: checkpointPackageJson(),
      }),
      [],
      "checkpoint support errors",
    );
  });
});

test("rejects missing v3 local checkpoint support files and package scripts", () => {
  withTempDir((dir) => {
    writeCheckpointSupportFiles(dir, {
      omit: [
        "build-tools/v3-local-checkpoint.test.mjs",
        "build-tools/macos-preview-bundle.mjs",
        "build-tools/macos-preview-bundle.test.mjs",
        "build-tools/macos-preview-sign.mjs",
        "build-tools/macos-preview-sign.test.mjs",
        "build-tools/macos-preview-open.mjs",
        "build-tools/macos-preview-open.test.mjs",
      ],
    });
    const scripts = { ...checkpointPackageJson().scripts };
    delete scripts["sign:macos:preview"];
    delete scripts["open:macos:preview"];
    delete scripts["open:macos:preview:dmg"];

    const errors = v3LocalCheckpointSupportErrors({
      desktopRoot: dir,
      desktopPackage: { scripts },
    }).join("\n");

    assertIncludes(errors, "v3-local-checkpoint.test.mjs", "checkpoint test file");
    assertIncludes(errors, "macos-preview-bundle.mjs", "preview bundle support file");
    assertIncludes(errors, "macos-preview-bundle.test.mjs", "preview bundle test file");
    assertIncludes(errors, "macos-preview-sign.mjs", "preview signer support file");
    assertIncludes(errors, "macos-preview-sign.test.mjs", "preview signer test file");
    assertIncludes(errors, "macos-preview-open.mjs", "preview open support file");
    assertIncludes(errors, "macos-preview-open.test.mjs", "preview open test file");
    assertIncludes(errors, "sign:macos:preview", "preview signer script");
    assertIncludes(errors, "open:macos:preview", "preview open script");
    assertIncludes(errors, "open:macos:preview:dmg", "preview DMG open script");
  });
});

test("stops the checkpoint on the first failing step", () => {
  const calls = [];
  assertThrows(
    () =>
      runV3LocalCheckpoint({
        version: "0.1.1",
        log: () => {},
        runCommand(command, args) {
          calls.push([command, args]);
          return calls.length === 2
            ? { status: 1, stdout: "", stderr: "preflight failed" }
            : { status: 0, stdout: "", stderr: "" };
        },
      }),
    "preflight failed",
    "checkpoint failure",
  );

  assertEqual(calls.length, 2, "call count");
});

test("runs the untracked whitespace check after command steps", () => {
  const calls = [];
  const logs = [];

  runV3LocalCheckpoint({
    version: "0.1.1",
    log: (line) => logs.push(line),
    runCommand(command, args) {
      calls.push([command, args]);
      if (command === "git" && args.join(" ") === "ls-files --others --exclude-standard") {
        return { status: 0, stdout: "apps/apm-desktop/src/main.ts\n", stderr: "" };
      }
      if (
        command === "git" &&
        args.join(" ") ===
          "diff --no-index --check -- /dev/null apps/apm-desktop/src/main.ts"
      ) {
        return { status: 1, stdout: "", stderr: "" };
      }
      return { status: 0, stdout: "", stderr: "" };
    },
  });

  assertEqual(
    calls.some(
      ([command, args]) =>
        command === "git" && args.join(" ") === "ls-files --others --exclude-standard",
    ),
    true,
    "untracked file discovery ran",
  );
  assertEqual(
    calls.some(
      ([command, args]) =>
        command === "git" &&
        args.join(" ") ===
          "diff --no-index --check -- /dev/null apps/apm-desktop/src/main.ts",
    ),
    true,
    "untracked whitespace diff ran",
  );
  assertIncludes(
    logs.join("\n"),
    "==> untracked file whitespace check",
    "untracked whitespace step log",
  );
});

test("reports whitespace errors in untracked files", () => {
  const errors = untrackedFileWhitespaceErrors({
    runCommand(command, args) {
      if (command === "git" && args.join(" ") === "ls-files --others --exclude-standard") {
        return {
          status: 0,
          stdout: "apps/apm-desktop/src/main.ts\napps/apm-desktop/src-tauri/icons/icon.png\n",
          stderr: "",
        };
      }
      if (
        command === "git" &&
        args.join(" ") ===
          "diff --no-index --check -- /dev/null apps/apm-desktop/src/main.ts"
      ) {
        return {
          status: 3,
          stdout: "apps/apm-desktop/src/main.ts:1: trailing whitespace.",
          stderr: "",
        };
      }
      if (
        command === "git" &&
        args.join(" ") ===
          "diff --no-index --check -- /dev/null apps/apm-desktop/src-tauri/icons/icon.png"
      ) {
        return { status: 1, stdout: "", stderr: "" };
      }
      return { status: 1, stdout: "", stderr: `unexpected command: ${command} ${args.join(" ")}` };
    },
  });

  assertDeepEqual(
    errors,
    ["apps/apm-desktop/src/main.ts:1: trailing whitespace."],
    "untracked whitespace errors",
  );
});

test("reports untracked file discovery failures", () => {
  const errors = untrackedFileWhitespaceErrors({
    runCommand(command, args) {
      if (command === "git" && args.join(" ") === "ls-files --others --exclude-standard") {
        return { status: 128, stdout: "", stderr: "fatal: not a git repository" };
      }
      return { status: 1, stdout: "", stderr: `unexpected command: ${command} ${args.join(" ")}` };
    },
  });

  assertIncludes(
    errors.join("\n"),
    "git ls-files for untracked files failed: fatal: not a git repository",
    "git ls-files failure",
  );
});

test("prints checkpoint help without running support checks", () => {
  const logs = [];
  const errors = [];
  const status = runV3LocalCheckpointCommand(["--help"], {
    log: (line) => logs.push(line),
    error: (line) => errors.push(line),
    checkpointOptions: {
      desktopRoot: "/missing-desktop-root",
    },
  });

  assertEqual(status, 0, "exit status");
  assertIncludes(logs.join("\n"), "Usage: npm run verify:v3:local", "usage output");
  assertDeepEqual(errors, [], "error output");
});

test("rejects unknown checkpoint arguments before running support checks", () => {
  const logs = [];
  const errors = [];
  const status = runV3LocalCheckpointCommand(["--bogus", "--help=false"], {
    log: (line) => logs.push(line),
    error: (line) => errors.push(line),
    checkpointOptions: {
      desktopRoot: "/missing-desktop-root",
    },
  });

  assertEqual(status, 1, "exit status");
  assertIncludes(errors.join("\n"), "unknown argument: --bogus", "unknown argument");
  assertIncludes(errors.join("\n"), "--help does not accept a value", "help flag value");
  assertDeepEqual(logs, [], "log output");
});

runTests();

function checkpointPackageJson() {
  return {
    scripts: Object.fromEntries(
      requiredV3LocalCheckpointPackageScripts().map((script) => [script, `echo ${script}`]),
    ),
  };
}

function writeCheckpointSupportFiles(root, options = {}) {
  const omit = new Set(options.omit ?? []);
  for (const relativePath of requiredV3LocalCheckpointSupportFiles()) {
    if (!omit.has(relativePath)) {
      writeFile(resolve(root, relativePath), "\n");
    }
  }
}

function writeFile(path, contents) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
}

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-v3-checkpoint-test-"));
  try {
    mkdirSync(dir, { recursive: true });
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

function assertThrows(run, expected, message) {
  try {
    run();
  } catch (error) {
    if (!errorMessage(error).includes(expected)) {
      throw new Error(`${message}: expected ${expected}, got ${errorMessage(error)}`);
    }
    return;
  }
  throw new Error(`${message}: expected function to throw`);
}
