import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  checksumManifestText,
  releaseAssetNames,
  releaseEvidenceManifest,
} from "./macos-release-assets.mjs";
import { requiredReleaseEnvironmentSecrets } from "./macos-release.mjs";
import {
  formatMacosReleaseStatus,
  formatMacosReleaseStatusMarkdown,
  localReleaseEvidenceStatus,
  macosReleaseStatusReport,
  runMacosReleaseStatusCommand,
} from "./macos-release-status.mjs";
import { repoRoot } from "./macos-release-github-common.mjs";

const tests = [];

test("reports ready when local and remote release gates pass", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub(),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  assertEqual(report.ready, true, "ready status");
  assertDeepEqual(
    report.checks.map((check) => [check.id, check.status]),
    [
      ["context", "pass"],
      ["local_release_preflight", "pass"],
      ["local_release_evidence", "pass"],
      ["local_worktree", "pass"],
      ["remote_desktop_workflow", "pass"],
      ["release_environment_secrets", "pass"],
      ["release_tag", "pass"],
    ],
    "check statuses",
  );
  assertDeepEqual(report.blockers, [], "blockers");
  assertDeepEqual(report.nextSteps, [], "next steps");
});

test("reports remote workflow, secret, and tag blockers without throwing", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    runCommand: fakeGithub({
      missingWorkflow: true,
      missingSecrets: ["APPLE_API_KEY_BASE64"],
      tagSha: "stale-release-sha",
    }),
    localSecretTemplate: missingSecretTemplate(),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  const blockers = report.blockers.join("\n");
  assertEqual(report.ready, false, "ready status");
  assertIncludes(blockers, "desktop release workflow is not visible", "workflow blocker");
  assertIncludes(blockers, "APPLE_API_KEY_BASE64", "secret blocker");
  assertIncludes(blockers, "release tag v0.1.1 points to stale-releas", "tag blocker");
  assertIncludes(
    report.nextSteps.join("\n"),
    "Merge and push .github/workflows/desktop-release.yml",
    "workflow next step",
  );
  assertIncludes(
    report.nextSteps.join("\n"),
    "release:macos:github-secrets-template -- --output ../../.env.release.local",
    "secret template output next step",
  );
  assertIncludes(
    report.nextSteps.join("\n"),
    "fill and source that local env file",
    "secret source next step",
  );
  assertIncludes(
    report.nextSteps.join("\n"),
    "release:macos:github-secrets -- --repo andreanjos/apm",
    "secret dry-run next step",
  );
  assertIncludes(
    report.nextSteps.join("\n"),
    "rerun it with --apply after the dry run passes",
    "secret apply next step",
  );
  assertIncludes(
    report.nextSteps.join("\n"),
    "release:macos:tag -- --tag v0.1.1 --expected-commit expected-rel",
    "tag next step",
  );
  assertEqual(
    report.nextSteps.join("\n").includes("pass --expected-commit <sha>"),
    false,
    "should not suggest generic expected commit when one was supplied",
  );
});

test("points secret setup at an existing ignored local template", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub({
      missingSecrets: ["APPLE_API_KEY_BASE64"],
    }),
    localSecretTemplate: existingSecretTemplate(),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  const nextSteps = report.nextSteps.join("\n");
  assertIncludes(
    nextSteps,
    "Fill and source the existing ../../.env.release.local",
    "existing template next step",
  );
  assertIncludes(
    nextSteps,
    "release:macos:github-secrets -- --repo andreanjos/apm",
    "secret dry-run next step",
  );
  assertEqual(
    nextSteps.includes("release:macos:github-secrets-template"),
    false,
    "should not ask to regenerate existing template",
  );
});

test("warns before using an existing unsafe local secret template", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub({
      missingSecrets: ["APPLE_API_KEY_BASE64"],
    }),
    localSecretTemplate: unsafeSecretTemplate(),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  const nextSteps = report.nextSteps.join("\n");
  assertIncludes(
    nextSteps,
    "Fix ../../.env.release.local so it is ignored by Git and mode 600",
    "unsafe template remediation",
  );
  assertIncludes(
    nextSteps,
    "release:macos:github-secrets -- --repo andreanjos/apm",
    "secret dry-run next step",
  );
  assertEqual(
    nextSteps.includes("release:macos:github-secrets-template"),
    false,
    "should not ask to regenerate unsafe existing template",
  );
});

