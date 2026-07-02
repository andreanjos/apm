import {
  argumentErrors,
  booleanArg,
  dirtyReleaseIntentErrors,
  gitWorkingTreeStatus,
  valueArg,
} from "./macos-release-github-common.mjs";

const tests = [];

test("parses value args without consuming the next flag", () => {
  assertEqual(
    valueArg(["--repo", "andreanjos/apm", "--check"], "--repo"),
    "andreanjos/apm",
    "repo value",
  );
  assertEqual(
    valueArg(["--repo", "--check"], "--repo"),
    undefined,
    "missing repo value",
  );
});

test("parses boolean args as presence flags or explicit values", () => {
  assertEqual(
    booleanArg(["--allow-dirty", "--check"], "--allow-dirty", false),
    true,
    "presence flag before another flag",
  );
  assertEqual(
    booleanArg(["--allow-dirty=false", "--check"], "--allow-dirty", true),
    false,
    "explicit false",
  );
  assertEqual(
    booleanArg(["--allow-dirty", "false"], "--allow-dirty", true),
    false,
    "separate false",
  );
});

test("reports unknown and malformed release command arguments", () => {
  const errors = argumentErrors(
    [
      "--repo",
      "andreanjos/apm",
      "--allow-dirty=maybe",
      "--check=false",
      "--extra",
      "loose",
    ],
    {
      flagArgs: ["--check"],
      valueArgs: ["--repo"],
      booleanArgs: ["--allow-dirty"],
    },
  ).join("\n");

  assertIncludes(
    errors,
    "invalid boolean value for --allow-dirty: maybe",
    "invalid boolean",
  );
  assertIncludes(errors, "--check does not accept a value", "flag value");
  assertIncludes(errors, "unknown argument: --extra", "unknown arg");
  assertIncludes(errors, "unexpected argument value: loose", "unexpected value");
});

test("accepts separated boolean values while validating release arguments", () => {
  assertEqual(
    argumentErrors(
      ["--repo=andreanjos/apm", "--allow-dirty", "false", "--check"],
      {
        flagArgs: ["--check"],
        valueArgs: ["--repo"],
        booleanArgs: ["--allow-dirty"],
      },
    ).length,
    0,
    "argument errors",
  );
});

test("requires dirty release checks to name an expected commit", () => {
  assertEqual(
    dirtyReleaseIntentErrors({
      allowDirty: true,
      expectedCommit: "",
    }).join("\n"),
    "--allow-dirty requires --expected-commit <sha> so dirty checks stay tied to an explicit committed release",
    "missing expected commit",
  );
  assertEqual(
    dirtyReleaseIntentErrors({
      allowDirty: true,
      expectedCommit: "abc123",
    }).length,
    0,
    "explicit expected commit",
  );
  assertEqual(
    dirtyReleaseIntentErrors({
      allowDirty: false,
      expectedCommit: "",
    }).length,
    0,
    "clean worktree path",
  );
});

test("reads git worktree status with the requested untracked file mode", () => {
  const calls = [];
  const status = gitWorkingTreeStatus((command, args) => {
    calls.push({ command, args });
    return {
      status: 0,
      stdout: " M README.md\n?? apps/apm-desktop/package.json\n",
      stderr: "",
    };
  }, {
    untrackedFiles: "all",
  });

  assertEqual(calls[0].command, "git", "git command");
  assertEqual(
    calls[0].args.join(" "),
    "status --porcelain --untracked-files=all",
    "git status args",
  );
  assertEqual(status.untrackedFiles, "all", "untracked mode");
  assertEqual(
    status.changes.join("\n"),
    " M README.md\n?? apps/apm-desktop/package.json",
    "worktree changes",
  );
});

runTests();

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

function assertIncludes(value, expected, message) {
  if (!value.includes(expected)) {
    throw new Error(`${message}: expected value to include ${expected}`);
  }
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
