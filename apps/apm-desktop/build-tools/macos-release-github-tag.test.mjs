import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import {
  releaseTagPlan,
  runReleaseTagCommand,
} from "./macos-release-github-tag.mjs";
import { repoRoot } from "./macos-release-github-common.mjs";

const tests = [];

test("prints a dry-run retag plan without mutating tags", () => {
  const calls = [];
  const output = [];
  const status = runReleaseTagCommand([
    "--tag",
    "v0.1.1",
    "--expected-commit",
    "expected-release-sha",
  ], {
    log: (line) => output.push(line),
    runCommand: fakeGit(calls, {
      remoteRefSha: "stale-tag-ref-sha",
      remoteTargetSha: "stale-release-sha",
    }),
  });

  const text = output.join("\n");
  assertEqual(status, 0, "exit status");
  assertIncludes(text, "Dry run only", "dry-run marker");
  assertIncludes(
    text,
    "git push --force-with-lease=refs/tags/v0.1.1:stale-tag-ref-sha origin expected-release-sha:refs/tags/v0.1.1",
    "lease-protected push command",
  );
  assertEqual(
    calls.some((call) => call.args[0] === "push"),
    false,
    "no mutating git commands",
  );
});

test("requires an explicit expected commit before applying", () => {
  const errors = [];
  const status = runReleaseTagCommand(["--tag", "v0.1.1", "--apply"], {
    error: (line) => errors.push(line),
    runCommand: () => {
      throw new Error("should not run git commands");
    },
  });

  assertEqual(status, 1, "exit status");
  assertIncludes(errors.join("\n"), "--apply requires --expected-commit", "apply guard");
});

test("applies an existing remote tag move with force-with-lease", () => {
  const calls = [];
  const output = [];
  const status = runReleaseTagCommand([
    "--tag",
    "v0.1.1",
    "--expected-commit",
    "expected-release-sha",
    "--apply",
  ], {
    log: (line) => output.push(line),
    runCommand: fakeGit(calls, {
      remoteRefSha: "stale-tag-ref-sha",
      remoteTargetSha: "stale-release-sha",
    }),
  });

  assertEqual(status, 0, "exit status");
  assertIncludes(output.join("\n"), "release tag v0.1.1 moved", "applied message");
  assertDeepEqual(
    mutatingGitCalls(calls),
    [
      [
        "push",
        "--force-with-lease=refs/tags/v0.1.1:stale-tag-ref-sha",
        "origin",
        "expected-release-sha:refs/tags/v0.1.1",
      ],
    ],
    "mutating git calls",
  );
});

test("creates a missing remote tag without force-with-lease", () => {
  const calls = [];
  const plan = releaseTagPlan({
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    runCommand: fakeGit(calls, { missingRemoteTag: true }),
  });

  assertEqual(plan.action, "create", "tag action");
  assertDeepEqual(
    plan.commands.map((command) => command.args),
    [
      ["push", "origin", "expected-release-sha:refs/tags/v0.1.1"],
    ],
    "create commands",
  );
});

test("does nothing when the remote tag already points at the expected commit", () => {
  const output = [];
  const status = runReleaseTagCommand([
    "--tag",
    "v0.1.1",
    "--expected-commit",
    "expected-release-sha",
    "--apply",
  ], {
    log: (line) => output.push(line),
    runCommand: fakeGit([], {
      remoteRefSha: "expected-release-sha",
    }),
  });

  assertEqual(status, 0, "exit status");
  assertIncludes(output.join("\n"), "already points", "noop message");
});

test("uses the peeled commit for annotated remote tags while preserving the lease", () => {
  const plan = releaseTagPlan({
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    runCommand: fakeGit([], {
      remoteRefSha: "annotated-tag-object-sha",
      remoteTargetSha: "stale-release-sha",
    }),
  });

  assertEqual(plan.action, "move", "tag action");
  assertEqual(plan.remoteRefSha, "annotated-tag-object-sha", "lease sha");
  assertEqual(plan.remoteTargetSha, "stale-release-sha", "peeled target sha");
  assertDeepEqual(
    plan.commands[0].args,
    [
      "push",
      "--force-with-lease=refs/tags/v0.1.1:annotated-tag-object-sha",
      "origin",
      "expected-release-sha:refs/tags/v0.1.1",
    ],
    "lease push command",
  );
});

