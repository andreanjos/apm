import {
  buildMacosPreviewBundle,
  optionsFromArgs,
  previewBundleFailureMessage,
  runPreviewBundleCommand,
} from "./macos-preview-bundle.mjs";
import { desktopRoot, errorMessage } from "./macos-release-github-common.mjs";

const tests = [];

test("builds local preview bundles through Tauri with a bounded timeout", () => {
  const calls = [];

  const result = buildMacosPreviewBundle({
    timeoutMs: 42,
    runCommand(command, args, options) {
      calls.push([command, args, options]);
      return { status: 0, stdout: "", stderr: "" };
    },
  });

  assertEqual(result.status, 0, "status");
  assertDeepEqual(
    calls,
    [
      [
        "npx",
        ["tauri", "build", "--bundles", "app,dmg"],
        {
          cwd: desktopRoot,
          killSignal: "SIGTERM",
          stdio: "inherit",
          timeout: 42,
        },
      ],
    ],
    "Tauri build call",
  );
});

test("parses preview bundle timeout arguments", () => {
  assertDeepEqual(
    optionsFromArgs(["--timeout-ms", "123"]),
    { errors: [], help: false, timeoutMs: 123 },
    "separated timeout",
  );
  assertDeepEqual(
    optionsFromArgs(["--timeout-ms=456"]),
    { errors: [], help: false, timeoutMs: 456 },
    "inline timeout",
  );
});

test("rejects invalid preview bundle arguments before running Tauri", () => {
  const parsed = optionsFromArgs(["--bogus", "--timeout-ms=0"]);

  assertEqual(parsed.help, false, "help flag");
  assertIncludes(parsed.errors.join("\n"), "unknown argument: --bogus", "unknown arg");
  assertIncludes(
    parsed.errors.join("\n"),
    "--timeout-ms must be a positive integer: 0",
    "invalid timeout",
  );
});

test("prints preview bundle help without running Tauri", () => {
  const logs = [];
  const errors = [];
  const status = runPreviewBundleCommand(["--help"], {
    log: (line) => logs.push(line),
    error: (line) => errors.push(line),
    bundleOptions: {
      runCommand() {
        throw new Error("Tauri should not run for help");
      },
    },
  });

  assertEqual(status, 0, "exit status");
  assertIncludes(logs.join("\n"), "Usage: npm run bundle:macos", "usage");
  assertIncludes(logs.join("\n"), "without running Tauri", "help safety");
  assertDeepEqual(errors, [], "errors");
});

test("fails invalid preview bundle arguments without running Tauri", () => {
  const logs = [];
  const errors = [];
  const status = runPreviewBundleCommand(["--timeout-ms=-1"], {
    log: (line) => logs.push(line),
    error: (line) => errors.push(line),
    bundleOptions: {
      runCommand() {
        throw new Error("Tauri should not run for invalid args");
      },
    },
  });

  assertEqual(status, 1, "exit status");
  assertIncludes(errors.join("\n"), "--timeout-ms must be a positive integer", "error output");
  assertDeepEqual(logs, [], "logs");
});

test("reports preview bundle timeouts clearly", () => {
  const message = previewBundleFailureMessage(
    {
      status: null,
      stdout: "",
      stderr: "",
      error: { code: "ETIMEDOUT" },
    },
    99,
  );

  assertEqual(message, "tauri preview bundle timed out after 99ms", "timeout message");
});

test("reports Tauri preview bundle stderr failures", () => {
  const logs = [];
  const errors = [];
  const status = runPreviewBundleCommand(["--timeout-ms", "88"], {
    log: (line) => logs.push(line),
    error: (line) => errors.push(line),
    bundleOptions: {
      runCommand() {
        return { status: 1, stdout: "", stderr: "DMG builder failed" };
      },
    },
  });

  assertEqual(status, 1, "exit status");
  assertIncludes(errors.join("\n"), "DMG builder failed", "stderr");
  assertDeepEqual(logs, [], "logs");
});

runTests();

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
