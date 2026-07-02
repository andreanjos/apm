import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { repoRoot } from "./macos-release-github-common.mjs";
import {
  appZipPayloadErrors,
  checksumManifestText,
  normalizeVersion,
  releaseDmgVersionErrors,
  releaseEvidenceManifest,
  releaseAssetNames,
  verifyChecksumManifest,
  verifyReleaseEvidenceManifest,
} from "./macos-release-assets.mjs";

const tests = [];

test("normalizes release asset names", () => {
  assertDeepEqual(
    releaseAssetNames("v0.1.1"),
    {
      appZip: "apm-0.1.1-macos-app.zip",
      checksums: "apm-0.1.1-desktop.sha256",
      evidence: "apm-0.1.1-desktop-release-evidence.json",
    },
    "asset names",
  );
  assertEqual(normalizeVersion("0.2.0"), "0.2.0", "plain version");
});

test("writes and verifies checksum manifest entries", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const manifest = resolve(dir, "apm-0.1.1-desktop.sha256");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(manifest, checksumManifestText([appZip, dmg]));

    assertDeepEqual(verifyChecksumManifest(manifest, dir), [], "checksum errors");
  });
});

test("rejects checksum manifests without exact artifact coverage", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const extra = resolve(dir, "apm_0.1.1_extra.dmg");
    const manifest = resolve(dir, "apm-0.1.1-desktop.sha256");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(extra, "extra dmg");
    writeFileSync(manifest, checksumManifestText([appZip, extra, extra]));

    const errors = verifyChecksumManifest(manifest, dir, {
      expectedFilenames: ["apm-0.1.1-macos-app.zip", "apm_0.1.1_aarch64.dmg"],
    }).join("\n");
    assertIncludes(
      errors,
      "must include release artifact: apm_0.1.1_aarch64.dmg",
      "missing checksum coverage",
    );
    assertIncludes(
      errors,
      "unexpected release artifact: apm_0.1.1_extra.dmg",
      "unexpected checksum coverage",
    );
    assertIncludes(
      errors,
      "duplicate artifact entry: apm_0.1.1_extra.dmg",
      "duplicate checksum coverage",
    );
  });
});

test("writes and verifies release evidence manifests", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const checksums = resolve(dir, "apm-0.1.1-desktop.sha256");
    const evidence = resolve(dir, "apm-0.1.1-desktop-release-evidence.json");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(checksums, checksumManifestText([appZip, dmg]));
    writeFileSync(
      evidence,
      JSON.stringify(
        releaseEvidenceManifest({
          version: "0.1.1",
          appZip,
          dmgs: [dmg],
          checksumManifest: checksums,
          generatedAt: "2026-07-01T00:00:00.000Z",
        }),
      ),
    );

    const parsed = JSON.parse(readFileSync(evidence, "utf8"));
    assertEqual(parsed.schema_version, 1, "evidence schema version");
    assertEqual(parsed.version, "0.1.1", "evidence version");
    assertDeepEqual(
      parsed.artifacts.map((artifact) => artifact.role),
      ["app_zip", "dmg", "checksum_manifest"],
      "evidence artifact roles",
    );
    assertDeepEqual(
      parsed.checks,
      {
        app_zip_payload: "verified",
        dmg_payload: "verified",
        checksum_manifest: "verified",
      },
      "evidence checks",
    );
    assertDeepEqual(
      verifyReleaseEvidenceManifest(evidence, dir, "0.1.1"),
      [],
      "evidence manifest errors",
    );
  });
});

test("rejects release evidence without exact artifact coverage", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const extra = resolve(dir, "apm_0.1.1_extra.dmg");
    const checksums = resolve(dir, "apm-0.1.1-desktop.sha256");
    const evidence = resolve(dir, "apm-0.1.1-desktop-release-evidence.json");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(extra, "extra dmg");
    writeFileSync(checksums, checksumManifestText([appZip, dmg, extra]));
    const manifest = releaseEvidenceManifest({
      version: "0.1.1",
      appZip,
      dmgs: [extra, extra],
      checksumManifest: checksums,
    });
    writeFileSync(evidence, JSON.stringify(manifest));

    const errors = verifyReleaseEvidenceManifest(evidence, dir, "0.1.1", {
      expectedFilenames: [
        "apm-0.1.1-macos-app.zip",
        "apm_0.1.1_aarch64.dmg",
        "apm-0.1.1-desktop.sha256",
      ],
    }).join("\n");
    assertIncludes(
      errors,
      "must include release artifact: apm_0.1.1_aarch64.dmg",
      "missing evidence coverage",
    );
    assertIncludes(
      errors,
      "unexpected release artifact: apm_0.1.1_extra.dmg",
      "unexpected evidence coverage",
    );
    assertIncludes(
      errors,
      "duplicate artifact entry: apm_0.1.1_extra.dmg",
      "duplicate evidence coverage",
    );
  });
});

