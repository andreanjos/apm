import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { requiredReleaseEnvironmentSecrets } from "./macos-release.mjs";
import {
  defaultReleaseTag,
  desktopReleaseWorkflowFile,
  desktopWorkflowDispatchErrors,
  desktopWorkflowReadinessErrors,
  runDesktopWorkflowCommand,
} from "./macos-release-github-workflow.mjs";
import { repoRoot } from "./macos-release-github-common.mjs";

const tests = [];

test("derives default release tag from the desktop package version", () => {
  withTempDir((dir) => {
    const packagePath = resolve(dir, "package.json");
    writeFileSync(packagePath, `${JSON.stringify({ version: "0.1.1" })}\n`);

    assertEqual(defaultReleaseTag(packagePath), "v0.1.1", "default release tag");
  });
});

test("accepts complete remote workflow readiness", () => {
  assertDeepEqual(
    desktopWorkflowReadinessErrors({
      repo: "andreanjos/apm",
      tag: "v0.1.1",
      runCommand: fakeGithub(),
    }),
    [],
    "workflow readiness errors",
  );
});

test("reports a desktop workflow that has not reached the default branch", () => {
  const errors = desktopWorkflowReadinessErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub([], { missingWorkflow: true }),
  }).join("\n");

  assertIncludes(errors, "desktop-release.yml to the default branch", "missing workflow");
});

test("reports missing release tag and environment secrets", () => {
  const errors = desktopWorkflowReadinessErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub([], {
      missingSecrets: ["APPLE_API_KEY_BASE64"],
      missingTag: true,
    }),
  }).join("\n");

  assertIncludes(errors, "APPLE_API_KEY_BASE64", "missing secret");
  assertIncludes(errors, "release tag v0.1.1", "missing tag");
});

test("reports release tags that do not point at the expected commit", () => {
  const errors = desktopWorkflowReadinessErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    runCommand: fakeGithub([], {
      tagSha: "stale-release-sha",
    }),
  }).join("\n");

  assertIncludes(errors, "release tag v0.1.1 points to stale-releas", "stale tag");
  assertIncludes(errors, "expected expected-rel", "expected commit");
  assertIncludes(errors, "move v0.1.1 to expected-rel", "explicit retag guidance");
  assertIncludes(
    errors,
    "rerun with --expected-commit stale-releas",
    "intended older tag guidance",
  );
  assertEqual(
    errors.includes("pass --expected-commit <sha>"),
    false,
    "should not suggest a generic expected commit after one was supplied",
  );
});

test("reports dirty local worktree before workflow dispatch", () => {
  const errors = desktopWorkflowReadinessErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub([], {
      dirtyWorktree: [" M README.md", "?? apps/"],
    }),
  }).join("\n");

  assertIncludes(errors, "working tree has uncommitted changes", "dirty worktree");
  assertIncludes(errors, "workflow builds the release tag", "release tag warning");
});

test("allows dirty local worktree checks with an explicit expected commit", () => {
  assertDeepEqual(
    desktopWorkflowReadinessErrors({
      repo: "andreanjos/apm",
      tag: "v0.1.1",
      allowDirty: true,
      expectedCommit: "expected-release-sha",
      runCommand: fakeGithub([], {
        dirtyWorktree: [" M README.md"],
      }),
    }),
    [],
    "workflow readiness errors",
  );
});

test("reports local worktree inspection failures before workflow dispatch", () => {
  const errors = desktopWorkflowReadinessErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub([], {
      worktreeStatusError: "fatal: not a git repository",
    }),
  }).join("\n");

  assertIncludes(errors, "local worktree status check failed", "worktree failure");
  assertIncludes(errors, "not a git repository", "git failure");
});

test("resolves annotated release tags before commit comparison", () => {
  const calls = [];

  assertDeepEqual(
    desktopWorkflowReadinessErrors({
      repo: "andreanjos/apm",
      tag: "v0.1.1",
      expectedCommit: "expected-release-sha",
      runCommand: fakeGithub(calls, {
        annotatedTag: true,
      }),
    }),
    [],
    "annotated tag readiness errors",
  );
  assertEqual(
    calls.some(
      (call) =>
        call.args.join(" ") === "api repos/andreanjos/apm/git/tags/tag-object-sha",
    ),
    true,
    "annotated tag object lookup",
  );
});

test("dispatches the desktop workflow only after readiness passes", () => {
  const calls = [];

  assertDeepEqual(
    desktopWorkflowDispatchErrors({
      repo: "andreanjos/apm",
      tag: "v0.1.1",
      runCommand: fakeGithub(calls),
    }),
    [],
    "workflow dispatch errors",
  );

  const dispatch = calls.find(
    (call) => call.command === "gh" &&
      call.args[0] === "workflow" &&
      call.args[1] === "run",
  );
  assertDeepEqual(
    dispatch.args,
    [
      "workflow",
      "run",
      desktopReleaseWorkflowFile,
      "--repo",
      "andreanjos/apm",
      "--raw-field",
      "tag=v0.1.1",
      "--raw-field",
      "publish=false",
    ],
    "workflow dispatch args",
  );
});

