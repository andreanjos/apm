import { rmSync, writeFileSync, mkdtempSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { repoRoot } from "./macos-release-github-common.mjs";
import {
  desktopWorkflowArtifactAcceptanceErrors,
  desktopWorkflowArtifactContext,
  latestDesktopWorkflowRun,
  runDesktopWorkflowArtifactAcceptanceCommand,
  workflowRunInventoryErrors,
} from "./macos-release-github-artifacts.mjs";

const tests = [];

test("finds the latest matching Desktop Release run when run id is omitted", () => {
  const calls = [];
  let accepted = null;
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    acceptanceErrors: (options) => {
      accepted = options;
      return [];
    },
    runCommand: fakeGh(calls),
  });

  assertDeepEqual(errors, [], "artifact acceptance errors");
  const listCall = calls.find((call) => call.args[0] === "run" && call.args[1] === "list");
  assertDeepEqual(
    listCall.args.slice(0, 13),
    [
      "run",
      "list",
      "--repo",
      "andreanjos/apm",
      "--workflow",
      "desktop-release.yml",
      "--event",
      "workflow_dispatch",
      "--limit",
      "20",
      "--json",
      "databaseId,status,conclusion,event,workflowName,displayTitle,url,headSha",
    ],
    "run list args",
  );
  const downloadCall = calls.find(
    (call) => call.args[0] === "run" && call.args[1] === "download",
  );
  assertEqual(downloadCall.args[2], "123", "resolved run id");
  assertIncludes(accepted.artifactsDir, "github-release-artifacts/123", "accepted dir");
});

test("skips published runs when discovering the latest accepted dry-run", () => {
  const calls = [];
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    acceptanceErrors: () => [],
    runCommand: fakeGh(calls, {
      listRuns: [
        successfulRun({ databaseId: 124, displayTitle: "Desktop Release v0.1.1 publish=true" }),
        successfulRun({ databaseId: 123, displayTitle: "Desktop Release v0.1.1 publish=false" }),
      ],
    }),
  });

  assertDeepEqual(errors, [], "artifact acceptance errors");
  const downloadCall = calls.find(
    (call) => call.args[0] === "run" && call.args[1] === "download",
  );
  assertEqual(downloadCall.args[2], "123", "resolved dry-run id");
});

test("skips dry-runs from the wrong commit when discovering latest accepted run", () => {
  const calls = [];
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    acceptanceErrors: () => [],
    runCommand: fakeGh(calls, {
      listRuns: [
        successfulRun({ databaseId: 124, headSha: "older-release-sha" }),
        successfulRun({ databaseId: 123, headSha: "expected-release-sha" }),
      ],
    }),
  });

  assertDeepEqual(errors, [], "artifact acceptance errors");
  const downloadCall = calls.find(
    (call) => call.args[0] === "run" && call.args[1] === "download",
  );
  assertEqual(downloadCall.args[2], "123", "resolved same-commit dry-run id");
});

test("reports missing matching Desktop Release runs when run id is omitted", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    acceptanceErrors: () => [],
    runCommand: fakeGh([], { listRuns: [] }),
  }).join("\n");

  assertIncludes(errors, "no completed dry-run Desktop Release workflow run", "missing run error");
});

test("reports missing same-commit dry-runs when only older runs exist", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    expectedCommit: "expected-release-sha",
    acceptanceErrors: () => [],
    runCommand: fakeGh([], {
      listRuns: [successfulRun({ headSha: "older-release-sha" })],
    }),
  }).join("\n");

  assertIncludes(errors, "v0.1.1 at commit expected-rel", "expected commit error");
});

test("rejects malformed explicit GitHub workflow run ids", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "latest",
    acceptanceErrors: () => [],
    runCommand: fakeGh([]),
  }).join("\n");

  assertIncludes(errors, "--run-id", "malformed run id");
});

test("downloads the expected workflow artifact and accepts it", () => {
  const calls = [];
  let accepted = null;
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "123",
    runCommand: fakeGh(calls),
    acceptanceErrors: (options) => {
      accepted = options;
      return [];
    },
  });

  assertDeepEqual(errors, [], "artifact acceptance errors");
  const downloadCall = calls.find(
    (call) => call.args[0] === "run" && call.args[1] === "download",
  );
  assertDeepEqual(
    downloadCall.args.slice(0, 7),
    [
      "run",
      "download",
      "123",
      "--repo",
      "andreanjos/apm",
      "--name",
      "apm-desktop-v0.1.1",
    ],
    "download args",
  );
  assertIncludes(downloadCall.args.at(-1), "github-release-artifacts/123", "download dir");
  assertEqual(accepted.version, "0.1.1", "accepted version");
  assertIncludes(accepted.artifactsDir, "github-release-artifacts/123", "accepted dir");
});