test("blocks dirty worktrees unless the dirty intent is explicit", () => {
  const errors = [];
  const status = runReleaseTagCommand([
    "--tag",
    "v0.1.1",
    "--expected-commit",
    "expected-release-sha",
  ], {
    error: (line) => errors.push(line),
    runCommand: fakeGit([], {
      dirtyWorktree: [" M docs/v3-readiness.md"],
      remoteRefSha: "stale-tag-ref-sha",
      remoteTargetSha: "stale-release-sha",
    }),
  });

  assertEqual(status, 1, "exit status");
  assertIncludes(errors.join("\n"), "working tree has uncommitted changes", "dirty guard");

  const plan = releaseTagPlan({
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    allowDirty: true,
    runCommand: fakeGit([], {
      dirtyWorktree: [" M docs/v3-readiness.md"],
      remoteRefSha: "stale-tag-ref-sha",
      remoteTargetSha: "stale-release-sha",
    }),
  });
  assertDeepEqual(plan.errors, [], "explicit dirty intent errors");
});

test("prints help without inspecting git state", () => {
  let callCount = 0;
  const output = [];
  const status = runReleaseTagCommand(["--help"], {
    log: (line) => output.push(line),
    runCommand: () => {
      callCount += 1;
      return { status: 1, stdout: "", stderr: "should not run" };
    },
  });

  assertEqual(status, 0, "exit status");
  assertIncludes(output.join("\n"), "Usage: npm run release:macos:tag", "usage");
  assertEqual(callCount, 0, "git calls");
});

test("rejects unknown arguments before git state checks", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-github-tag.mjs"),
    "--tag",
    "v0.1.1",
    "--expected-commit",
    "expected-release-sha",
    "--bogus",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(result.stderr, "unknown argument: --bogus", "unknown argument");
});

runTests();

function fakeGit(calls = [], behavior = {}) {
  return (command, args) => {
    calls.push({ command, args });
    if (command !== "git") {
      return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
    }

    if (args[0] === "rev-parse") {
      if (behavior.revParseError) {
        return { status: 1, stdout: "", stderr: behavior.revParseError };
      }
      return {
        status: 0,
        stdout: `${behavior.expectedCommit ?? "expected-release-sha"}\n`,
        stderr: "",
      };
    }

    if (args[0] === "status" && args[1] === "--porcelain") {
      return {
        status: 0,
        stdout: `${(behavior.dirtyWorktree ?? []).join("\n")}\n`,
        stderr: "",
      };
    }

    if (args[0] === "ls-remote") {
      if (behavior.remoteLookupError) {
        return { status: 1, stdout: "", stderr: behavior.remoteLookupError };
      }
      if (behavior.missingRemoteTag) {
        return { status: 0, stdout: "", stderr: "" };
      }
      const lines = [
        `${behavior.remoteRefSha ?? "expected-release-sha"}\trefs/tags/v0.1.1`,
      ];
      if (behavior.remoteTargetSha) {
        lines.push(`${behavior.remoteTargetSha}\trefs/tags/v0.1.1^{}`);
      }
      return { status: 0, stdout: `${lines.join("\n")}\n`, stderr: "" };
    }

    if (args[0] === "tag" || args[0] === "push") {
      return { status: 0, stdout: "", stderr: "" };
    }

    return { status: 1, stdout: "", stderr: `unexpected git args: ${args.join(" ")}` };
  };
}

function mutatingGitCalls(calls) {
  return calls
    .filter((call) => call.command === "git" && call.args[0] === "push")
    .map((call) => call.args);
}

function test(name, fn) {
  tests.push({ name, fn });
}

function runTests() {
  let failures = 0;
  for (const { name, fn } of tests) {
    try {
      fn();
      console.log(`ok - ${name}`);
    } catch (error) {
      failures += 1;
      console.error(`not ok - ${name}`);
      console.error(error.stack || error.message);
    }
  }
  if (failures > 0) {
    process.exitCode = 1;
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
    throw new Error(`${message}: expected ${JSON.stringify(value)} to include ${expected}`);
  }
}
