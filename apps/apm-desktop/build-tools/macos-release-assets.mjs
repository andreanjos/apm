import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { basename, dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  argumentErrors,
  errorMessage,
  isMain,
  repoRoot,
  valueArg,
} from "./macos-release-github-common.mjs";
import {
  appBundleInfoPlistValueErrors,
  appBundlePayloadErrors,
  selectDmgArtifacts,
} from "./macos-verify.mjs";

const appPath = resolve(repoRoot, "target/release/bundle/macos/apm.app");
const dmgDir = resolve(repoRoot, "target/release/bundle/dmg");
const defaultOutputDir = resolve(repoRoot, "desktop-release");
const requiredReleaseEvidenceChecks = {
  app_zip_payload: "verified",
  dmg_payload: "verified",
  checksum_manifest: "verified",
};

if (isMain(import.meta.url)) {
  main();
}

function main() {
  try {
    const options = optionsFromArgs(process.argv.slice(2));
    if (options.help) {
      console.log(usage());
      return;
    }
    packageDesktopReleaseAssets(options);
  } catch (error) {
    console.error(`desktop release asset packaging failed: ${errorMessage(error)}`);
    process.exit(1);
  }
}

export function packageDesktopReleaseAssets(options = {}) {
  const version = normalizeVersion(options.version);
  const outputDir = safeOutputDir(options.outputDir ?? defaultOutputDir);
  const names = releaseAssetNames(version);
  const appZip = resolve(outputDir, names.appZip);

  if (!directoryExists(appPath)) {
    throw new Error(`missing app bundle: ${appPath}`);
  }
  if (!directoryExists(dmgDir)) {
    throw new Error(`missing DMG output directory: ${dmgDir}`);
  }

  const dmgs = selectDmgArtifacts(readdirSync(dmgDir), dmgDir);
  if (dmgs.length === 0) {
    throw new Error(`missing apm DMG artifact under ${dmgDir}`);
  }

  const dmgVersionErrors = releaseDmgVersionErrors(dmgs, version);
  if (dmgVersionErrors.length > 0) {
    throw new Error(dmgVersionErrors.join("\n"));
  }

  const appVersionErrors = appBundleReleaseVersionErrors(appPath, version, "release app");
  if (appVersionErrors.length > 0) {
    throw new Error(appVersionErrors.join("\n"));
  }

  rmSync(outputDir, { recursive: true, force: true });
  mkdirSync(outputDir, { recursive: true });

  run("ditto", ["-c", "-k", "--keepParent", appPath, appZip]);
  verifyAppZip(appZip, version);
  const copiedDmgs = dmgs.map((dmg) => {
    const target = resolve(outputDir, basename(dmg));
    copyFileSync(dmg, target);
    return target;
  });

  const manifestPath = resolve(outputDir, names.checksums);
  writeFileSync(manifestPath, checksumManifestText([appZip, ...copiedDmgs]));
  const checksumFilenames = [basename(appZip), ...copiedDmgs.map((dmg) => basename(dmg))];
  const checksumErrors = verifyChecksumManifest(manifestPath, outputDir, {
    expectedFilenames: checksumFilenames,
  });
  if (checksumErrors.length > 0) {
    throw new Error(checksumErrors.join("\n"));
  }

  const evidencePath = resolve(outputDir, names.evidence);
  writeFileSync(
    evidencePath,
    `${JSON.stringify(
      releaseEvidenceManifest({
        version,
        appZip,
        dmgs: copiedDmgs,
        checksumManifest: manifestPath,
      }),
      null,
      2,
    )}\n`,
  );
  const evidenceErrors = verifyReleaseEvidenceManifest(
    evidencePath,
    outputDir,
    version,
    { expectedFilenames: [...checksumFilenames, basename(manifestPath)] },
  );
  if (evidenceErrors.length > 0) {
    throw new Error(evidenceErrors.join("\n"));
  }

  console.log(`wrote desktop release assets: ${outputDir}`);
  for (const asset of [appZip, ...copiedDmgs, manifestPath, evidencePath]) {
    console.log(`- ${asset}`);
  }
}

export function releaseAssetNames(version) {
  const normalized = normalizeVersion(version);
  return {
    appZip: `apm-${normalized}-macos-app.zip`,
    checksums: `apm-${normalized}-desktop.sha256`,
    evidence: `apm-${normalized}-desktop-release-evidence.json`,
  };
}

export function normalizeVersion(version) {
  const normalized = `${version ?? ""}`.trim().replace(/^v/, "");
  if (!normalized) {
    throw new Error("--version is required");
  }
  return normalized;
}

export function checksumManifestText(paths) {
  return `${paths
    .map((path) => `${fileSha256(path)}  ${basename(path)}`)
    .join("\n")}\n`;
}

