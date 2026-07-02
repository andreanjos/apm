import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  baseConfigErrors,
  cargoReleasePackageVersions,
  desktopWorkflowErrors,
  localSecretTemplateIgnoreErrors,
  releaseEnvironmentErrors,
  releasePreflightErrors,
  releaseSupportFileErrors,
  releaseVersionErrors,
  requiredReleasePackageScripts,
  requiredReleaseSupportFiles,
  requiredReleaseSupportTestFiles,
  requiredWorkflowSnippets,
} from "./macos-release.mjs";
import { errorMessage, repoRoot } from "./macos-release-github-common.mjs";

const tests = [];
const releaseSecrets = [
  "APM_MACOS_CERTIFICATE_BASE64",
  "APM_MACOS_CERTIFICATE_PASSWORD",
  "APM_MACOS_KEYCHAIN_PASSWORD",
  "APM_MACOS_SIGNING_IDENTITY",
  "APM_MACOS_PROVIDER_SHORT_NAME",
  "APPLE_API_KEY",
  "APPLE_API_ISSUER",
  "APPLE_API_KEY_BASE64",
];

test("accepts release-safe Tauri base config", () => {
  assertDeepEqual(baseConfigErrors(releaseConfig()), [], "base config errors");
});

test("rejects release config that drops tests or bundled sidecar", () => {
  const config = releaseConfig({
    build: { beforeBuildCommand: "npm run build" },
    bundle: { active: true, targets: ["app"], externalBin: [] },
  });

  const errors = baseConfigErrors(config).join("\n");

  assertIncludes(errors, "stage the sidecar, test, and build", "build command error");
  assertIncludes(errors, "bundle.targets must include dmg", "DMG target error");
  assertIncludes(errors, "bundle.externalBin must include sidecars/apm-cli", "sidecar error");
});

test("accepts matching desktop release versions", () => {
  assertDeepEqual(
    releaseVersionErrors({
      cliPackage: "0.1.1",
      desktopCrate: "0.1.1",
      tauri: "0.1.1",
      desktopPackage: "0.1.1",
    }),
    [],
    "version errors",
  );
});

test("rejects desktop release version drift", () => {
  const errors = releaseVersionErrors({
    cliPackage: "0.1.1",
    desktopCrate: "0.1.3",
    tauri: "0.1.2",
    desktopPackage: "",
  }).join("\n");

  assertIncludes(
    errors,
    "Cargo apm-desktop crate version 0.1.3 must match",
    "desktop crate drift error",
  );
  assertIncludes(errors, "Tauri config version 0.1.2 must match", "Tauri drift error");
  assertIncludes(errors, "desktop package.json version is required", "missing npm version error");
});

test("reads release package versions from Cargo metadata", () => {
  assertDeepEqual(
    cargoReleasePackageVersions(cargoMetadataFixture()),
    {
      cliPackage: "0.1.1",
      desktopCrate: "0.1.1",
    },
    "Cargo metadata package versions",
  );
});

test("accepts complete desktop release support files, tests, and package scripts", () => {
  withTempDir((dir) => {
    writeReleaseSupportFiles(dir);

    assertDeepEqual(
      releaseSupportFileErrors({
        desktopRoot: dir,
        desktopPackage: releasePackageJson(),
        packageLockPath: resolve(dir, "package-lock.json"),
      }),
      [],
      "release support file errors",
    );
  });
});

test("accepts ignored local release secret template paths", () => {
  assertDeepEqual(
    localSecretTemplateIgnoreErrors({
      gitIgnoreCheck: (path) => ({ ignored: path === ".env.release.local", error: "" }),
    }),
    [],
    "secret template ignore errors",
  );
});

test("rejects trackable local release secret template paths", () => {
  assertIncludes(
    localSecretTemplateIgnoreErrors({
      gitIgnoreCheck: () => ({ ignored: false, error: "" }),
    }).join("\n"),
    "must be ignored by git",
    "trackable secret template error",
  );
});