test("reports a dirty local worktree as a release blocker", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub({
      dirtyWorktree: [" M README.md", "?? apps/"],
    }),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  const blockers = report.blockers.join("\n");
  assertEqual(report.ready, false, "ready status");
  assertIncludes(blockers, "Local release worktree", "worktree check label");
  assertIncludes(blockers, "working tree has uncommitted changes", "worktree blocker");
  assertIncludes(blockers, "workflow builds the release tag", "release tag warning");
  assertDeepEqual(report.localWorktree.changes, [" M README.md", "?? apps/"], "worktree changes");
  assertIncludes(
    report.nextSteps.join("\n"),
    "Commit or stash local changes",
    "worktree next step",
  );
});

test("can expand untracked directories for merge handoff review", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    untrackedFiles: "all",
    runCommand: fakeGithub({
      dirtyWorktreeByMode: {
        all: [
          " M README.md",
          "?? apps/apm-desktop/package.json",
          "?? apps/apm-desktop/src/main.ts",
        ],
      },
    }),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  assertEqual(report.ready, false, "ready status");
  assertEqual(report.localWorktree.untrackedFiles, "all", "untracked mode");
  assertDeepEqual(
    report.localWorktree.changes,
    [
      " M README.md",
      "?? apps/apm-desktop/package.json",
      "?? apps/apm-desktop/src/main.ts",
    ],
    "expanded worktree changes",
  );
  assertIncludes(
    formatMacosReleaseStatusMarkdown(report),
    "## Local Worktree Changes (--untracked-files=all)",
    "expanded markdown heading",
  );
});

test("can hide untracked files for committed-change-only release checks", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    untrackedFiles: "no",
    runCommand: fakeGithub({
      dirtyWorktreeByMode: {
        no: [" M README.md"],
      },
    }),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  assertEqual(report.localWorktree.untrackedFiles, "no", "untracked mode");
  assertDeepEqual(report.localWorktree.changes, [" M README.md"], "tracked changes");
  assertIncludes(
    formatMacosReleaseStatus(report),
    "local worktree changes (--untracked-files=no):",
    "text mode heading",
  );
});

test("allows dirty local worktree status checks with an explicit expected commit", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    allowDirty: true,
    expectedCommit: "expected-release-sha",
    runCommand: fakeGithub({
      dirtyWorktree: [" M README.md"],
    }),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  assertEqual(report.ready, true, "ready status");
  assertDeepEqual(
    report.checks.find((check) => check.id === "local_worktree").errors,
    [],
    "worktree errors",
  );
  assertDeepEqual(report.localWorktree.changes, [" M README.md"], "worktree changes");
  assertEqual(report.localWorktree.allowedDirty, true, "allowed dirty status");
});

test("reports local worktree inspection failures", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub({
      worktreeStatusError: "fatal: not a git repository",
    }),
    localReleaseEvidence: validLocalEvidence(),
    releasePreflightErrors: () => [],
  });

  const blockers = report.blockers.join("\n");
  assertEqual(report.ready, false, "ready status");
  assertIncludes(blockers, "local worktree status check failed", "worktree failure");
  assertIncludes(blockers, "not a git repository", "git failure");
});

test("formats a readable status report", () => {
  const text = formatMacosReleaseStatus({
    ready: false,
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    environment: "macos-desktop-release",
    localEvidence: validLocalEvidence(),
    localWorktree: dirtyLocalWorktree(),
    checks: [
      {
        id: "remote_desktop_workflow",
        label: "Remote Desktop Release workflow",
        status: "fail",
        errors: ["desktop release workflow is not visible"],
      },
    ],
    blockers: [
      "Remote Desktop Release workflow: desktop release workflow is not visible",
    ],
    nextSteps: [
      "Merge and push .github/workflows/desktop-release.yml to the remote default branch.",
    ],
  });

  assertIncludes(text, "macOS desktop release status: not ready", "summary");
  assertIncludes(text, "local evidence: 2026-07-02T02:15:02.081Z", "evidence time");
  assertIncludes(text, "apm-0.1.1-macos-app.zip: 60d017", "evidence artifact");
  assertIncludes(text, "local worktree changes:", "worktree heading");
  assertIncludes(text, "  -  M README.md", "worktree modified file");
  assertIncludes(text, "  - ?? apps/", "worktree untracked file");
  assertIncludes(text, "- [fail] Remote Desktop Release workflow", "check");
  assertIncludes(text, "Blockers:", "blocker heading");
  assertIncludes(text, "Next steps:", "next steps heading");
  assertIncludes(text, "Merge and push .github/workflows/desktop-release.yml", "next step");
});