test("requires an accepted dry-run id before publish dispatch", () => {
  const calls = [];
  const errors = desktopWorkflowDispatchErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    publish: true,
    runCommand: fakeGithub(calls),
  }).join("\n");

  assertIncludes(errors, "--accepted-run-id", "accepted dry-run id");
  assertEqual(
    calls.some((call) => call.args[0] === "workflow" && call.args[1] === "run"),
    false,
    "dispatch call",
  );
});

test("validates an accepted dry-run before publish dispatch", () => {
  const calls = [];
  let accepted = null;

  assertDeepEqual(
    desktopWorkflowDispatchErrors({
      repo: "andreanjos/apm",
      tag: "v0.1.1",
      publish: true,
      acceptedRunId: "123",
      runCommand: fakeGithub(calls),
      workflowArtifactAcceptanceErrors: (options) => {
        accepted = options;
        return [];
      },
    }),
    [],
    "publish dispatch errors",
  );

  assertEqual(accepted.runId, "123", "accepted run id");
  assertEqual(accepted.requireDryRun, true, "dry-run requirement");
  const dispatch = calls.find(
    (call) => call.command === "gh" &&
      call.args[0] === "workflow" &&
      call.args[1] === "run",
  );
  assertIncludes(dispatch.args.join(" "), "publish=true", "publish dispatch");
  assertIncludes(
    dispatch.args.join(" "),
    "accepted_run_id=123",
    "accepted run id dispatch",
  );
});

test("does not publish when accepted dry-run validation fails", () => {
  const calls = [];
  const errors = desktopWorkflowDispatchErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    publish: true,
    acceptedRunId: "123",
    runCommand: fakeGithub(calls),
    workflowArtifactAcceptanceErrors: () => ["bad dry-run artifact"],
  }).join("\n");

  assertIncludes(errors, "bad dry-run artifact", "accepted dry-run failure");
  assertEqual(
    calls.some((call) => call.args[0] === "workflow" && call.args[1] === "run"),
    false,
    "dispatch call",
  );
});

test("does not dispatch when readiness fails", () => {
  const calls = [];
  const errors = desktopWorkflowDispatchErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runCommand: fakeGithub(calls, {
      missingSecrets: ["APPLE_API_KEY_BASE64"],
    }),
  }).join("\n");

  assertIncludes(errors, "APPLE_API_KEY_BASE64", "readiness error");
  assertEqual(
    calls.some((call) => call.args[0] === "workflow" && call.args[1] === "run"),
    false,
    "dispatch call",
  );
});

test("prints workflow command help without readiness checks or dispatch", () => {
  const output = [];
  const errors = [];
  const status = runDesktopWorkflowCommand([
    "--help",
    "--dispatch",
    "--publish=true",
    "--accepted-run-id",
    "not-a-run-id",
  ], {
    log: (line) => output.push(line),
    error: (line) => errors.push(line),
  });

  const help = output.join("\n");
  assertEqual(status, 0, "help status");
  assertIncludes(help, "Usage: npm run release:macos:workflow-check", "check usage");
  assertIncludes(help, "npm run release:macos:workflow-dispatch", "dispatch usage");
  assertDeepEqual(errors, [], "help errors");
});

test("rejects unknown workflow arguments before readiness checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-github-workflow.mjs"),
    "--repo",
    "andreanjos/apm",
    "--tag",
    "v0.1.1",
    "--publish=maybe",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(
    result.stderr,
    "invalid boolean value for --publish: maybe",
    "invalid publish value",
  );
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

test("rejects --allow-dirty without an expected release commit", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-github-workflow.mjs"),
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

runTests();

function fakeGithub(calls = [], behavior = {}) {
  return (command, args, options = {}) => {
    calls.push({ command, args, options });
    if (command === "git" && args.join(" ") === "status --porcelain --untracked-files=normal") {
      if (behavior.worktreeStatusError) {
        return { status: 1, stdout: "", stderr: behavior.worktreeStatusError };
      }
      return {
        status: 0,
        stdout: `${(behavior.dirtyWorktree ?? []).join("\n")}\n`,
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
    if (args[0] === "workflow" && args[1] === "run") {
      return {
        status: 0,
        stdout: "https://github.com/andreanjos/apm/actions/runs/1\n",
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
          type: behavior.annotatedTag ? "tag" : "commit",
          sha: behavior.annotatedTag
            ? "tag-object-sha"
            : behavior.tagSha ?? "expected-release-sha",
        },
      })}\n`,
      stderr: "",
    };
  }
  if (key === "api repos/andreanjos/apm/git/tags/tag-object-sha") {
    return {
      status: 0,
      stdout: `${JSON.stringify({
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

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-desktop-workflow-test-"));
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
