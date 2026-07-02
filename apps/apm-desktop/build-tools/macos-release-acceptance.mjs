import {
  existsSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  argumentErrors,
  errorMessage,
  isMain,
  repoRoot,
  valueArg,
} from "./macos-release-github-common.mjs";
import {
  appZipPayloadErrors,
  normalizeVersion,
  releaseAssetNames,
  releaseDmgVersionErrors,
  verifyChecksumManifest,
  verifyReleaseEvidenceManifest,
} from "./macos-release-assets.mjs";
import {
  dmgArtifactErrors,
  releaseSignatureErrors,
  selectDmgArtifacts,
} from "./macos-verify.mjs";

const defaultArtifactsDir = resolve(repoRoot, "desktop-release");

if (isMain(import.meta.url)) {
  const status = runReleaseArtifactAcceptanceCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runReleaseArtifactAcceptanceCommand(argv = [], runtime = {}) {
  const log = runtime.log ?? console.log;
  const writeError = runtime.error ?? console.error;

  try {
    const options = optionsFromArgs(argv);
    if (options.help) {
      log(usage());
      return 0;
    }
    if (options.errors.length > 0) {
      writeFailure(writeError, "macOS release artifact acceptance failed:", options.errors);
      return 1;
    }

    const errors = releaseArtifactAcceptanceErrors(options);
    if (errors.length > 0) {
      writeFailure(writeError, "macOS release artifact acceptance failed:", errors);
      return 1;
    }

    log("macOS release artifact acceptance passed");
    return 0;
  } catch (error) {
    writeError(`macOS release artifact acceptance failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function releaseArtifactAcceptanceErrors(options = {}) {
  const inventory = releaseArtifactInventory(options);
  if (inventory.errors.length > 0) {
    return inventory.errors;
  }

  const extractRoot = mkdtempSync(resolve(tmpdir(), "apm-release-assets-"));
  try {
    const extract = run("ditto", ["-x", "-k", inventory.appZip, extractRoot]);
    if (extract.status !== 0) {
      return [`failed to extract app zip: ${extract.stderr || extract.stdout}`];
    }

    const extractedApp = resolve(extractRoot, "apm.app");
    return [
      ...appZipPayloadErrors(extractRoot, { expectedVersion: inventory.version }),
      ...dmgArtifactErrors(inventory.dmgs),
      ...releaseSignatureErrors(extractedApp, inventory.dmgs),
    ];
  } finally {
    rmSync(extractRoot, { recursive: true, force: true });
  }
}

export function releaseArtifactInventoryErrors(options = {}) {
  return releaseArtifactInventory(options).errors;
}

function releaseArtifactInventory(options = {}) {
  const version = normalizeVersion(options.version);
  const artifactsDir = resolve(options.artifactsDir ?? defaultArtifactsDir);
  const names = releaseAssetNames(version);
  const appZip = resolve(artifactsDir, names.appZip);
  const checksumManifest = resolve(artifactsDir, names.checksums);
  const evidenceManifest = resolve(artifactsDir, names.evidence);
  const errors = [];

  if (!directoryExists(artifactsDir)) {
    return {
      version,
      artifactsDir,
      appZip,
      checksumManifest,
      evidenceManifest,
      dmgs: [],
      errors: [`missing release artifact directory: ${artifactsDir}`],
    };
  }

  const entries = readdirSync(artifactsDir);
  const dmgs = selectDmgArtifacts(entries, artifactsDir);
  for (const [path, label] of [
    [appZip, "app zip"],
    [checksumManifest, "checksum manifest"],
    [evidenceManifest, "release evidence manifest"],
  ]) {
    if (!existsSync(path)) {
      errors.push(`missing ${label}: ${path}`);
    }
  }
  if (dmgs.length === 0) {
    errors.push(`missing apm DMG artifact under ${artifactsDir}`);
  }
  errors.push(...releaseArtifactDirectoryEntryErrors(entries, names, dmgs, artifactsDir));

  if (dmgs.length > 0) {
    errors.push(...releaseDmgVersionErrors(dmgs, version));
  }
  const dmgNames = dmgs.map((dmg) => basename(dmg));
  const checksumFilenames = [names.appZip, ...dmgNames];
  if (existsSync(checksumManifest)) {
    errors.push(
      ...verifyChecksumManifest(checksumManifest, artifactsDir, {
        expectedFilenames: checksumFilenames,
      }),
    );
  }
  if (existsSync(evidenceManifest)) {
    errors.push(
      ...verifyReleaseEvidenceManifest(evidenceManifest, artifactsDir, version, {
        expectedFilenames: [...checksumFilenames, names.checksums],
      }),
    );
  }

  return {
    version,
    artifactsDir,
    appZip,
    checksumManifest,
    evidenceManifest,
    dmgs,
    errors,
  };
}

function releaseArtifactDirectoryEntryErrors(entries, names, dmgs, artifactsDir) {
  const expected = new Set([
    names.appZip,
    names.checksums,
    names.evidence,
    ...dmgs.map((dmg) => basename(dmg)),
  ]);
  return entries
    .filter((entry) => !expected.has(entry))
    .map((entry) => `unexpected release artifact: ${resolve(artifactsDir, entry)}`);
}

function directoryExists(path) {
  return existsSync(path) && statSync(path).isDirectory();
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    valueArgs: ["--version", "--artifacts", "--artifacts-dir"],
    flagArgs: ["--help"],
  });

  return {
    help: argv.includes("--help"),
    errors,
    version: valueArg(argv, "--version"),
    artifactsDir: valueArg(argv, "--artifacts") ?? valueArg(argv, "--artifacts-dir"),
  };
}

function run(command, commandArgs) {
  return spawnSync(command, commandArgs, {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

function usage() {
  return [
    "Usage: npm run accept:macos:release -- [--version <version>] [--artifacts <dir>]",
    "",
    "Verifies a downloaded macOS desktop release artifact directory.",
    "",
    "Options:",
    "  --version <version>        Expected release version; defaults to package version",
    "  --artifacts <dir>          Artifact directory to verify",
    "  --artifacts-dir <dir>      Alias for --artifacts",
    "  --help                     Show this help without verifying artifacts",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