test("formats a markdown status report for release handoff notes", () => {
  const text = formatMacosReleaseStatusMarkdown({
    ready: false,
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    environment: "macos-desktop-release",
    localEvidence: validLocalEvidence(),
    localWorktree: dirtyLocalWorktree(),
    checks: [
      {
        id: "local_release_preflight",
        label: "Local release preflight",
        status: "pass",
        errors: [],
      },
      {
        id: "release_environment_secrets",
        label: "GitHub release environment secrets",
        status: "fail",
        errors: ["missing GitHub Environment secrets: APPLE_API_KEY_BASE64"],
      },
    ],
    blockers: [
      "GitHub release environment secrets: missing GitHub Environment secrets: APPLE_API_KEY_BASE64",
    ],
    nextSteps: [
      "Move the tag or pass --expected-commit <sha> after release acceptance.",
    ],
  });

  assertIncludes(text, "# macOS Desktop Release Status", "heading");
  assertIncludes(text, "- Status: not ready", "status");
  assertIncludes(text, "- Repo: `andreanjos/apm`", "repo");
  assertIncludes(text, "## Local Evidence", "evidence heading");
  assertIncludes(text, "- Generated At: `2026-07-02T02:15:02.081Z`", "evidence time");
  assertIncludes(text, "`apm-0.1.1-macos-app.zip`: `60d017`", "evidence artifact");
  assertIncludes(text, "## Local Worktree Changes", "worktree heading");
  assertIncludes(text, "- ` M README.md`", "worktree modified file");
  assertIncludes(text, "- `?? apps/`", "worktree untracked file");
  assertIncludes(text, "- [x] Local release preflight", "passing check");
  assertIncludes(text, "- [ ] GitHub release environment secrets", "failing check");
  assertIncludes(text, "APPLE_API_KEY_BASE64", "secret blocker");
  assertIncludes(text, "## Blockers", "blocker heading");
  assertIncludes(text, "## Next Steps", "next steps heading");
  assertIncludes(
    text,
    "1. Move the tag or pass --expected-commit &lt;sha&gt;",
    "escaped next step",
  );
  assertIncludes(
    text,
    "after release acceptance.",
    "next step suffix",
  );
});

test("formats ready markdown without blocker noise", () => {
  const text = formatMacosReleaseStatusMarkdown({
    ready: true,
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    environment: "macos-desktop-release",
    localEvidence: validLocalEvidence(),
    checks: [
      {
        id: "local_release_preflight",
        label: "Local release preflight",
        status: "pass",
        errors: [],
      },
    ],
    blockers: [],
    nextSteps: [],
  });

  assertIncludes(text, "- Status: ready", "ready status");
  assertIncludes(text, "## Local Evidence", "evidence heading");
  assertIncludes(text, "## Blockers\n\n- None", "no blockers");
  assertIncludes(text, "## Next Steps\n\n- None", "no next steps");
});

test("formats allowed dirty markdown with worktree inventory but no blocker", () => {
  const text = formatMacosReleaseStatusMarkdown({
    ready: true,
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    environment: "macos-desktop-release",
    localEvidence: validLocalEvidence(),
    localWorktree: {
      allowedDirty: true,
      changes: [" M README.md"],
      errors: [],
    },
    checks: [
      {
        id: "local_worktree",
        label: "Local release worktree",
        status: "pass",
        errors: [],
      },
    ],
    blockers: [],
    nextSteps: [],
  });

  assertIncludes(text, "- Status: ready", "ready status");
  assertIncludes(text, "## Local Worktree Changes", "worktree heading");
  assertIncludes(text, "- ` M README.md`", "allowed dirty file");
  assertIncludes(text, "- [x] Local release worktree", "passing worktree check");
  assertIncludes(text, "## Blockers\n\n- None", "no blockers");
});