test("reports local release secret template ignore check failures", () => {
  assertIncludes(
    localSecretTemplateIgnoreErrors({
      gitIgnoreCheck: () => ({ ignored: false, error: "fatal: not a git repository" }),
    }).join("\n"),
    "not a git repository",
    "ignore check failure",
  );
});

test("rejects missing desktop release support files, tests, and package scripts", () => {
  withTempDir((dir) => {
    writeReleaseSupportFiles(dir, {
      omit: [
        "build-tools/macos-release-github-artifacts.mjs",
        "build-tools/macos-release-github-common.mjs",
        "build-tools/macos-release-github-common.test.mjs",
        "build-tools/macos-release-github-readiness.mjs",
        "build-tools/macos-release-status.mjs",
        "build-tools/macos-release-status.test.mjs",
        "package-lock.json",
      ],
    });
    const scripts = { ...releasePackageJson().scripts };
    delete scripts["release:macos:status"];
    delete scripts["release:macos:github-secrets-template"];
    delete scripts["release:macos:workflow-accept"];

    const errors = releaseSupportFileErrors({
      desktopRoot: dir,
      desktopPackage: { scripts },
      packageLockPath: resolve(dir, "package-lock.json"),
    }).join("\n");

    assertIncludes(errors, "package-lock.json", "package lock error");
    assertIncludes(
      errors,
      "macos-release-github-artifacts.mjs",
      "workflow artifact helper error",
    );
    assertIncludes(errors, "macos-release-github-common.mjs", "GitHub common helper error");
    assertIncludes(
      errors,
      "macos-release-github-common.test.mjs",
      "GitHub common helper test error",
    );
    assertIncludes(
      errors,
      "macos-release-github-readiness.mjs",
      "GitHub readiness helper error",
    );
    assertIncludes(errors, "macos-release-status.mjs", "status helper error");
    assertIncludes(errors, "macos-release-status.test.mjs", "status helper test error");
    assertIncludes(errors, "release:macos:status", "status script error");
    assertIncludes(errors, "release:macos:workflow-accept", "workflow accept script error");
    assertIncludes(
      errors,
      "release:macos:github-secrets-template",
      "secret template script error",
    );
  });
});

test("accepts manual protected desktop workflow shape", () => {
  assertDeepEqual(desktopWorkflowErrors(workflowFixture()), [], "workflow errors");
});

test("rejects desktop workflow with unsafe publish default and missing secret", () => {
  const workflow = workflowFixture()
    .replace("        default: false", "        default: true")
    .replace("APPLE_API_KEY_BASE64", "APPLE_API_KEY_CONTENTS")
    .replace("      accepted_run_id:", "      accepted_dry_run_id:")
    .replace("  actions: read", "  actions: none");

  const errors = desktopWorkflowErrors(workflow).join("\n");

  assertIncludes(errors, "publish input must default to false", "publish default error");
  assertIncludes(errors, "APPLE_API_KEY_BASE64", "missing secret error");
  assertIncludes(errors, "accepted dry-run ID", "accepted run input error");
  assertIncludes(errors, "actions: read", "actions read permission error");
});

test("check mode preflight skips local signing credential requirements", () => {
  withTempDir((dir) => {
    const configPath = resolve(dir, "tauri.conf.json");
    const packagePath = resolve(dir, "package.json");
    const workflowPath = resolve(dir, "desktop-release.yml");
    writeFileSync(configPath, `${JSON.stringify(releaseConfig(), null, 2)}\n`);
    writeFileSync(packagePath, `${JSON.stringify(releasePackageJson(), null, 2)}\n`);
    writeFileSync(workflowPath, workflowFixture());
    writeReleaseSupportFiles(dir);

    assertDeepEqual(
      releasePreflightErrors({
        checkOnly: true,
        desktopRoot: dir,
        tauriConfigPath: configPath,
        desktopPackageJsonPath: packagePath,
        desktopPackageLockPath: resolve(dir, "package-lock.json"),
        cargoMetadata: cargoMetadataFixture(),
        workflowPath,
        gitIgnoreCheck: () => ({ ignored: true, error: "" }),
        platform: "linux",
        env: {},
      }),
      [],
      "check-only preflight errors",
    );
  });
});

