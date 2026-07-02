import { spawnSync } from "node:child_process";
import {
  argumentErrors,
  desktopRoot,
  errorMessage,
  isMain,
  valueArg,
} from "./macos-release-github-common.mjs";

const defaultPreviewBundleTimeoutMs = 600_000;

if (isMain(import.meta.url)) {
  const status = runPreviewBundleCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runPreviewBundleCommand(argv, runtime = {}) {
  const log = runtime.log ?? console.log;
  const writeError = runtime.error ?? console.error;
  const options = optionsFromArgs(argv);

  if (options.help) {
    log(usage());
    return 0;
  }
  if (options.errors.length > 0) {
    writeFailure(writeError, options.errors);
    return 1;
  }

  try {
    const result = buildMacosPreviewBundle({
      ...(runtime.bundleOptions ?? {}),
      timeoutMs: options.timeoutMs,
    });
    if (result.status !== 0) {
      writeError(
        `macOS preview bundle failed: ${previewBundleFailureMessage(result, options.timeoutMs)}`,
      );
      return 1;
    }
    log("macOS preview bundle completed");
    return 0;
  } catch (error) {
    writeError(`macOS preview bundle failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function buildMacosPreviewBundle(options = {}) {
  const runCommand = options.runCommand ?? run;
  const timeoutMs = previewBundleTimeoutMs(options.timeoutMs);
  return runCommand("npx", ["tauri", "build", "--bundles", "app,dmg"], {
    cwd: options.cwd ?? desktopRoot,
    killSignal: "SIGTERM",
    stdio: "inherit",
    timeout: timeoutMs,
  });
}

export function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--help"],
    valueArgs: ["--timeout-ms"],
  });
  const timeoutText = valueArg(argv, "--timeout-ms");
  const timeoutMs = previewBundleTimeoutMs(timeoutText, errors);
  return {
    errors,
    help: argv.includes("--help"),
    timeoutMs,
  };
}

export function previewBundleFailureMessage(result, timeoutMs) {
  if (result.error?.code === "ETIMEDOUT") {
    return `tauri preview bundle timed out after ${timeoutMs}ms`;
  }
  return (
    result.stderr ||
    result.stdout ||
    (result.error ? errorMessage(result.error) : "tauri build failed")
  );
}

function previewBundleTimeoutMs(value, errors = []) {
  if (value === undefined) {
    return defaultPreviewBundleTimeoutMs;
  }

  const timeoutMs = Number(value);
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1) {
    errors.push(`--timeout-ms must be a positive integer: ${value}`);
    return defaultPreviewBundleTimeoutMs;
  }
  return timeoutMs;
}

function usage() {
  return [
    "Usage: npm run bundle:macos -- [--timeout-ms <ms>]",
    "",
    "Builds local macOS preview app and DMG artifacts with a bounded Tauri build.",
    "",
    "Options:",
    "  --timeout-ms <ms>  Maximum preview bundle build time before failing clearly",
    "  --help             Show this help without running Tauri",
  ].join("\n");
}

function writeFailure(writeError, errors) {
  writeError("macOS preview bundle argument validation failed:");
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    cwd: options.cwd,
    killSignal: options.killSignal,
    stdio: options.stdio,
    timeout: options.timeout,
  });
}
