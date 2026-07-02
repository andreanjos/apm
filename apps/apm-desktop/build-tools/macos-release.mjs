import { existsSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  argumentErrors,
  desktopRoot,
  errorMessage,
  isMain,
  repoRoot,
} from "./macos-release-github-common.mjs";

const tauriConfigPath = resolve(desktopRoot, "src-tauri/tauri.conf.json");
const desktopPackageJsonPath = resolve(desktopRoot, "package.json");
const desktopPackageLockPath = resolve(desktopRoot, "package-lock.json");
const desktopWorkflowPath = resolve(repoRoot, ".github/workflows/desktop-release.yml");
const generatedConfigPath = resolve(
  desktopRoot,
  "src-tauri/tauri.release.generated.conf.json",
);

if (isMain(import.meta.url)) {
  main(process.argv.slice(2), process.env, process.platform);
}

function main(argv, env, platform) {
  if (argv.includes("--help")) {
    console.log(usage());
    return;
  }

  const argErrors = argumentErrors(argv, {
    flagArgs: ["--check", "--help"],
  });
  if (argErrors.length > 0) {
    console.error(`macOS release preflight failed: ${argErrors.join("\n")}`);
    process.exit(1);
  }

  const args = new Set(argv);
  const checkOnly = args.has("--check");
  const releaseEnv = releaseEnvironment(env);
  const errors = releasePreflightErrors({
    checkOnly,
    env,
    platform,
    tauriConfigPath,
    workflowPath: desktopWorkflowPath,
  });

  if (errors.length > 0) {
    console.error("macOS release preflight failed:");
    for (const error of errors) {
      console.error(`- ${error}`);
    }
    process.exit(1);
  }

  if (checkOnly) {
    console.log("macOS release preflight config and workflow check passed");
    process.exit(0);
  }

  writeReleaseConfig(releaseEnv);

  const tauriArgs = [
    "tauri",
    "build",
    "--bundles",
    "app,dmg",
    "--ci",
    "--config",
    generatedConfigPath,
  ];
  if (releaseEnv.target) {
    tauriArgs.push("--target", releaseEnv.target);
  }

  try {
    run("npx", tauriArgs, desktopRoot, {
      APM_DESKTOP_DISTRIBUTION_CHANNEL: "public",
    });
    run("node", ["build-tools/macos-verify.mjs", "--mode=release"], desktopRoot);
  } finally {
    cleanReleaseConfig();
  }
}

export function releasePreflightErrors(options = {}) {
  const checkOnly = options.checkOnly ?? false;
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const tauriConfig = readJson(options.tauriConfigPath ?? tauriConfigPath);
  const desktopPackage = readJson(options.desktopPackageJsonPath ?? desktopPackageJsonPath);
  const cargoPackages = cargoReleasePackageVersionsForPreflight(options);
  return [
    ...baseConfigErrors(tauriConfig),
    ...cargoPackages.errors,
    ...releaseVersionErrors({
      ...cargoPackages.versions,
      tauri: tauriConfig.version,
      desktopPackage: desktopPackage.version,
    }),
    ...desktopWorkflowFileErrors(options.workflowPath ?? desktopWorkflowPath),
    ...localSecretTemplateIgnoreErrors(options),
    ...releaseSupportFileErrors({
      desktopRoot: options.desktopRoot ?? desktopRoot,
      desktopPackage,
      packageLockPath: options.desktopPackageLockPath ?? desktopPackageLockPath,
    }),
    ...(checkOnly
      ? []
      : releaseEnvironmentErrors({
          env,
          platform,
          commandExists: options.commandExists,
          xcrunToolExists: options.xcrunToolExists,
          codesigningIdentityExists: options.codesigningIdentityExists,
        })),
  ];
}