test("check mode preflight rejects Cargo package version drift", () => {
  withTempDir((dir) => {
    const configPath = resolve(dir, "tauri.conf.json");
    const packagePath = resolve(dir, "package.json");
    const workflowPath = resolve(dir, "desktop-release.yml");
    writeFileSync(configPath, `${JSON.stringify(releaseConfig(), null, 2)}\n`);
    writeFileSync(packagePath, `${JSON.stringify(releasePackageJson(), null, 2)}\n`);
    writeFileSync(workflowPath, workflowFixture());
    writeReleaseSupportFiles(dir);

    const errors = releasePreflightErrors({
      checkOnly: true,
      desktopRoot: dir,
      tauriConfigPath: configPath,
      desktopPackageJsonPath: packagePath,
      desktopPackageLockPath: resolve(dir, "package-lock.json"),
      cargoMetadata: cargoMetadataFixture({
        cliPackageVersion: "0.1.2",
        desktopCrateVersion: "0.1.1",
      }),
      workflowPath,
      gitIgnoreCheck: () => ({ ignored: true, error: "" }),
      platform: "linux",
      env: {},
    }).join("\n");

    assertIncludes(errors, "Cargo apm CLI package version 0.1.2", "CLI drift error");
  });
});

test("release mode requires macOS signing and notarization environment", () => {
  const errors = releaseEnvironmentErrors({
    env: {
      APM_MACOS_SIGNING_IDENTITY: "Apple Development: Example",
      APM_MACOS_PROVIDER_SHORT_NAME: "",
    },
    platform: "linux",
    commandExists: () => false,
    xcrunToolExists: () => false,
    codesigningIdentityExists: () => false,
  }).join("\n");

  assertIncludes(errors, "must run on macOS", "platform error");
  assertIncludes(errors, "Developer ID Application identity", "identity class error");
  assertIncludes(errors, "APM_MACOS_PROVIDER_SHORT_NAME is required", "provider error");
  assertIncludes(errors, "codesign is required", "codesign error");
  assertIncludes(errors, "APPLE_API_KEY is required", "api key error");
});

test("release mode accepts complete environment with available tools", () => {
  withTempDir((dir) => {
    const apiKeyPath = resolve(dir, "AuthKey_TEST.p8");
    writeFileSync(apiKeyPath, appleApiPrivateKey());
    const env = {
      APM_MACOS_SIGNING_IDENTITY: "Developer ID Application: Example Org (TEAMID1234)",
      APM_MACOS_PROVIDER_SHORT_NAME: "TEAMID1234",
      APPLE_API_KEY: "KEYID",
      APPLE_API_ISSUER: "ISSUERID",
      APPLE_API_KEY_PATH: apiKeyPath,
    };

    assertDeepEqual(
      releaseEnvironmentErrors({
        env,
        platform: "darwin",
        commandExists: () => true,
        xcrunToolExists: () => true,
        codesigningIdentityExists: (identity) => identity === env.APM_MACOS_SIGNING_IDENTITY,
      }),
      [],
      "release env errors",
    );
  });
});

test("release mode rejects malformed local notarization API key paths", () => {
  withTempDir((dir) => {
    const malformedPath = resolve(dir, "AuthKey_TEST.p8");
    writeFileSync(malformedPath, "not a private key");
    const env = {
      APM_MACOS_SIGNING_IDENTITY: "Developer ID Application: Example Org (TEAMID1234)",
      APM_MACOS_PROVIDER_SHORT_NAME: "TEAMID1234",
      APPLE_API_KEY: "KEYID",
      APPLE_API_ISSUER: "ISSUERID",
      APPLE_API_KEY_PATH: malformedPath,
    };

    assertIncludes(
      releaseEnvironmentErrors({
        env,
        platform: "darwin",
        commandExists: () => true,
        xcrunToolExists: () => true,
        codesigningIdentityExists: () => true,
      }).join("\n"),
      "App Store Connect private key file",
      "private key content error",
    );

    assertIncludes(
      releaseEnvironmentErrors({
        env: { ...env, APPLE_API_KEY_PATH: dir },
        platform: "darwin",
        commandExists: () => true,
        xcrunToolExists: () => true,
        codesigningIdentityExists: () => true,
      }).join("\n"),
      "APPLE_API_KEY_PATH must be a file",
      "directory path error",
    );

    assertIncludes(
      releaseEnvironmentErrors({
        env: { ...env, APPLE_API_KEY_PATH: resolve(dir, "missing.p8") },
        platform: "darwin",
        commandExists: () => true,
        xcrunToolExists: () => true,
        codesigningIdentityExists: () => true,
      }).join("\n"),
      "APPLE_API_KEY_PATH does not exist",
      "missing path error",
    );
  });
});