test("rejects release evidence with missing or stale checks", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const checksums = resolve(dir, "apm-0.1.1-desktop.sha256");
    const evidence = resolve(dir, "apm-0.1.1-desktop-release-evidence.json");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(checksums, checksumManifestText([appZip, dmg]));
    const manifest = releaseEvidenceManifest({
      version: "0.1.1",
      appZip,
      dmgs: [dmg],
      checksumManifest: checksums,
    });
    delete manifest.checks.dmg_payload;
    manifest.checks.checksum_manifest = "skipped";
    writeFileSync(evidence, JSON.stringify(manifest));

    const errors = verifyReleaseEvidenceManifest(evidence, dir, "0.1.1").join("\n");
    assertIncludes(errors, "dmg_payload must be verified", "missing check error");
    assertIncludes(errors, "checksum_manifest must be verified", "stale check error");
  });
});

test("rejects release evidence with malformed checks", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const checksums = resolve(dir, "apm-0.1.1-desktop.sha256");
    const evidence = resolve(dir, "apm-0.1.1-desktop-release-evidence.json");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(checksums, checksumManifestText([appZip, dmg]));
    const manifest = releaseEvidenceManifest({
      version: "0.1.1",
      appZip,
      dmgs: [dmg],
      checksumManifest: checksums,
    });
    manifest.checks = [];
    writeFileSync(evidence, JSON.stringify(manifest));

    assertIncludes(
      verifyReleaseEvidenceManifest(evidence, dir, "0.1.1").join("\n"),
      "checks must be an object",
      "malformed checks error",
    );
  });
});

test("rejects release evidence checksum drift", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const checksums = resolve(dir, "apm-0.1.1-desktop.sha256");
    const evidence = resolve(dir, "apm-0.1.1-desktop-release-evidence.json");
    writeFileSync(appZip, "app zip");
    writeFileSync(dmg, "dmg");
    writeFileSync(checksums, checksumManifestText([appZip, dmg]));
    writeFileSync(
      evidence,
      JSON.stringify(
        releaseEvidenceManifest({
          version: "0.1.1",
          appZip,
          dmgs: [dmg],
          checksumManifest: checksums,
        }),
      ),
    );
    writeFileSync(dmg, "changed dmg");

    assertIncludes(
      verifyReleaseEvidenceManifest(evidence, dir, "0.1.1").join("\n"),
      "release evidence byte size mismatch",
      "evidence drift error",
    );
  });
});

test("accepts DMG artifacts matching release version", () => {
  assertDeepEqual(
    releaseDmgVersionErrors(
      [
        "/tmp/apm_0.1.1_aarch64.dmg",
        "/tmp/apm_0.1.1_universal-apple-darwin.dmg",
      ],
      "0.1.1",
    ),
    [],
    "DMG version errors",
  );
});

test("rejects stale DMG artifacts for release version", () => {
  assertIncludes(
    releaseDmgVersionErrors(["/tmp/apm_0.1.0_aarch64.dmg"], "0.1.1").join("\n"),
    "version 0.1.0 must match 0.1.1",
    "stale DMG version error",
  );
});

test("rejects malformed DMG artifact names", () => {
  assertIncludes(
    releaseDmgVersionErrors(["/tmp/apm_0.1.1.dmg"], "0.1.1").join("\n"),
    "apm_<version>_<arch>.dmg",
    "malformed DMG name error",
  );
});

test("accepts extracted app zip payload", () => {
  withTempDir((dir) => {
    createAppBundleFixture(dir);

    assertDeepEqual(appZipPayloadErrors(dir), [], "app zip payload errors");
  });
});

