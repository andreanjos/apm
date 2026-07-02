import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readlinkSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
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
import { verifySidecarContract } from "./macos-contract-check.mjs";

const bundleRoot = resolve(repoRoot, "target/release/bundle");
const appPath = resolve(bundleRoot, "macos/apm.app");
const dmgDir = resolve(bundleRoot, "dmg");
const appExecutablePaths = [
  "Contents/MacOS/apm-desktop",
  "Contents/MacOS/apm-cli",
];
const requiredAppBundlePayloadPaths = [
  ...appExecutablePaths,
  "Contents/Info.plist",
];

if (isMain(import.meta.url)) {
  main();
}

function main() {
  if (process.argv.includes("--help")) {
    console.log(usage());
    return;
  }

  let options;
  try {
    options = optionsFromArgs(process.argv.slice(2));
  } catch (error) {
    fail("", [errorMessage(error)]);
  }
  if (!["preview", "release"].includes(options.mode)) {
    fail(options.mode, [`unsupported verification mode: ${options.mode}`]);
  }

  const errors = verifyMacosArtifacts(options);
  if (errors.length > 0) {
    fail(options.mode, errors);
  }

  console.log(`macOS ${options.mode} artifact verification passed`);
}

export function verifyMacosArtifacts(options = {}) {
  const mode = options.mode ?? "preview";
  const requireDmg = options.requireDmg || mode === "release";
  return [
    ...verifyAppBundle(),
    ...(mode === "preview" ? previewSignatureErrors(appPath) : []),
    ...(requireDmg ? verifyDmgArtifacts() : []),
    ...(mode === "release" ? verifyReleaseSignatures() : []),
  ];
}

function verifyAppBundle() {
  const errors = [];
  const sidecarBinary = resolve(appPath, "Contents/MacOS/apm-cli");
  const infoPlist = resolve(appPath, "Contents/Info.plist");

  if (!directoryExists(appPath)) {
    return [`missing app bundle: ${appPath}`];
  }
  errors.push(...appBundlePayloadErrors(appPath, {
    label: "app bundle",
    requireIcon: true,
  }));

  if (existsSync(infoPlist)) {
    errors.push(
      ...appBundleInfoPlistValueErrors(appPath, {
        CFBundleIdentifier: "com.andreanjos.apm",
        CFBundleName: "apm",
        CFBundleExecutable: "apm-desktop",
        CFBundlePackageType: "APPL",
      }),
    );
  }

  if (existsSync(sidecarBinary)) {
    const version = run(sidecarBinary, ["--version"]);
    if (version.status !== 0) {
      errors.push(`bundled apm-cli --version failed: ${version.stderr || version.stdout}`);
    } else if (!version.stdout.trim().startsWith("apm ")) {
      errors.push(`bundled apm-cli reported unexpected version: ${version.stdout.trim()}`);
    }

    const help = run(sidecarBinary, ["serve", "contract", "--help"]);
    if (help.status !== 0) {
      errors.push(`bundled apm-cli serve contract --help failed: ${help.stderr || help.stdout}`);
    }

    const contract = run(sidecarBinary, ["--json", "serve", "contract"]);
    if (contract.status !== 0) {
      errors.push(
        `bundled apm-cli --json serve contract failed: ${contract.stderr || contract.stdout}`,
      );
    } else {
      errors.push(...verifySidecarContract(contract.stdout));
    }
  }

  return errors;
}

function verifyDmgArtifacts() {
  if (!directoryExists(dmgDir)) {
    return [`missing DMG output directory: ${dmgDir}`];
  }

  const dmgs = selectDmgArtifacts(readdirSync(dmgDir), dmgDir);
  if (dmgs.length === 0) {
    return [`missing apm DMG artifact under ${dmgDir}`];
  }

  return dmgArtifactErrors(dmgs);
}

export function dmgArtifactErrors(dmgs) {
  const errors = [];
  for (const dmg of dmgs) {
    const result = run("hdiutil", ["verify", dmg]);
    if (result.status !== 0) {
      errors.push(`hdiutil verify failed for ${basename(dmg)}: ${result.stderr || result.stdout}`);
      continue;
    }

    errors.push(...verifyDmgMountContents(dmg));
  }
  return errors;
}

