import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  argumentErrors,
  desktopPackageJsonPath,
  desktopRoot,
  errorMessage,
  isMain,
  repoRoot,
} from "./macos-release-github-common.mjs";

const desktopReleaseRoot = resolve(repoRoot, "desktop-release");

if (isMain(import.meta.url)) {
  const status = runV3LocalCheckpointCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runV3LocalCheckpointCommand(argv, runtime = {}) {
  const writeError = runtime.error ?? console.error;
  try {
    const options = optionsFromArgs(argv);
    if (options.help) {
      (runtime.log ?? console.log)(usage());
      return 0;
    }
    if (options.errors.length > 0) {
      writeFailure(writeError, options.errors);
      return 1;
    }
    runV3LocalCheckpoint({
      log: runtime.log ?? console.log,
      ...(runtime.checkpointOptions ?? {}),
    });
    return 0;
  } catch (error) {
    writeError(`v3 local checkpoint failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function runV3LocalCheckpoint(options = {}) {
  const runCommand = options.runCommand ?? runStepCommand;
  const log = options.log ?? console.log;
  const supportErrors = v3LocalCheckpointSupportErrors(options);
  if (supportErrors.length > 0) {
    throw new Error(
      ["v3 local checkpoint support failed:", ...supportErrors.map((error) => `- ${error}`)]
        .join("\n"),
    );
  }

  for (const step of v3LocalCheckpointSteps(options)) {
    log(`\n==> ${step.label}`);
    const result = runCommand(step.command, step.args, { cwd: step.cwd });
    if (result.status !== 0) {
      throw new Error(
        `${step.label} failed: ${result.stderr || result.stdout || "command exited non-zero"}`,
      );
    }
  }
  log("\n==> untracked file whitespace check");
  const untrackedWhitespaceErrors = untrackedFileWhitespaceErrors(options);
  if (untrackedWhitespaceErrors.length > 0) {
    throw new Error(
      [
        "untracked file whitespace check failed:",
        ...untrackedWhitespaceErrors.map((error) => `- ${error}`),
      ].join("\n"),
    );
  }
  log("\nv3 local checkpoint passed");
}

export function v3LocalCheckpointSupportErrors(options = {}) {
  const root = options.desktopRoot ?? desktopRoot;
  const packageJson =
    options.desktopPackage ?? readJson(options.desktopPackageJsonPath ?? desktopPackageJsonPath);
  const scripts = packageJson.scripts ?? {};
  const errors = [];

  for (const relativePath of requiredV3LocalCheckpointSupportFiles()) {
    const path = resolve(root, relativePath);
    if (!existsSync(path)) {
      errors.push(`missing v3 local checkpoint support file: ${path}`);
    }
  }

  for (const script of requiredV3LocalCheckpointPackageScripts()) {
    if (!scripts[script]) {
      errors.push(`desktop package.json must define script ${script}`);
    }
  }

  return errors;
}

export function requiredV3LocalCheckpointSupportFiles() {
  return [
    "build-tools/v3-local-checkpoint.mjs",
    "build-tools/v3-local-checkpoint.test.mjs",
    "build-tools/macos-preview-bundle.mjs",
    "build-tools/macos-preview-bundle.test.mjs",
    "build-tools/macos-preview-sign.mjs",
    "build-tools/macos-preview-sign.test.mjs",
    "build-tools/macos-preview-open.mjs",
    "build-tools/macos-preview-open.test.mjs",
    "build-tools/macos-verify.mjs",
    "build-tools/macos-release-assets.mjs",
  ];
}

export function requiredV3LocalCheckpointPackageScripts() {
  return [
    "verify:v3:local",
    "release:macos:check",
    "bundle:macos:verified",
    "bundle:macos",
    "sign:macos:preview",
    "verify:macos:preview:dmg",
    "open:macos:preview",
    "open:macos:preview:dmg",
  ];
}

export function v3LocalCheckpointSteps(options = {}) {
  const version = options.version ?? desktopPackageVersion(options.desktopPackageJsonPath);
  return [
    {
      label: "cargo workspace tests",
      command: "cargo",
      args: ["test", "--workspace"],
      cwd: repoRoot,
    },
    {
      label: "desktop release preflight",
      command: "npm",
      args: ["run", "release:macos:check"],
      cwd: desktopRoot,
    },
    {
      label: "desktop verified preview bundle",
      command: "npm",
      args: ["run", "bundle:macos:verified"],
      cwd: desktopRoot,
    },
    {
      label: "desktop preview app launch smoke",
      command: "npm",
      args: ["run", "open:macos:preview", "--", "--dry-run"],
      cwd: desktopRoot,
    },
    {
      label: "desktop preview DMG launch smoke",
      command: "npm",
      args: ["run", "open:macos:preview:dmg", "--", "--dry-run"],
      cwd: desktopRoot,
    },
    {
      label: "desktop release asset evidence",
      command: "node",
      args: ["build-tools/macos-release-assets.mjs", "--version", version],
      cwd: desktopRoot,
    },
    {
      label: "desktop release checksum manifest",
      command: "shasum",
      args: ["-a", "256", "-c", `apm-${version}-desktop.sha256`],
      cwd: desktopReleaseRoot,
    },
    {
      label: "git diff whitespace check",
      command: "git",
      args: ["diff", "--check"],
      cwd: repoRoot,
    },
  ];
}

export function untrackedFileWhitespaceErrors(options = {}) {
  const runCommand = options.runCommand ?? runCapturedCommand;
  const listResult = runCommand(
    "git",
    ["ls-files", "--others", "--exclude-standard"],
    { cwd: repoRoot },
  );
  if (listResult.status !== 0) {
    return [
      `git ls-files for untracked files failed: ${
        commandOutput(listResult) || "command exited non-zero"
      }`,
    ];
  }

  return (listResult.stdout ?? "")
    .split(/\r?\n/)
    .filter(Boolean)
    .flatMap((path) => untrackedFileWhitespaceError(path, runCommand));
}

function desktopPackageVersion(path = desktopPackageJsonPath) {
  const version = readJson(path).version;
  if (!version) {
    throw new Error(`desktop package version is required: ${path}`);
  }
  return version;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function runStepCommand(command, args, options) {
  return spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    stdio: "inherit",
  });
}

function runCapturedCommand(command, args, options) {
  return spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
  });
}

function untrackedFileWhitespaceError(path, runCommand) {
  const result = runCommand(
    "git",
    ["diff", "--no-index", "--check", "--", "/dev/null", path],
    { cwd: repoRoot },
  );
  const output = commandOutput(result);
  if (output || result.status > 1) {
    return [output || `${path}: git diff --no-index --check exited ${result.status}`];
  }
  return [];
}

function commandOutput(result) {
  return [result.stdout, result.stderr]
    .filter(Boolean)
    .join("\n")
    .trim();
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--help"],
  });
  return {
    help: argv.includes("--help"),
    errors,
  };
}

function usage() {
  return [
    "Usage: npm run verify:v3:local -- [--help]",
    "",
    "Runs the local v3.0 desktop checkpoint without dispatching public release work.",
    "",
    "Options:",
    "  --help  Show this help without running the checkpoint",
  ].join("\n");
}

function writeFailure(writeError, errors) {
  writeError("v3 local checkpoint argument validation failed:");
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