test("rejects unknown release preflight arguments before release work", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release.mjs"),
    "--check=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertIncludes(result.stderr, "--check does not accept a value", "check value error");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

test("prints release preflight help without release work", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release.mjs"),
    "--help",
    "--check=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertDeepEqual(
    {
      status: result.status,
      stderr: result.stderr,
    },
    {
      status: 0,
      stderr: "",
    },
    "help result",
  );
  assertIncludes(result.stdout, "Usage: npm run release:macos:check", "check usage");
  assertIncludes(result.stdout, "npm run bundle:macos:release", "release usage");
  assertIncludes(result.stdout, "without running preflight or release build work", "help safety");
});

runTests();

function appleApiPrivateKey() {
  return [
    "-----BEGIN PRIVATE KEY-----",
    "example",
    "-----END PRIVATE KEY-----",
    "",
  ].join("\n");
}

function releaseConfig(overrides = {}) {
  const { build = {}, bundle = {}, ...rest } = overrides;
  return {
    productName: "apm",
    version: "0.1.1",
    identifier: "com.andreanjos.apm",
    build: {
      beforeBuildCommand: "npm run sidecar:stage && npm test && npm run build",
      ...build,
    },
    bundle: {
      active: true,
      targets: ["app", "dmg"],
      externalBin: ["sidecars/apm-cli"],
      ...bundle,
    },
    ...rest,
  };
}

function releasePackageJson() {
  return {
    version: "0.1.1",
    scripts: Object.fromEntries(
      requiredReleasePackageScripts().map((script) => [script, `echo ${script}`]),
    ),
  };
}

function writeReleaseSupportFiles(root, options = {}) {
  const omit = new Set(options.omit ?? []);
  if (!omit.has("package-lock.json")) {
    writeFile(resolve(root, "package-lock.json"), "{}\n");
  }
  for (const relativePath of requiredReleaseSupportFiles()) {
    if (!omit.has(relativePath)) {
      writeFile(resolve(root, relativePath), "\n");
    }
  }
  for (const relativePath of requiredReleaseSupportTestFiles()) {
    if (!omit.has(relativePath)) {
      writeFile(resolve(root, relativePath), "\n");
    }
  }
}

function writeFile(path, contents) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
}

function workflowFixture() {
  return [
    ...requiredWorkflowSnippets().map(({ snippet }) => snippet),
    ...releaseSecrets,
  ].join("\n");
}

function cargoMetadataFixture(options = {}) {
  return {
    packages: [
      {
        name: "apm-core",
        version: options.coreVersion ?? "0.1.1",
        targets: [{ name: "apm_core", kind: ["lib"] }],
      },
      {
        name: "apm-cli",
        version: options.cliPackageVersion ?? "0.1.1",
        targets: [{ name: "apm", kind: ["bin"] }],
      },
      {
        name: "apm-desktop",
        version: options.desktopCrateVersion ?? "0.1.1",
        targets: [{ name: "apm-desktop", kind: ["bin"] }],
      },
    ],
  };
}

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-release-test-"));
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
      console.error(errorMessage(error));
    }
  }
  if (failureCount > 0) {
    throw new Error(`${failureCount} unit ${failureCount === 1 ? "test" : "tests"} failed.`);
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