test("accepts extracted app zip payload matching release version", () => {
  withTempDir((dir) => {
    createAppBundleFixture(dir, { version: "0.1.1" });

    assertDeepEqual(
      appZipPayloadErrors(dir, { expectedVersion: "0.1.1" }),
      [],
      "app zip version errors",
    );
  });
});

test("rejects extracted app zip payload with mismatched release version", () => {
  withTempDir((dir) => {
    createAppBundleFixture(dir, { version: "0.1.2" });

    assertIncludes(
      appZipPayloadErrors(dir, { expectedVersion: "0.1.1" }).join("\n"),
      "CFBundleShortVersionString must be 0.1.1",
      "version mismatch error",
    );
  });
});

test("rejects extracted app zip payload without sidecar", () => {
  withTempDir((dir) => {
    createAppBundleFixture(dir);
    rmSync(resolve(dir, "apm.app/Contents/MacOS/apm-cli"));

    assertIncludes(
      appZipPayloadErrors(dir).join("\n"),
      "apm-cli",
      "missing sidecar error",
    );
  });
});

test("rejects extracted app zip without root app bundle", () => {
  withTempDir((dir) => {
    assertIncludes(
      appZipPayloadErrors(dir).join("\n"),
      "apm.app at its root",
      "missing app bundle error",
    );
  });
});

test("rejects unknown release asset packaging arguments before writing output", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-assets.mjs"),
    "--version",
    "0.1.1",
    "--output",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(result.stderr, "--output requires a value", "missing output value");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

test("prints release asset packaging help without packaging artifacts", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-assets.mjs"),
    "--help",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 0, "exit status");
  assertIncludes(result.stdout, "Usage: node build-tools/macos-release-assets.mjs", "help usage");
  assertIncludes(result.stdout, "--version <version>", "version option");
  assertIncludes(result.stdout, "without packaging artifacts", "non-mutating help");
  assertEqual(result.stderr, "", "stderr");
});

test("reports checksum mismatch", () => {
  withTempDir((dir) => {
    const appZip = resolve(dir, "apm-0.1.1-macos-app.zip");
    const manifest = resolve(dir, "apm-0.1.1-desktop.sha256");
    writeFileSync(appZip, "app zip");
    writeFileSync(manifest, checksumManifestText([appZip]));
    writeFileSync(appZip, "changed");

    assertIncludes(
      verifyChecksumManifest(manifest, dir).join("\n"),
      "checksum mismatch",
      "checksum mismatch error",
    );
  });
});

test("reports missing checksum asset", () => {
  withTempDir((dir) => {
    const manifest = resolve(dir, "apm-0.1.1-desktop.sha256");
    writeFileSync(
      manifest,
      "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  missing.dmg\n",
    );

    assertIncludes(
      verifyChecksumManifest(manifest, dir).join("\n"),
      "missing file",
      "missing asset error",
    );
  });
});

test("rejects checksum entries outside the artifact directory", () => {
  withTempDir((dir) => {
    const manifest = resolve(dir, "apm-0.1.1-desktop.sha256");
    writeFileSync(
      manifest,
      "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  ../outside.dmg\n",
    );

    assertIncludes(
      verifyChecksumManifest(manifest, dir).join("\n"),
      "release artifact filename",
      "path entry error",
    );
  });
});

runTests();

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-assets-test-"));
  try {
    run(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function createAppBundleFixture(root, options = {}) {
  const contents = resolve(root, "apm.app/Contents");
  const macos = resolve(contents, "MacOS");
  const resources = resolve(contents, "Resources");
  mkdirSync(macos, { recursive: true });
  mkdirSync(resources, { recursive: true });
  writeFileSync(resolve(contents, "Info.plist"), infoPlist(options.version ?? "0.1.1"));
  writeFileSync(resolve(resources, "apm.icns"), "icon");
  for (const binary of ["apm-desktop", "apm-cli"]) {
    const path = resolve(macos, binary);
    writeFileSync(path, "#!/bin/sh\n");
    chmodSync(path, 0o755);
  }
}

function infoPlist(version) {
  const plistDoctype =
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" ' +
    '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">';
  return `<?xml version="1.0" encoding="UTF-8"?>
${plistDoctype}
<plist version="1.0">
<dict>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
</dict>
</plist>
`;
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

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
