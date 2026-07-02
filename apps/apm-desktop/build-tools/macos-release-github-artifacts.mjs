import { mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { normalizeVersion } from "./macos-release-assets.mjs";
import { releaseArtifactAcceptanceErrors } from "./macos-release-acceptance.mjs";
import {
  argumentErrors,
  booleanArg,
  commitMatchesSha,
  defaultReleaseTag,
  desktopReleaseWorkflowFile,
  desktopReleaseWorkflowPath,
  desktopRoot,
  errorMessage,
  gitHeadCommit,
  ghJson,
  gitRemoteUrl,
  isMain,
  repoFromRemoteUrl,
  run,
  shortSha,
  valueArg,
} from "./macos-release-github-common.mjs";

const defaultDownloadRoot = resolve(desktopRoot, ".tmp/github-release-artifacts");
export const desktopReleaseWorkflowName = "Desktop Release";
const workflowRunJsonFields =
  "databaseId,status,conclusion,event,workflowName,displayTitle,url,headSha";

if (isMain(import.meta.url)) {
  const status = runDesktopWorkflowArtifactAcceptanceCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runDesktopWorkflowArtifactAcceptanceCommand(argv = [], runtime = {}) {
  const log = runtime.log ?? console.log;
  const writeError = runtime.error ?? console.error;

  try {
    const options = optionsFromArgs(argv);
    if (options.help) {
      log(usage());
      return 0;
    }
    if (options.errors.length > 0) {
      writeFailure(
        writeError,
        "macOS desktop release workflow artifact acceptance failed:",
        options.errors,
      );
      return 1;
    }

    const errors = desktopWorkflowArtifactAcceptanceErrors(options);
    if (errors.length > 0) {
      writeFailure(
        writeError,
        "macOS desktop release workflow artifact acceptance failed:",
        errors,
      );
      return 1;
    }

    log("macOS desktop release workflow artifact acceptance passed");
    return 0;
  } catch (error) {
    writeError(
      `macOS desktop release workflow artifact acceptance failed: ${errorMessage(error)}`,
    );
    return 1;
  }
}

export function desktopWorkflowArtifactAcceptanceErrors(options = {}) {
  const context = desktopWorkflowArtifactContext(options);
  if (context.errors.length > 0) {
    return context.errors;
  }

  const run = resolvedDesktopWorkflowRun(context, options);
  if (run.error) {
    return [run.error];
  }

  const runErrors = workflowRunInventoryErrors(run.value, context);
  if (runErrors.length > 0) {
    return runErrors;
  }

  const resolvedContext = contextWithWorkflowRun(context, run.value);
  if (resolvedContext.errors.length > 0) {
    return resolvedContext.errors;
  }

  const downloadErrors = downloadWorkflowArtifactErrors(resolvedContext, options);
  if (downloadErrors.length > 0) {
    return downloadErrors;
  }

  const accept = options.acceptanceErrors ?? releaseArtifactAcceptanceErrors;
  return accept({
    version: resolvedContext.version,
    artifactsDir: resolvedContext.artifactsDir,
  });
}

export function desktopWorkflowArtifactContext(options = {}) {
  const repo = options.repo ?? repoFromRemoteUrl(gitRemoteUrl(options.runCommand));
  const tag = options.tag ?? defaultReleaseTag(options.desktopPackageJsonPath);
  const runId = `${options.runId ?? ""}`.trim();
  const errors = [];

  if (!repo) {
    errors.push("could not determine GitHub repository; pass --repo owner/name");
  }
  if (!tag) {
    errors.push("could not determine release tag; pass --tag vX.Y.Z");
  }
  if (runId && !/^[0-9]+$/.test(runId)) {
    errors.push("GitHub workflow run id is required; pass --run-id <id>");
  }

  const version = tag ? normalizeVersion(tag) : "";
  const artifactName = tag ? `apm-desktop-${tag}` : "";
  const artifactsDir = runId ? resolve(defaultDownloadRoot, runId) : defaultDownloadRoot;
  const requireDryRun = options.requireDryRun ?? !options.allowPublishedRun;
  const expectedCommit =
    `${options.expectedCommit ?? ""}`.trim() || gitHeadCommit(options.runCommand);
  return {
    repo,
    tag,
    runId,
    version,
    artifactName,
    artifactsDir,
    errors,
    requireDryRun,
    expectedCommit,
  };
}

export function desktopWorkflowRunErrors(context, options = {}) {
  const workflowRun = context.runId
    ? desktopWorkflowRun(context, options)
    : latestDesktopWorkflowRun(context, options);
  if (workflowRun.error) {
    return [workflowRun.error];
  }

  return workflowRunInventoryErrors(workflowRun.value, context);
}

export function desktopWorkflowRun(context, options = {}) {
  return ghJson(
    options.runCommand,
    [
      "run",
      "view",
      context.runId,
      "--repo",
      context.repo,
      "--json",
      workflowRunJsonFields,
    ],
    `desktop release workflow run ${context.runId}`,
  );
}

export function latestDesktopWorkflowRun(context, options = {}) {
  const workflowRuns = ghJson(
    options.runCommand,
    [
      "run",
      "list",
      "--repo",
      context.repo,
      "--workflow",
      desktopReleaseWorkflowFile,
      "--event",
      "workflow_dispatch",
      "--limit",
      `${options.runLimit ?? 20}`,
      "--json",
      workflowRunJsonFields,
    ],
    `desktop release workflow runs for ${context.tag}`,
  );
  if (workflowRuns.error) {
    if (workflowRuns.error.includes("HTTP 404")) {
      return {
        error:
          `desktop release workflow is not visible on ${context.repo}; ` +
          `merge ${desktopReleaseWorkflowPath} to the default branch before accepting artifacts`,
      };
    }
    return workflowRuns;
  }

  const runs = Array.isArray(workflowRuns.value) ? workflowRuns.value : [];
  const match = runs.find((run) => workflowRunMatchesContext(run, context));
  if (!match) {
    const runKind = context.requireDryRun ? "dry-run " : "";
    const commitLabel = context.expectedCommit
      ? ` at commit ${shortSha(context.expectedCommit)}`
      : "";
    return {
      error:
        `no completed ${runKind}Desktop Release workflow run found for ` +
        `${context.tag}${commitLabel}; ` +
        "pass --run-id <id> after the dry-run workflow completes",
    };
  }

  return { value: match };
}

export function workflowRunInventoryErrors(workflowRun, context) {
  const errors = [];
  const runLabel = workflowRunLabel(workflowRun, context);
  if (!/^[0-9]+$/.test(`${workflowRun?.databaseId ?? ""}`)) {
    errors.push(`workflow run for ${context.tag} must include a numeric databaseId`);
  }
  if (workflowRun?.workflowName !== desktopReleaseWorkflowName) {
    errors.push(
      `workflow run ${runLabel} must be ${desktopReleaseWorkflowName}; ` +
        `got ${workflowRun?.workflowName ?? "unknown workflow"}`,
    );
  }
  if (!workflowRunTitleIncludesTag(workflowRun, context)) {
    errors.push(
      `workflow run ${runLabel} must be for ${context.tag}; ` +
        `got ${workflowRun?.displayTitle ?? "unknown title"}`,
    );
  }
  if (context.requireDryRun && !workflowRunTitleIncludesDryRun(workflowRun)) {
    errors.push(
      `workflow run ${runLabel} must be a dry-run with publish=false; ` +
        `got ${workflowRun?.displayTitle ?? "unknown title"}`,
    );
  }
  if (workflowRun?.event !== "workflow_dispatch") {
    errors.push(
      `workflow run ${runLabel} must be a manual workflow_dispatch run; ` +
        `got ${workflowRun?.event ?? "unknown event"}`,
    );
  }
  if (workflowRun?.status !== "completed") {
    errors.push(
      `workflow run ${runLabel} must be completed before artifact acceptance; ` +
        `got ${workflowRun?.status ?? "unknown status"}`,
    );
  } else if (workflowRun?.conclusion !== "success") {
    errors.push(
      `workflow run ${runLabel} must conclude success before artifact acceptance; ` +
        `got ${workflowRun?.conclusion ?? "unknown conclusion"}`,
    );
  }
  if (context.expectedCommit && !workflowRunCommitMatches(workflowRun, context)) {
    if (!workflowRun?.headSha) {
      errors.push(`workflow run ${runLabel} must include headSha for commit verification`);
    } else {
      errors.push(
        `workflow run ${runLabel} must be for commit ${shortSha(context.expectedCommit)}; ` +
          `got ${shortSha(workflowRun.headSha)}`,
      );
    }
  }

  return errors;
}

function resolvedDesktopWorkflowRun(context, options) {
  return context.runId
    ? desktopWorkflowRun(context, options)
    : latestDesktopWorkflowRun(context, options);
}

function contextWithWorkflowRun(context, workflowRun) {
  const runId = `${workflowRun?.databaseId ?? context.runId ?? ""}`;
  return {
    ...context,
    runId,
    artifactsDir: runId ? resolve(defaultDownloadRoot, runId) : context.artifactsDir,
    errors: /^[0-9]+$/.test(runId)
      ? []
      : [`workflow run for ${context.tag} must include a numeric databaseId`],
  };
}

function workflowRunMatchesContext(workflowRun, context) {
  return (
    workflowRun?.workflowName === desktopReleaseWorkflowName &&
    workflowRun?.event === "workflow_dispatch" &&
    workflowRun?.status === "completed" &&
    workflowRun?.conclusion === "success" &&
    workflowRunTitleIncludesTag(workflowRun, context) &&
    (!context.requireDryRun || workflowRunTitleIncludesDryRun(workflowRun)) &&
    (!context.expectedCommit || workflowRunCommitMatches(workflowRun, context))
  );
}

function workflowRunLabel(workflowRun, context) {
  return `${context.runId || workflowRun?.databaseId || "unknown"}`;
}

function workflowRunTitleIncludesTag(workflowRun, context) {
  return `${workflowRun?.displayTitle ?? ""}`.includes(context.tag);
}

function workflowRunTitleIncludesDryRun(workflowRun) {
  return `${workflowRun?.displayTitle ?? ""}`.includes("publish=false");
}

function workflowRunCommitMatches(workflowRun, context) {
  return commitMatchesSha(workflowRun?.headSha, context.expectedCommit);
}

function downloadWorkflowArtifactErrors(context, options) {
  rmSync(context.artifactsDir, { recursive: true, force: true });
  mkdirSync(context.artifactsDir, { recursive: true });

  const result = run(
    options.runCommand,
    "gh",
    [
      "run",
      "download",
      context.runId,
      "--repo",
      context.repo,
      "--name",
      context.artifactName,
      "--dir",
      context.artifactsDir,
    ],
  );
  if (result.status !== 0) {
    return [
      `desktop release workflow artifact download failed: ${
        result.stderr || result.stdout || "gh exited non-zero"
      }`,
    ];
  }

  return [];
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--help"],
    valueArgs: ["--repo", "--tag", "--run-id", "--run", "--expected-commit"],
    booleanArgs: ["--allow-published-run"],
  });

  return {
    help: argv.includes("--help"),
    errors,
    repo: valueArg(argv, "--repo"),
    tag: valueArg(argv, "--tag"),
    runId: valueArg(argv, "--run-id") ?? valueArg(argv, "--run"),
    expectedCommit: valueArg(argv, "--expected-commit"),
    allowPublishedRun: booleanArg(argv, "--allow-published-run", false),
  };
}

function usage() {
  return [
    "Usage: npm run release:macos:workflow-accept -- [--repo <owner/name>] [--tag <tag>] [--run-id <id>]",
    "",
    "Downloads and verifies a completed dry-run Desktop Release workflow artifact set.",
    "",
    "Options:",
    "  --repo <owner/name>          GitHub repository; defaults to origin remote",
    "  --tag <tag>                 Release tag; defaults to desktop package version",
    "  --run-id <id>               Explicit workflow run id to download",
    "  --run <id>                  Alias for --run-id",
    "  --expected-commit <sha>     Require the workflow run headSha to match",
    "  --allow-published-run       Allow inspecting publish=true runs after publication",
    "  --help                      Show this help without querying GitHub",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