test("derives repository from origin remote", () => {
  const calls = [];
  const gh = fakeGh(calls);
  const runCommand = (command, args, options) => {
    if (command === "git" && args.join(" ") === "remote get-url origin") {
      return {
        status: 0,
        stdout: "git@github.com:andreanjos/apm.git\n",
        stderr: "",
      };
    }
    return gh(command, args, options);
  };

  assertDeepEqual(
    desktopWorkflowArtifactAcceptanceErrors({
      tag: "v0.1.1",
      runId: "124",
      runCommand,
      acceptanceErrors: () => [],
    }),
    [],
    "origin-derived artifact errors",
  );
  const downloadCall = calls.find(
    (call) => call.args[0] === "run" && call.args[1] === "download",
  );
  assertEqual(downloadCall.args[4], "andreanjos/apm", "origin-derived repo");
});

test("requires a completed successful Desktop Release run before download", () => {
  const calls = [];
  let acceptanceCallCount = 0;
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    runCommand: fakeGh(calls, { runStatus: "in_progress", runConclusion: "" }),
    acceptanceErrors: () => {
      acceptanceCallCount += 1;
      return [];
    },
  }).join("\n");

  assertIncludes(errors, "must be completed", "run status error");
  assertEqual(
    calls.some((call) => call.args[0] === "run" && call.args[1] === "download"),
    false,
    "download call",
  );
  assertEqual(acceptanceCallCount, 0, "acceptance call count");
});

test("requires explicit workflow run to match the expected release commit", () => {
  const calls = [];
  let acceptanceCallCount = 0;
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    expectedCommit: "expected-release-sha",
    runCommand: fakeGh(calls, { headSha: "older-release-sha" }),
    acceptanceErrors: () => {
      acceptanceCallCount += 1;
      return [];
    },
  }).join("\n");

  assertIncludes(errors, "must be for commit expected-rel", "run commit error");
  assertIncludes(errors, "got older-releas", "actual run commit");
  assertEqual(
    calls.some((call) => call.args[0] === "run" && call.args[1] === "download"),
    false,
    "download call",
  );
  assertEqual(acceptanceCallCount, 0, "acceptance call count");
});

test("reports workflow run lookup failures before download", () => {
  const calls = [];
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    runCommand: fakeGh(calls, { failRunView: "run not found" }),
    acceptanceErrors: () => [],
  }).join("\n");

  assertIncludes(errors, "run not found", "run view failure");
  assertEqual(
    calls.some((call) => call.args[0] === "run" && call.args[1] === "download"),
    false,
    "download call",
  );
});

test("reports failed Desktop Release runs before download", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    runCommand: fakeGh([], { runConclusion: "failure" }),
    acceptanceErrors: () => [],
  }).join("\n");

  assertIncludes(errors, "must conclude success", "run conclusion error");
});

test("reports wrong workflow runs before download", () => {
  const errors = workflowRunInventoryErrors(
    {
      databaseId: 125,
      workflowName: "CI",
      displayTitle: "CI",
      event: "push",
      status: "completed",
      conclusion: "success",
    },
    { runId: "125" },
  ).join("\n");

  assertIncludes(errors, "Desktop Release", "workflow name error");
  assertIncludes(errors, "workflow_dispatch", "workflow event error");
});

test("reports workflow runs for a different release tag before download", () => {
  const errors = workflowRunInventoryErrors(
    successfulRun({ displayTitle: "Desktop Release v0.1.2 publish=false" }),
    { runId: "125", tag: "v0.1.1" },
  ).join("\n");

  assertIncludes(errors, "must be for v0.1.1", "workflow tag error");
});

test("requires dry-run workflow runs by default", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    runCommand: fakeGh([], {
      displayTitle: "Desktop Release v0.1.1 publish=true",
    }),
    acceptanceErrors: () => [],
  }).join("\n");

  assertIncludes(errors, "publish=false", "dry-run title error");
});

test("allows published workflow runs only when explicitly requested", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    allowPublishedRun: true,
    runCommand: fakeGh([], {
      displayTitle: "Desktop Release v0.1.1 publish=true",
    }),
    acceptanceErrors: () => [],
  });

  assertDeepEqual(errors, [], "published run acceptance errors");
});

test("reports workflow run list failures", () => {
  const result = latestDesktopWorkflowRun(
    { repo: "andreanjos/apm", tag: "v0.1.1" },
    { runCommand: fakeGh([], { failRunList: "workflow not found" }) },
  );

  assertIncludes(result.error, "workflow not found", "run list failure");
});

test("reports missing remote desktop workflow before latest-run lookup", () => {
  const result = latestDesktopWorkflowRun(
    { repo: "andreanjos/apm", tag: "v0.1.1" },
    { runCommand: fakeGh([], { failRunList: "gh: Not Found (HTTP 404)" }) },
  );

  assertIncludes(result.error, "desktop release workflow is not visible", "workflow 404");
});

