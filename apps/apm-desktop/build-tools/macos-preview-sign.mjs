import { existsSync, readdirSync, renameSync, rmSync, statSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  argumentErrors,
  errorMessage,
  isMain,
  repoRoot,
  valueArg,
} from "./macos-release-github-common.mjs";
import { selectDmgArtifacts, strictCodeSignatureErrors } from "./macos-verify.mjs";

const defaultBundleRoot = resolve(repoRoot, "target/release/bundle");
const defaultMacosBundleDir = resolve(defaultBundleRoot, "macos");
const defaultDmgDir = resolve(defaultBundleRoot, "dmg");
const defaultAppPath = resolve(defaultMacosBundleDir, "apm.app");
const defaultBundleDmgScript = resolve(defaultDmgDir, "bundle_dmg.sh");
const defaultDmgRebuildAttempts = 2;
const defaultDmgRebuildTimeoutMs = 120_000;

if (isMain(import.meta.url)) {
  main(process.argv.slice(2));
}

function main(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--skip-dmg"],
    valueArgs: ["--app"],
  });
  if (errors.length > 0) {
    console.error(`macOS preview app signing failed: ${errors.join("\n")}`);
    process.exit(1);
  }

  const appBundlePath = valueArg(argv, "--app") ?? defaultAppPath;
  const skipDmg = argv.includes("--skip-dmg");
  try {
    preparePreviewArtifacts({ appBundlePath, skipDmg });
    console.log(`macOS preview app ad-hoc signature passed: ${appBundlePath}`);
  } catch (error) {
    console.error(`macOS preview app signing failed: ${errorMessage(error)}`);
    process.exit(1);
  }
}

export function preparePreviewArtifacts(options = {}) {
  const appBundlePath = options.appBundlePath ?? defaultAppPath;
  const dmgDir = options.dmgDir ?? defaultDmgDir;
  const runCommand = options.runCommand ?? run;
  signPreviewAppBundle(appBundlePath, { runCommand });
  if (!options.skipDmg) {
    rebuildPreviewDmgArtifacts({
      dmgDir,
      macosBundleDir: options.macosBundleDir ?? dirname(appBundlePath),
      bundleDmgScript: options.bundleDmgScript ?? defaultBundleDmgScript,
      rebuildAttempts: options.rebuildAttempts,
      rebuildTimeoutMs: options.rebuildTimeoutMs,
      tempToken: options.tempToken,
      runCommand,
    });
  }
}

export function signPreviewAppBundle(appBundlePath = defaultAppPath, options = {}) {
  const runCommand = options.runCommand ?? run;
  if (!directoryExists(appBundlePath)) {
    throw new Error(`missing app bundle: ${appBundlePath}`);
  }

  const signing = runCommand("codesign", [
    "--force",
    "--deep",
    "--sign",
    "-",
    appBundlePath,
  ]);
  if (signing.status !== 0) {
    throw new Error(signing.stderr || signing.stdout || "codesign failed");
  }

  const verificationErrors = strictCodeSignatureErrors(appBundlePath, {
    label: "preview app",
    runCommand,
  });
  if (verificationErrors.length > 0) {
    throw new Error(verificationErrors.join("\n"));
  }
}

export function rebuildPreviewDmgArtifacts(options = {}) {
  const dmgDir = options.dmgDir ?? defaultDmgDir;
  if (!directoryExists(dmgDir)) {
    return;
  }

  const dmgs = selectDmgArtifacts(readdirSync(dmgDir), dmgDir);
  if (dmgs.length === 0) {
    return;
  }

  const macosBundleDir = options.macosBundleDir ?? defaultMacosBundleDir;
  const bundleDmgScript = options.bundleDmgScript ?? defaultBundleDmgScript;
  const rebuildAttempts = previewDmgRebuildAttempts(options.rebuildAttempts);
  const rebuildTimeoutMs = previewDmgRebuildTimeoutMs(options.rebuildTimeoutMs);
  const runCommand = options.runCommand ?? run;
  if (!existsSync(bundleDmgScript)) {
    throw new Error(`missing preview DMG builder: ${bundleDmgScript}`);
  }
  if (!directoryExists(macosBundleDir)) {
    throw new Error(`missing macOS bundle directory: ${macosBundleDir}`);
  }

  for (const dmg of dmgs) {
    const tempDmg = temporaryDmgPath(dmg, options.tempToken);
    const rebuilding = rebuildPreviewDmg({
      bundleDmgScript,
      tempDmg,
      macosBundleDir,
      rebuildAttempts,
      rebuildTimeoutMs,
      runCommand,
    });
    if (rebuilding.status !== 0) {
      removePreviewDmgTemporaryFiles(tempDmg);
      throw new Error(
        `could not rebuild preview DMG ${basename(dmg)}: ` +
          `${previewDmgRebuildFailureMessage(rebuilding, rebuildTimeoutMs)}`,
      );
    }
    renameSync(tempDmg, dmg);
    removePreviewDmgTemporaryFiles(tempDmg);
  }
}

function rebuildPreviewDmg({
  bundleDmgScript,
  tempDmg,
  macosBundleDir,
  rebuildAttempts,
  rebuildTimeoutMs,
  runCommand,
}) {
  let result = {
    status: 1,
    stdout: "",
    stderr: "preview DMG rebuild was not attempted",
  };
  for (let attempt = 1; attempt <= rebuildAttempts; attempt += 1) {
    removePreviewDmgTemporaryFiles(tempDmg);
    result = runCommand(
      "bash",
      previewDmgBuildArgs({
        bundleDmgScript,
        tempDmg,
        macosBundleDir,
      }),
      { timeout: rebuildTimeoutMs, killSignal: "SIGTERM" },
    );
    if (result.status === 0) {
      return result;
    }
  }
  return result;
}

function previewDmgRebuildAttempts(value) {
  if (value === undefined) {
    return defaultDmgRebuildAttempts;
  }
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`invalid preview DMG rebuild attempts: ${value}`);
  }
  return value;
}

function previewDmgRebuildTimeoutMs(value) {
  if (value === undefined) {
    return defaultDmgRebuildTimeoutMs;
  }
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`invalid preview DMG rebuild timeout: ${value}`);
  }
  return value;
}

function previewDmgRebuildFailureMessage(result, timeoutMs) {
  if (result.error?.code === "ETIMEDOUT") {
    return `bundle_dmg.sh timed out after ${timeoutMs}ms`;
  }
  return result.stderr || result.stdout || "bundle_dmg.sh failed";
}

function previewDmgBuildArgs({ bundleDmgScript, tempDmg, macosBundleDir }) {
  return [
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
    macosBundleDir,
  ];
}

function temporaryDmgPath(dmg, token = process.pid) {
  return resolve(dirname(dmg), `.tmp-${token}-${basename(dmg)}`);
}

function removePreviewDmgTemporaryFiles(tempDmg) {
  rmSync(tempDmg, { force: true });
  for (const name of readdirSync(dirname(tempDmg))) {
    if (name.startsWith("rw.") && name.endsWith(`.${basename(tempDmg)}`)) {
      rmSync(resolve(dirname(tempDmg), name), { force: true });
    }
  }
}

function directoryExists(path) {
  return existsSync(path) && statSync(path).isDirectory();
}

function run(command, commandArgs, options = {}) {
  return spawnSync(command, commandArgs, {
    cwd: repoRoot,
    encoding: "utf8",
    killSignal: options.killSignal,
    timeout: options.timeout,
  });
}