export function verifyChecksumManifest(manifestPath, directory, options = {}) {
  const errors = [];
  const filenames = [];
  const raw = readFileSync(manifestPath, "utf8");
  for (const [index, line] of raw.split(/\r?\n/).entries()) {
    if (!line.trim()) {
      continue;
    }
    const entry = parseChecksumLine(line);
    if (!entry) {
      errors.push(`invalid checksum line ${index + 1}: ${line}`);
      continue;
    }
    if (basename(entry.file) !== entry.file) {
      errors.push(`checksum entry must be a release artifact filename: ${entry.file}`);
      continue;
    }
    filenames.push(entry.file);
    const filePath = resolve(directory, entry.file);
    if (!existsSync(filePath)) {
      errors.push(`checksum references missing file: ${entry.file}`);
      continue;
    }
    const actual = fileSha256(filePath);
    if (actual !== entry.sha256) {
      errors.push(`checksum mismatch for ${entry.file}: expected ${entry.sha256}, got ${actual}`);
    }
  }
  return [
    ...errors,
    ...releaseManifestFilenameCoverageErrors(
      "checksum manifest",
      filenames,
      options.expectedFilenames,
    ),
  ];
}

export function releaseEvidenceManifest(options) {
  const version = normalizeVersion(options.version);
  return {
    schema_version: 1,
    product: "apm-desktop",
    version,
    generated_at: options.generatedAt ?? new Date().toISOString(),
    artifacts: [
      artifactEvidence(options.appZip, "app_zip"),
      ...options.dmgs.map((dmg) => artifactEvidence(dmg, "dmg")),
      artifactEvidence(options.checksumManifest, "checksum_manifest"),
    ],
    checks: {
      app_zip_payload: "verified",
      dmg_payload: "verified",
      checksum_manifest: "verified",
    },
  };
}

export function verifyReleaseEvidenceManifest(
  evidencePath,
  directory,
  expectedVersion,
  options = {},
) {
  const errors = [];
  const evidence = readEvidenceJson(evidencePath, errors);
  if (!evidence) {
    return errors;
  }

  const version = normalizeVersion(expectedVersion);
  if (evidence.schema_version !== 1) {
    errors.push("release evidence schema_version must be 1");
  }
  if (evidence.product !== "apm-desktop") {
    errors.push("release evidence product must be apm-desktop");
  }
  if (evidence.version !== version) {
    errors.push(`release evidence version must be ${version}`);
  }

  errors.push(...releaseEvidenceCheckErrors(evidence.checks));

  if (!Array.isArray(evidence.artifacts)) {
    errors.push("release evidence artifacts must be an array");
  }
  const artifacts = Array.isArray(evidence.artifacts) ? evidence.artifacts : [];
  const roles = new Set(artifacts.map((artifact) => artifact?.role));
  const artifactFilenames = artifacts
    .map((artifact) => artifact?.filename)
    .filter((filename) => typeof filename === "string");
  for (const role of ["app_zip", "checksum_manifest"]) {
    if (!roles.has(role)) {
      errors.push(`release evidence must include ${role}`);
    }
  }
  if (!roles.has("dmg")) {
    errors.push("release evidence must include at least one dmg");
  }
  errors.push(
    ...releaseManifestFilenameCoverageErrors(
      "release evidence",
      artifactFilenames,
      options.expectedFilenames,
    ),
  );

  for (const artifact of artifacts) {
    errors.push(...releaseEvidenceArtifactErrors(artifact, directory));
  }

  const checksumArtifact = artifacts.find(
    (artifact) => artifact?.role === "checksum_manifest",
  );
  if (checksumArtifact?.filename) {
    errors.push(
      ...verifyChecksumManifest(resolve(directory, checksumArtifact.filename), directory),
    );
  }

  return errors;
}

export function appZipPayloadErrors(extractRoot, options = {}) {
  const extractedApp = resolve(extractRoot, "apm.app");
  if (!directoryExists(extractedApp)) {
    return [`release app zip must contain apm.app at its root: ${extractedApp}`];
  }
  return [
    ...appBundlePayloadErrors(extractedApp, {
      label: "release app zip apm.app",
      requireIcon: true,
    }),
    ...(options.expectedVersion
      ? appBundleReleaseVersionErrors(
          extractedApp,
          options.expectedVersion,
          "release app zip apm.app",
        )
      : []),
  ];
}

export function appBundleReleaseVersionErrors(bundlePath, expectedVersion, label) {
  return appBundleInfoPlistValueErrors(
    bundlePath,
    {
      CFBundleShortVersionString: expectedVersion,
      CFBundleVersion: expectedVersion,
    },
    { label },
  );
}

export function releaseDmgVersionErrors(dmgs, expectedVersion) {
  const errors = [];
  for (const dmg of dmgs) {
    const name = basename(dmg);
    const match = name.match(/^apm_([^_]+)_.+\.dmg$/);
    if (!match) {
      errors.push(`release DMG artifact name must match apm_<version>_<arch>.dmg: ${name}`);
      continue;
    }
    if (match[1] !== expectedVersion) {
      errors.push(
        `release DMG artifact ${name} version ${match[1]} must match ${expectedVersion}`,
      );
    }
  }
  return errors;
}