export function selectDmgArtifacts(entries, directory) {
  return entries
    .filter((entry) => entry.startsWith("apm_") && entry.endsWith(".dmg"))
    .map((entry) => resolve(directory, entry));
}

function verifyDmgMountContents(dmg) {
  const mountPoint = mkdtempSync(resolve(tmpdir(), "apm-dmg-"));
  const attach = run("hdiutil", [
    "attach",
    dmg,
    "-nobrowse",
    "-readonly",
    "-mountpoint",
    mountPoint,
  ]);
  const errors = [];
  let attached = false;
  let detached = false;

  if (attach.status !== 0) {
    errors.push(`hdiutil attach failed for ${basename(dmg)}: ${attach.stderr || attach.stdout}`);
  } else {
    attached = true;
    errors.push(...mountedDmgAppErrors(mountPoint));
    errors.push(...previewSignatureErrors(resolve(mountPoint, "apm.app"), { label: "DMG app" }));

    const detach = run("hdiutil", ["detach", mountPoint, "-quiet"]);
    if (detach.status !== 0) {
      errors.push(`hdiutil detach failed for ${basename(dmg)}: ${detach.stderr || detach.stdout}`);
    } else {
      detached = true;
    }
  }

  if (!attached || detached) {
    rmSync(mountPoint, { recursive: true, force: true });
  }
  return errors;
}

export function mountedDmgAppErrors(mountPoint) {
  const errors = [];
  const mountedApp = resolve(mountPoint, "apm.app");
  if (!directoryExists(mountedApp)) {
    return [`DMG must contain apm.app at its root: ${mountedApp}`];
  }
  errors.push(...mountedDmgApplicationsLinkErrors(mountPoint));
  errors.push(...appBundlePayloadErrors(mountedApp, { label: "DMG app" }));

  return errors;
}

export function appBundlePayloadErrors(bundlePath, options = {}) {
  const label = options.label ?? "app bundle";
  const requiredPaths = options.requireIcon
    ? [...requiredAppBundlePayloadPaths, "Contents/Resources/apm.icns"]
    : requiredAppBundlePayloadPaths;
  const errors = [];

  for (const path of requiredPaths.map((item) => resolve(bundlePath, item))) {
    if (!existsSync(path)) {
      errors.push(`${label} is missing required file: ${path}`);
    }
  }

  for (const path of appExecutablePaths.map((item) => resolve(bundlePath, item))) {
    if (existsSync(path) && !isExecutable(path)) {
      errors.push(`${label} executable is not executable: ${path}`);
    }
  }

  return errors;
}

export function appBundleInfoPlistValueErrors(bundlePath, expectedValues, options = {}) {
  const label = options.label ?? "app bundle";
  const infoPlist = resolve(bundlePath, "Contents/Info.plist");
  if (!existsSync(infoPlist)) {
    return [`${label} is missing required file: ${infoPlist}`];
  }

  const plist = readPlist(infoPlist);
  if (plist.error) {
    return [plist.error];
  }

  const errors = [];
  for (const [key, expected] of Object.entries(expectedValues)) {
    expectPlistValue(errors, plist.value, key, expected);
  }
  return errors.map((error) => `${label} ${error}`);
}

function mountedDmgApplicationsLinkErrors(mountPoint) {
  const applications = resolve(mountPoint, "Applications");
  const link = readLinkStats(applications);
  if (!link) {
    return [`DMG must contain an Applications install target at its root: ${applications}`];
  }
  if (!link.isSymbolicLink()) {
    return [`DMG Applications install target must be a symlink: ${applications}`];
  }
  const target = readlinkSync(applications);
  if (target !== "/Applications") {
    return [`DMG Applications symlink must target /Applications; got ${target}`];
  }
  return [];
}