test("reads and verifies local release evidence for status reports", () => {
  withTempDir((dir) => {
    const version = "0.1.1";
    const names = releaseAssetNames(version);
    const appZip = resolve(dir, names.appZip);
    const dmg = resolve(dir, "apm_0.1.1_aarch64.dmg");
    const checksumManifest = resolve(dir, names.checksums);
    const evidencePath = resolve(dir, names.evidence);

    writeFileSync(appZip, "app zip payload");
    writeFileSync(dmg, "dmg payload");
    writeFileSync(checksumManifest, checksumManifestText([appZip, dmg]));
    writeFileSync(
      evidencePath,
      `${JSON.stringify(
        releaseEvidenceManifest({
          version,
          appZip,
          dmgs: [dmg],
          checksumManifest,
          generatedAt: "2026-07-02T02:15:02.081Z",
        }),
        null,
        2,
      )}\n`,
    );

    const evidence = localReleaseEvidenceStatus(
      { tag: "v0.1.1" },
      { desktopReleaseDir: dir },
    );

    assertDeepEqual(evidence.errors, [], "evidence errors");
    assertEqual(evidence.generatedAt, "2026-07-02T02:15:02.081Z", "generated at");
    assertDeepEqual(
      evidence.artifacts.map((artifact) => [artifact.role, artifact.filename]),
      [
        ["app_zip", names.appZip],
        ["dmg", "apm_0.1.1_aarch64.dmg"],
        ["checksum_manifest", names.checksums],
      ],
      "artifact summary",
    );
  });
});

test("reports missing local release evidence as a handoff blocker", () => {
  const report = macosReleaseStatusReport({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub(),
    localReleaseEvidence: {
      path: "/tmp/missing-evidence.json",
      exists: false,
      generatedAt: "",
      artifacts: [],
      errors: ["missing local release evidence JSON: /tmp/missing-evidence.json"],
    },
    releasePreflightErrors: () => [],
  });

  assertEqual(report.ready, false, "ready status");
  assertIncludes(report.blockers.join("\n"), "Local release evidence", "evidence blocker");
  assertIncludes(
    report.nextSteps.join("\n"),
    "Run npm run verify:v3:local",
    "evidence next step",
  );
});

test("prints status help without release checks", () => {
  let commandCount = 0;
  const output = [];
  const errors = [];
  const status = runMacosReleaseStatusCommand([
    "--help",
    "--json",
    "--markdown",
    "--allow-dirty=maybe",
  ], {
    log: (line) => output.push(line),
    error: (line) => errors.push(line),
    runCommand: () => {
      commandCount += 1;
      return { status: 1, stdout: "", stderr: "should not run" };
    },
    releasePreflightErrors: () => {
      throw new Error("should not run preflight");
    },
  });

  const help = output.join("\n");
  assertEqual(status, 0, "help status");
  assertIncludes(help, "Usage: npm run release:macos:status", "status usage");
  assertIncludes(help, "--markdown", "markdown option");
  assertEqual(commandCount, 0, "help command calls");
  assertDeepEqual(errors, [], "help errors");
});

test("rejects conflicting status output format flags", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-status.mjs"),
    "--json",
    "--markdown",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(
    result.stderr,
    "pass only one status output format",
    "format conflict error",
  );
});

test("rejects --allow-dirty without an expected release commit", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-status.mjs"),
    "--repo",
    "andreanjos/apm",
    "--tag",
    "v0.1.1",
    "--allow-dirty",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(
    result.stderr,
    "--allow-dirty requires --expected-commit <sha>",
    "allow dirty expected commit error",
  );
});

test("rejects invalid untracked file modes before release checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-status.mjs"),
    "--repo",
    "andreanjos/apm",
    "--tag",
    "v0.1.1",
    "--untracked-files",
    "everything",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(
    result.stderr,
    "--untracked-files must be one of: normal, all, no",
    "untracked mode error",
  );
});

test("rejects unknown status arguments before release checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-status.mjs"),
    "--repo",
    "andreanjos/apm",
    "--tag",
    "v0.1.1",
    "--allow-dirty=maybe",
    "--bogus",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(
    result.stderr,
    "invalid boolean value for --allow-dirty: maybe",
    "invalid boolean error",
  );
  assertIncludes(result.stderr, "unknown argument: --bogus", "unknown argument error");
});

runTests();