function releaseEvidenceCheckErrors(checks) {
  if (!checks || typeof checks !== "object" || Array.isArray(checks)) {
    return ["release evidence checks must be an object"];
  }

  const errors = [];
  for (const [name, expected] of Object.entries(requiredReleaseEvidenceChecks)) {
    if (checks[name] !== expected) {
      errors.push(
        `release evidence check ${name} must be ${expected}; ` +
          `got ${checks[name] ?? "missing"}`,
      );
    }
  }
  return errors;
}

function releaseManifestFilenameCoverageErrors(label, filenames, expectedFilenames) {
  if (!expectedFilenames) {
    return [];
  }

  const errors = [];
  const actual = new Set(filenames);
  const expected = new Set(expectedFilenames);
  for (const filename of duplicateValues(filenames)) {
    errors.push(`${label} contains duplicate artifact entry: ${filename}`);
  }
  for (const filename of expected) {
    if (!actual.has(filename)) {
      errors.push(`${label} must include release artifact: ${filename}`);
    }
  }
  for (const filename of actual) {
    if (!expected.has(filename)) {
      errors.push(`${label} references unexpected release artifact: ${filename}`);
    }
  }
  return errors;
}

function duplicateValues(values) {
  const seen = new Set();
  const duplicates = new Set();
  for (const value of values) {
    if (seen.has(value)) {
      duplicates.add(value);
    }
    seen.add(value);
  }
  return duplicates;
}

function parseChecksumLine(line) {
  const match = line.match(/^([a-fA-F0-9]{64})  (.+)$/);
  if (!match) {
    return null;
  }
  return { sha256: match[1].toLowerCase(), file: match[2] };
}

function artifactEvidence(path, role) {
  return {
    role,
    filename: basename(path),
    bytes: statSync(path).size,
    sha256: fileSha256(path),
  };
}

function readEvidenceJson(evidencePath, errors) {
  try {
    return JSON.parse(readFileSync(evidencePath, "utf8"));
  } catch (error) {
    errors.push(`release evidence JSON is invalid: ${errorMessage(error)}`);
    return null;
  }
}

function releaseEvidenceArtifactErrors(artifact, directory) {
  const errors = [];
  if (!artifact || typeof artifact !== "object") {
    return ["release evidence artifact entries must be objects"];
  }
  if (!["app_zip", "dmg", "checksum_manifest"].includes(artifact.role)) {
    errors.push(`release evidence artifact has unsupported role: ${artifact.role}`);
  }
  if (
    typeof artifact.filename !== "string" ||
    !artifact.filename ||
    basename(artifact.filename) !== artifact.filename
  ) {
    errors.push(`release evidence filename must be a release artifact: ${artifact.filename}`);
    return errors;
  }

  const artifactPath = resolve(directory, artifact.filename);
  if (!existsSync(artifactPath)) {
    errors.push(`release evidence references missing file: ${artifact.filename}`);
    return errors;
  }

  const bytes = statSync(artifactPath).size;
  if (artifact.bytes !== bytes) {
    errors.push(
      `release evidence byte size mismatch for ${artifact.filename}: ` +
        `expected ${artifact.bytes}, got ${bytes}`,
    );
  }

  const sha256 = fileSha256(artifactPath);
  if (artifact.sha256 !== sha256) {
    errors.push(
      `release evidence checksum mismatch for ${artifact.filename}: ` +
        `expected ${artifact.sha256}, got ${sha256}`,
    );
  }

  return errors;
}

function fileSha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function safeOutputDir(path) {
  const outputDir = resolve(path);
  if (outputDir === repoRoot) {
    throw new Error(`release asset output cannot be the repository root: ${outputDir}`);
  }
  if (dirname(outputDir) === outputDir) {
    throw new Error(`release asset output cannot be the filesystem root: ${outputDir}`);
  }
  return outputDir;
}

function verifyAppZip(appZip, expectedVersion) {
  const extractRoot = mkdtempSync(resolve(tmpdir(), "apm-app-zip-"));
  try {
    run("ditto", ["-x", "-k", appZip, extractRoot]);
    const errors = appZipPayloadErrors(extractRoot, { expectedVersion });
    if (errors.length > 0) {
      throw new Error(errors.join("\n"));
    }
  } finally {
    rmSync(extractRoot, { recursive: true, force: true });
  }
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    valueArgs: ["--version", "--output"],
    flagArgs: ["--help"],
  });
  if (errors.length > 0) {
    throw new Error(errors.join("\n"));
  }

  return {
    help: argv.includes("--help"),
    version: valueArg(argv, "--version"),
    outputDir: valueArg(argv, "--output"),
  };
}

function usage() {
  return [
    "Usage: node build-tools/macos-release-assets.mjs --version <version> [--output <dir>] [--help]",
    "",
    "Packages the verified local desktop app and DMG into release assets.",
    "",
    "Options:",
    "  --version <version>  Desktop release version to package",
    "  --output <dir>       Output directory; defaults to ../../desktop-release",
    "  --help               Show this help without packaging artifacts",
  ].join("\n");
}

function directoryExists(path) {
  return existsSync(path) && statSync(path).isDirectory();
}

function run(command, commandArgs) {
  const result = spawnSync(command, commandArgs, {
    cwd: repoRoot,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(" ")} failed`);
  }
}