export function baseConfigErrors(config) {
  const errors = [];

  if (config.productName !== "apm") {
    errors.push("tauri.conf.json productName must remain 'apm'");
  }
  if (config.identifier !== "com.andreanjos.apm") {
    errors.push("tauri.conf.json identifier must remain 'com.andreanjos.apm'");
  }
  if (config.build?.beforeBuildCommand !== "npm run sidecar:stage && npm test && npm run build") {
    errors.push("beforeBuildCommand must stage the sidecar, test, and build before bundling");
  }
  if (!config.bundle?.active) {
    errors.push("bundle.active must be true for release artifacts");
  }
  if (!arrayIncludes(config.bundle?.targets, "app")) {
    errors.push("bundle.targets must include app");
  }
  if (!arrayIncludes(config.bundle?.targets, "dmg")) {
    errors.push("bundle.targets must include dmg");
  }
  if (!arrayIncludes(config.bundle?.externalBin, "sidecars/apm-cli")) {
    errors.push("bundle.externalBin must include sidecars/apm-cli");
  }

  return errors;
}

export function releaseVersionErrors(versions) {
  const labels = {
    cliPackage: "Cargo apm CLI package",
    desktopCrate: "Cargo apm-desktop crate",
    tauri: "Tauri config",
    desktopPackage: "desktop package.json",
  };
  const entries = Object.entries(labels).map(([key, label]) => ({
    key,
    label,
    version: `${versions[key] ?? ""}`.trim(),
  }));
  const errors = [];

  for (const entry of entries) {
    if (!entry.version) {
      errors.push(`${entry.label} version is required for desktop release`);
    }
  }

  const [baseline] = entries;
  if (baseline.version) {
    for (const entry of entries.slice(1)) {
      if (entry.version && entry.version !== baseline.version) {
        errors.push(
          `${entry.label} version ${entry.version} must match ` +
            `${baseline.label} version ${baseline.version}`,
        );
      }
    }
  }

  return errors;
}

export function cargoReleasePackageVersions(metadata) {
  const packages = Array.isArray(metadata?.packages) ? metadata.packages : [];
  return {
    cliPackage: cargoCliPackage(packages)?.version ?? "",
    desktopCrate: packages.find((pkg) => pkg?.name === "apm-desktop")?.version ?? "",
  };
}

function cargoReleasePackageVersionsForPreflight(options) {
  const metadata = cargoMetadataForPreflight(options);
  if (metadata.error) {
    return {
      versions: { cliPackage: "", desktopCrate: "" },
      errors: [metadata.error],
    };
  }

  return {
    versions: cargoReleasePackageVersions(metadata.value),
    errors: [],
  };
}

function cargoMetadataForPreflight(options) {
  if (options.cargoMetadata) {
    return { value: options.cargoMetadata };
  }

  const metadataText = runCargoMetadata();
  if (metadataText.error) {
    return metadataText;
  }

  try {
    return { value: JSON.parse(metadataText.value) };
  } catch (error) {
    return { error: `cargo metadata emitted invalid JSON: ${errorMessage(error)}` };
  }
}

function runCargoMetadata() {
  const result = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    return { error: `cargo metadata failed: ${result.stderr || result.stdout}` };
  }
  return { value: result.stdout };
}

function cargoCliPackage(packages) {
  return packages.find((pkg) =>
    Array.isArray(pkg?.targets) &&
    pkg.targets.some(
      (target) => target?.name === "apm" && arrayIncludes(target?.kind, "bin"),
    ),
  );
}

export function releaseSupportFileErrors(options = {}) {
  const root = options.desktopRoot ?? desktopRoot;
  const packageJson =
    options.desktopPackage ?? readJson(options.desktopPackageJsonPath ?? desktopPackageJsonPath);
  const packageLockPath = options.packageLockPath ?? desktopPackageLockPath;
  const errors = [];

  if (!existsSync(packageLockPath)) {
    errors.push(`missing desktop release support file: ${packageLockPath}`);
  }
  for (const relativePath of [
    ...requiredReleaseSupportFiles(),
    ...requiredReleaseSupportTestFiles(),
  ]) {
    const path = resolve(root, relativePath);
    if (!existsSync(path)) {
      errors.push(`missing desktop release support file: ${path}`);
    }
  }

  const scripts = packageJson.scripts ?? {};
  for (const script of requiredReleasePackageScripts()) {
    if (!scripts[script]) {
      errors.push(`desktop package.json must define script ${script}`);
    }
  }

  return errors;
}