function readLinkStats(path) {
  try {
    return lstatSync(path);
  } catch (error) {
    if (error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
}

function verifyReleaseSignatures() {
  return releaseSignatureErrors(appPath, releaseDmgs());
}

export function previewSignatureErrors(appBundlePath, options = {}) {
  return strictCodeSignatureErrors(appBundlePath, {
    label: options.label ?? "preview app",
    runCommand: options.runCommand,
  });
}

export function strictCodeSignatureErrors(appBundlePath, options = {}) {
  const label = options.label ?? "app";
  const runCommand = options.runCommand ?? run;
  const codesignVerify = runCommand("codesign", [
    "--verify",
    "--deep",
    "--strict",
    "--verbose=2",
    appBundlePath,
  ]);

  if (codesignVerify.status === 0) {
    return [];
  }

  return [
    `${label} codesign verification failed: ${codesignVerify.stderr || codesignVerify.stdout}`,
  ];
}

export function releaseSignatureErrors(appBundlePath, dmgs) {
  const errors = [];

  errors.push(...strictCodeSignatureErrors(appBundlePath));

  const codesignDetails = run("codesign", ["-dv", appBundlePath]);
  const signatureText = `${codesignDetails.stdout}\n${codesignDetails.stderr}`;
  if (codesignDetails.status !== 0) {
    errors.push(`could not inspect app signature: ${signatureText}`);
  } else {
    if (signatureText.includes("Signature=adhoc")) {
      errors.push("app is ad-hoc signed; release artifacts must use Developer ID");
    }
    if (!signatureText.includes("Authority=Developer ID Application:")) {
      errors.push("app signature does not include a Developer ID Application authority");
    }
    if (signatureText.includes("TeamIdentifier=not set")) {
      errors.push("app signature does not include a TeamIdentifier");
    }
  }

  const gatekeeper = run("spctl", ["-a", "-vv", "-t", "execute", appBundlePath]);
  if (gatekeeper.status !== 0) {
    errors.push(`Gatekeeper assessment failed: ${gatekeeper.stderr || gatekeeper.stdout}`);
  }

  const appStapler = run("xcrun", ["stapler", "validate", appBundlePath]);
  if (appStapler.status !== 0) {
    errors.push(`app stapler validation failed: ${appStapler.stderr || appStapler.stdout}`);
  }

  for (const dmg of dmgs) {
    const dmgStapler = run("xcrun", ["stapler", "validate", dmg]);
    if (dmgStapler.status !== 0) {
      errors.push(
        `DMG stapler validation failed for ${basename(dmg)}: ` +
          `${dmgStapler.stderr || dmgStapler.stdout}`,
      );
    }
  }

  return errors;
}

function releaseDmgs() {
  if (!directoryExists(dmgDir)) {
    return [];
  }
  return selectDmgArtifacts(readdirSync(dmgDir), dmgDir);
}

function readPlist(path) {
  const result = run("plutil", ["-convert", "json", "-o", "-", path]);
  if (result.status !== 0) {
    return { error: `failed to read ${path}: ${result.stderr || result.stdout}` };
  }
  try {
    return { value: JSON.parse(result.stdout) };
  } catch (error) {
    return { error: `failed to parse ${path}: ${error}` };
  }
}

function expectPlistValue(errors, plist, key, expected) {
  if (plist[key] !== expected) {
    errors.push(`Info.plist ${key} must be ${expected}; got ${plist[key] ?? "missing"}`);
  }
}

function directoryExists(path) {
  return existsSync(path) && statSync(path).isDirectory();
}

function isExecutable(path) {
  return (statSync(path).mode & 0o111) !== 0;
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--require-dmg", "--help"],
    valueArgs: ["--mode"],
  });
  if (errors.length > 0) {
    throw new Error(errors.join("\n"));
  }

  return {
    mode: valueArg(argv, "--mode") ?? "preview",
    requireDmg: argv.includes("--require-dmg"),
  };
}

function usage() {
  return [
    "Usage: npm run verify:macos:preview -- [--require-dmg] [--help]",
    "       npm run verify:macos:release -- [--help]",
    "",
    "Verifies local macOS desktop app and DMG artifacts.",
    "",
    "Options:",
    "  --mode <preview|release>  Verification mode; set by package scripts",
    "  --require-dmg             Require and inspect the local preview DMG",
    "  --help                    Show this help without artifact checks",
  ].join("\n");
}

function run(command, commandArgs) {
  return spawnSync(command, commandArgs, {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

function fail(mode, errors) {
  const label = mode ? `macOS ${mode}` : "macOS";
  console.error(`${label} artifact verification failed:`);
  for (const error of errors) {
    console.error(`- ${error}`);
  }
  process.exit(1);
}