function fakeGithub(behavior = {}) {
  return (command, args) => {
    if (command === "git" && args[0] === "status" && args[1] === "--porcelain") {
      if (behavior.worktreeStatusError) {
        return { status: 1, stdout: "", stderr: behavior.worktreeStatusError };
      }
      const mode = (args.find((arg) => arg.startsWith("--untracked-files=")) ?? "")
        .replace("--untracked-files=", "") || "normal";
      const dirtyWorktree =
        behavior.dirtyWorktreeByMode?.[mode] ?? behavior.dirtyWorktree ?? [];
      return {
        status: 0,
        stdout: `${dirtyWorktree.join("\n")}\n`,
        stderr: "",
      };
    }
    if (command !== "gh") {
      return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
    }
    if (args[0] === "api") {
      return ghApiResponse(args, behavior);
    }
    if (args[0] === "workflow" && args[1] === "view") {
      return {
        status: 0,
        stdout: workflowFixture(),
        stderr: "",
      };
    }
    return { status: 1, stdout: "", stderr: `unexpected gh request: ${args.join(" ")}` };
  };
}

function ghApiResponse(args, behavior) {
  const key = args.join(" ");
  if (key === "api repos/andreanjos/apm/actions/workflows/desktop-release.yml") {
    if (behavior.missingWorkflow) {
      return { status: 1, stdout: "", stderr: "gh: Not Found (HTTP 404)" };
    }
    return {
      status: 0,
      stdout: `${JSON.stringify({
        name: "Desktop Release",
        path: ".github/workflows/desktop-release.yml",
        state: "active",
      })}\n`,
      stderr: "",
    };
  }
  if (key === "api repos/andreanjos/apm/environments/macos-desktop-release") {
    return {
      status: 0,
      stdout: `${JSON.stringify({ name: "macos-desktop-release" })}\n`,
      stderr: "",
    };
  }
  if (key === "api repos/andreanjos/apm/environments/macos-desktop-release/secrets") {
    const missingSecrets = new Set(behavior.missingSecrets ?? []);
    return {
      status: 0,
      stdout: `${JSON.stringify({
        secrets: requiredReleaseEnvironmentSecrets()
          .filter((name) => !missingSecrets.has(name))
          .map((name) => ({ name })),
      })}\n`,
      stderr: "",
    };
  }
  if (key === "api repos/andreanjos/apm/git/ref/tags/v0.1.1") {
    if (behavior.missingTag) {
      return { status: 1, stdout: "", stderr: "gh: Not Found (HTTP 404)" };
    }
    return {
      status: 0,
      stdout: `${JSON.stringify({
        ref: "refs/tags/v0.1.1",
        object: {
          type: "commit",
          sha: behavior.tagSha ?? "expected-release-sha",
        },
      })}\n`,
      stderr: "",
    };
  }
  return { status: 1, stdout: "", stderr: `unexpected gh api request: ${key}` };
}

function workflowFixture() {
  return readFileSync(resolve(repoRoot, ".github/workflows/desktop-release.yml"), "utf8");
}

function validLocalEvidence() {
  return {
    path: "/tmp/apm-0.1.1-desktop-release-evidence.json",
    exists: true,
    generatedAt: "2026-07-02T02:15:02.081Z",
    artifacts: [
      {
        role: "app_zip",
        filename: "apm-0.1.1-macos-app.zip",
        sha256: "60d017",
      },
      {
        role: "dmg",
        filename: "apm_0.1.1_aarch64.dmg",
        sha256: "eb9e4c",
      },
      {
        role: "checksum_manifest",
        filename: "apm-0.1.1-desktop.sha256",
        sha256: "274f6b",
      },
    ],
    errors: [],
  };
}

function dirtyLocalWorktree() {
  return {
    allowedDirty: false,
    changes: [" M README.md", "?? apps/"],
    errors: ["working tree has uncommitted changes"],
  };
}

function missingSecretTemplate() {
  return {
    displayPath: "../../.env.release.local",
    exists: false,
    ignored: true,
    private: false,
    error: "",
  };
}

function existingSecretTemplate() {
  return {
    displayPath: "../../.env.release.local",
    exists: true,
    ignored: true,
    private: true,
    error: "",
  };
}

function unsafeSecretTemplate() {
  return {
    displayPath: "../../.env.release.local",
    exists: true,
    ignored: false,
    private: false,
    error: "",
  };
}

function test(name, run) {
  tests.push([name, run]);
}

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-release-status-test-"));
  try {
    run(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
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