export function requiredReleaseSupportFiles() {
  return [
    "build-tools/macos-release.mjs",
    "build-tools/macos-release-assets.mjs",
    "build-tools/macos-release-acceptance.mjs",
    "build-tools/macos-release-github-artifacts.mjs",
    "build-tools/macos-release-github-common.mjs",
    "build-tools/macos-release-github-env.mjs",
    "build-tools/macos-release-github-readiness.mjs",
    "build-tools/macos-release-github-secrets.mjs",
    "build-tools/macos-release-status.mjs",
    "build-tools/macos-release-github-tag.mjs",
    "build-tools/macos-release-github-workflow.mjs",
    "build-tools/macos-verify.mjs",
    "build-tools/run-unit-tests.mjs",
    "build-tools/stage-sidecar.mjs",
    "src-tauri/Cargo.toml",
    "src-tauri/tauri.conf.json",
  ];
}

export function requiredReleaseSupportTestFiles() {
  return [
    "build-tools/macos-release.test.mjs",
    "build-tools/macos-release-assets.test.mjs",
    "build-tools/macos-release-acceptance.test.mjs",
    "build-tools/macos-release-github-artifacts.test.mjs",
    "build-tools/macos-release-github-common.test.mjs",
    "build-tools/macos-release-github-env.test.mjs",
    "build-tools/macos-release-github-secrets.test.mjs",
    "build-tools/macos-release-github-workflow.test.mjs",
    "build-tools/macos-release-status.test.mjs",
    "build-tools/macos-release-github-tag.test.mjs",
    "build-tools/macos-verify.test.mjs",
  ];
}

export function requiredReleasePackageScripts() {
  return [
    "test",
    "test:unit",
    "build",
    "sidecar:stage",
    "release:macos:check",
    "bundle:macos:release",
    "verify:macos:release",
    "accept:macos:release",
    "release:macos:github-bootstrap",
    "release:macos:github-check",
    "release:macos:github-secrets",
    "release:macos:github-secrets-template",
    "release:macos:status",
    "release:macos:tag",
    "release:macos:workflow-check",
    "release:macos:workflow-dispatch",
    "release:macos:workflow-accept",
  ];
}

export function localSecretTemplateIgnoreErrors(options = {}) {
  const path = options.secretTemplatePath ?? ".env.release.local";
  const check = options.gitIgnoreCheck ?? gitIgnoreCheck;
  const result = check(path);
  if (result.error) {
    return [`could not verify local release secret template ignore rule: ${result.error}`];
  }
  return result.ignored
    ? []
    : [`local release secret template must be ignored by git: ${path}`];
}

function desktopWorkflowFileErrors(path) {
  if (!existsSync(path)) {
    return [`missing desktop release workflow: ${path}`];
  }

  return desktopWorkflowErrors(readFileSync(path, "utf8"));
}

export function desktopWorkflowErrors(workflow) {
  const errors = [];
  for (const { snippet, message } of requiredWorkflowSnippets()) {
    if (!workflow.includes(snippet)) {
      errors.push(message);
    }
  }

  for (const secret of requiredReleaseEnvironmentSecrets()) {
    if (!workflow.includes(secret)) {
      errors.push(`desktop release workflow must reference ${secret}`);
    }
  }

  return errors;
}

export function requiredReleaseEnvironmentSecrets() {
  return [
    "APM_MACOS_CERTIFICATE_BASE64",
    "APM_MACOS_CERTIFICATE_PASSWORD",
    "APM_MACOS_KEYCHAIN_PASSWORD",
    "APM_MACOS_SIGNING_IDENTITY",
    "APM_MACOS_PROVIDER_SHORT_NAME",
    "APPLE_API_KEY",
    "APPLE_API_ISSUER",
    "APPLE_API_KEY_BASE64",
  ];
}