test("prints workflow artifact acceptance help without GitHub lookup", () => {
  const output = [];
  const errors = [];
  const status = runDesktopWorkflowArtifactAcceptanceCommand([
    "--help",
    "--repo",
    "andreanjos/apm",
    "--run-id",
    "not-a-run-id",
  ], {
    log: (line) => output.push(line),
    error: (line) => errors.push(line),
  });

  assertEqual(status, 0, "help status");
  assertIncludes(
    output.join("\n"),
    "Usage: npm run release:macos:workflow-accept",
    "help usage",
  );
  assertDeepEqual(errors, [], "help errors");
});

test("rejects unknown workflow artifact arguments before GitHub lookup", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-github-artifacts.mjs"),
    "--repo",
    "andreanjos/apm",
    "--tag",
    "v0.1.1",
    "--allow-published-run=maybe",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(
    result.stderr,
    "invalid boolean value for --allow-published-run: maybe",
    "invalid boolean",
  );
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

test("reports download failures before acceptance", () => {
  let acceptanceCallCount = 0;
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "125",
    runCommand: fakeGh([], { failDownload: "artifact expired" }),
    acceptanceErrors: () => {
      acceptanceCallCount += 1;
      return [];
    },
  }).join("\n");

  assertIncludes(errors, "artifact expired", "download failure");
  assertEqual(acceptanceCallCount, 0, "acceptance call count");
});

test("reports acceptance failures after download", () => {
  const errors = desktopWorkflowArtifactAcceptanceErrors({
    repo: "andreanjos/apm",
    tag: "v0.1.1",
    runId: "126",
    runCommand: fakeGh([]),
    acceptanceErrors: () => ["bad signed artifact"],
  }).join("\n");

  assertIncludes(errors, "bad signed artifact", "acceptance failure");
});

test("derives default tag from package version", () => {
  withTempDir((dir) => {
    const packageJson = resolve(dir, "package.json");
    writeFileSync(packageJson, `${JSON.stringify({ version: "0.1.1" })}\n`);

    const context = desktopWorkflowArtifactContext({
      repo: "andreanjos/apm",
      runId: "127",
      desktopPackageJsonPath: packageJson,
    });

    assertEqual(context.tag, "v0.1.1", "default tag");
    assertEqual(context.version, "0.1.1", "default version");
    assertEqual(context.requireDryRun, true, "default dry-run requirement");
  });
});

runTests();

function fakeGh(calls, behavior = {}) {
  return (command, args) => {
    calls.push({ command, args });
    if (command !== "gh") {
      return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
    }
    if (args[0] === "run" && args[1] === "list") {
      if (behavior.failRunList) {
        return { status: 1, stdout: "", stderr: behavior.failRunList };
      }
      const runs = behavior.listRuns ?? [
        successfulRun({ databaseId: 122, displayTitle: "Desktop Release v0.1.0 publish=false" }),
        successfulRun({ databaseId: 123 }),
      ];
      return {
        status: 0,
        stdout: `${JSON.stringify(runs)}\n`,
        stderr: "",
      };
    }
    if (args[0] === "run" && args[1] === "view") {
      if (behavior.failRunView) {
        return { status: 1, stdout: "", stderr: behavior.failRunView };
      }
      return {
        status: 0,
        stdout: `${JSON.stringify(successfulRun({
          databaseId: Number(args[2]),
          workflowName: behavior.workflowName,
          displayTitle: behavior.displayTitle,
          event: behavior.runEvent,
          status: behavior.runStatus,
          conclusion: behavior.runConclusion,
          headSha: behavior.headSha,
        }))}\n`,
        stderr: "",
      };
    }
    if (args[0] === "run" && args[1] === "download") {
      if (behavior.failDownload) {
        return { status: 1, stdout: "", stderr: behavior.failDownload };
      }
      return { status: 0, stdout: "", stderr: "" };
    }
    return { status: 1, stdout: "", stderr: `unexpected gh request: ${args.join(" ")}` };
  };
}

function successfulRun(overrides = {}) {
  return {
    databaseId: overrides.databaseId ?? 123,
    workflowName: overrides.workflowName ?? "Desktop Release",
    displayTitle: overrides.displayTitle ?? "Desktop Release v0.1.1 publish=false",
    event: overrides.event ?? "workflow_dispatch",
    status: overrides.status ?? "completed",
    conclusion: overrides.conclusion ?? "success",
    headSha: overrides.headSha ?? "expected-release-sha",
    url: "https://github.com/andreanjos/apm/actions/runs/1",
  };
}

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-github-artifact-test-"));
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