export function requiredWorkflowSnippets() {
  return [
    {
      snippet: "workflow_dispatch:\n    inputs:",
      message: "desktop release workflow must stay manually dispatched",
    },
    {
      snippet: "run-name: Desktop Release ${{ inputs.tag }} publish=${{ inputs.publish }}",
      message: "desktop release workflow run name must include tag and publish mode",
    },
    {
      snippet: [
        "      tag:",
        "        description: \"Release tag to build, for example v0.1.1\"",
        "        required: true",
        "        type: string",
      ].join("\n"),
      message: "desktop release workflow must require an explicit tag input",
    },
    {
      snippet: [
        "      publish:",
        "        description: \"Attach verified desktop artifacts to the GitHub Release\"",
        "        required: true",
        "        type: boolean",
        "        default: false",
      ].join("\n"),
      message: "desktop release workflow publish input must default to false",
    },
    {
      snippet: [
        "      accepted_run_id:",
        "        description: \"Accepted publish=false Desktop Release run ID required when publish is true\"",
        "        required: false",
        "        type: string",
        "        default: \"\"",
      ].join("\n"),
      message: "desktop release workflow must accept an optional accepted dry-run ID",
    },
    {
      snippet: "permissions:\n  contents: write",
      message: "desktop release workflow needs contents: write for release uploads",
    },
    {
      snippet: "  actions: read",
      message: "desktop release workflow needs actions: read for accepted dry-run artifact checks",
    },
    {
      snippet: "environment: macos-desktop-release",
      message: "desktop release workflow must use the protected release environment",
    },
    {
      snippet: "ref: ${{ inputs.tag }}",
      message: "desktop release workflow must check out the requested release tag",
    },
    {
      snippet: "run: npm run release:macos:check",
      message: "desktop release workflow must run local release preflight",
    },
    {
      snippet: "run: npm run bundle:macos:release",
      message: "desktop release workflow must use the signed release bundle gate",
    },
    {
      snippet: "node apps/apm-desktop/build-tools/macos-release-assets.mjs --version \"$VERSION\"",
      message: "desktop release workflow must package and verify desktop release assets",
    },
    {
      snippet: "node apps/apm-desktop/build-tools/macos-release-acceptance.mjs --version \"$VERSION\"",
      message: "desktop release workflow must accept packaged desktop release assets",
    },
    {
      snippet: "uses: actions/upload-artifact@v4",
      message: "desktop release workflow must upload dry-run artifacts",
    },
    {
      snippet: [
        "      - name: Verify accepted dry-run before publish",
        "        if: ${{ inputs.publish }}",
        "        env:",
        "          GH_TOKEN: ${{ github.token }}",
        "        working-directory: apps/apm-desktop",
      ].join("\n"),
      message: "desktop release workflow must verify an accepted dry-run before publish",
    },
    {
      snippet: "--run-id \"${{ inputs.accepted_run_id }}\"",
      message: "desktop release workflow must pass accepted_run_id to artifact acceptance",
    },
    {
      snippet: [
        "      - name: Attach artifacts to GitHub Release",
        "        if: ${{ inputs.publish }}",
        "        uses: softprops/action-gh-release@v2",
      ].join("\n"),
      message: "desktop release workflow must publish only behind the publish switch",
    },
    {
      snippet: "fail_on_unmatched_files: true",
      message: "desktop release workflow must fail if release assets are missing",
    },
    {
      snippet: "if: always()",
      message: "desktop release workflow must clean up signing keychains",
    },
  ];
}

export function releaseEnvironmentErrors(options = {}) {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const releaseEnv = releaseEnvironment(env);
  const commandExistsFn = options.commandExists ?? commandExists;
  const xcrunToolExistsFn = options.xcrunToolExists ?? xcrunToolExists;
  const codesigningIdentityExistsFn =
    options.codesigningIdentityExists ?? codesigningIdentityExists;
  const errors = [];

  if (platform !== "darwin") {
    errors.push("public macOS desktop release builds must run on macOS");
  }
  if (!releaseEnv.signingIdentity) {
    errors.push("APM_MACOS_SIGNING_IDENTITY is required");
  } else if (!releaseEnv.signingIdentity.startsWith("Developer ID Application:")) {
    errors.push("APM_MACOS_SIGNING_IDENTITY must be a Developer ID Application identity");
  }
  if (!releaseEnv.providerShortName) {
    errors.push("APM_MACOS_PROVIDER_SHORT_NAME is required for notarization");
  }

  for (const name of ["APPLE_API_KEY", "APPLE_API_ISSUER", "APPLE_API_KEY_PATH"]) {
    if (!env[name]?.trim()) {
      errors.push(`${name} is required for notarization`);
    }
  }
  const apiKeyPath = env.APPLE_API_KEY_PATH?.trim();
  if (apiKeyPath) {
    errors.push(...appleApiKeyPathErrors(apiKeyPath));
  }

  for (const command of ["codesign", "security", "xcrun"]) {
    if (!commandExistsFn(command)) {
      errors.push(`${command} is required for macOS release signing/notarization`);
    }
  }
  if (commandExistsFn("xcrun")) {
    if (!xcrunToolExistsFn("notarytool")) {
      errors.push("xcrun notarytool is required for notarization");
    }
    if (!xcrunToolExistsFn("stapler")) {
      errors.push("xcrun stapler is required so release artifacts are stapled");
    }
  }
  if (
    releaseEnv.signingIdentity &&
    commandExistsFn("security") &&
    env.APM_MACOS_SKIP_IDENTITY_CHECK !== "1" &&
    !codesigningIdentityExistsFn(releaseEnv.signingIdentity)
  ) {
    errors.push(
      `codesigning identity is not available in the keychain: ${releaseEnv.signingIdentity}`,
    );
  }

  return errors;
}

function appleApiKeyPathErrors(apiKeyPath) {
  try {
    const stat = statSync(apiKeyPath);
    if (!stat.isFile()) {
      return [`APPLE_API_KEY_PATH must be a file: ${apiKeyPath}`];
    }
    const text = readFileSync(apiKeyPath, "utf8");
    if (
      !text.includes("-----BEGIN PRIVATE KEY-----") ||
      !text.includes("-----END PRIVATE KEY-----")
    ) {
      return ["APPLE_API_KEY_PATH must point to an App Store Connect private key file"];
    }
    return [];
  } catch (error) {
    if (error?.code === "ENOENT") {
      return [`APPLE_API_KEY_PATH does not exist: ${apiKeyPath}`];
    }
    return [`APPLE_API_KEY_PATH is not readable: ${apiKeyPath} (${errorMessage(error)})`];
  }
}

function writeReleaseConfig(releaseEnv) {
  const config = {
    bundle: {
      macOS: {
        signingIdentity: releaseEnv.signingIdentity,
        hardenedRuntime: true,
        providerShortName: releaseEnv.providerShortName,
      },
    },
  };
  writeFileSync(generatedConfigPath, `${JSON.stringify(config, null, 2)}\n`);
  console.log(`wrote release Tauri config overlay: ${generatedConfigPath}`);
}

function releaseEnvironment(env) {
  return {
    signingIdentity: env.APM_MACOS_SIGNING_IDENTITY?.trim() ?? "",
    providerShortName: env.APM_MACOS_PROVIDER_SHORT_NAME?.trim() ?? "",
    target: env.APM_MACOS_TARGET?.trim() ?? "",
  };
}

function cleanReleaseConfig() {
  rmSync(generatedConfigPath, { force: true });
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function usage() {
  return [
    "Usage: npm run release:macos:check -- [--help]",
    "       npm run bundle:macos:release -- [--help]",
    "",
    "Checks or builds signed/notarized macOS desktop release artifacts.",
    "",
    "Options:",
    "  --check  Validate release configuration without requiring signing secrets",
    "  --help   Show this help without running preflight or release build work",
  ].join("\n");
}

function arrayIncludes(value, item) {
  return Array.isArray(value) && value.includes(item);
}

function commandExists(command) {
  return spawnSync("sh", ["-c", `command -v ${command}`], {
    cwd: repoRoot,
    stdio: "ignore",
  }).status === 0;
}

function xcrunToolExists(tool) {
  return spawnSync("xcrun", ["--find", tool], {
    cwd: repoRoot,
    stdio: "ignore",
  }).status === 0;
}

function codesigningIdentityExists(identity) {
  const result = spawnSync("security", ["find-identity", "-v", "-p", "codesigning"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  return result.status === 0 && result.stdout.includes(identity);
}

function gitIgnoreCheck(path) {
  const result = spawnSync("git", ["check-ignore", "-q", "--", path], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (result.status === 0) {
    return { ignored: true, error: "" };
  }
  if (result.status === 1) {
    return { ignored: false, error: "" };
  }
  return {
    ignored: false,
    error: result.stderr || result.stdout || "git check-ignore exited non-zero",
  };
}

function run(command, commandArgs, cwd, env = {}) {
  const result = spawnSync(command, commandArgs, {
    cwd,
    env: { ...process.env, ...env },
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${commandArgs.join(" ")} failed`);
  }
}
