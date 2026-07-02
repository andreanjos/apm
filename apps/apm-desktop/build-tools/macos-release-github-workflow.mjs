import { desktopWorkflowArtifactAcceptanceErrors } from "./macos-release-github-artifacts.mjs";
import {
  defaultReleaseEnvironment,
  githubEnvironmentCheckErrors,
} from "./macos-release-github-env.mjs";
import {
  localWorktreeErrors,
  releaseTagErrors,
  remoteDesktopWorkflowErrors,
} from "./macos-release-github-readiness.mjs";
import {
  argumentErrors,
  booleanArg,
  defaultReleaseTag,
  dirtyReleaseIntentErrors,
  desktopReleaseWorkflowFile,
  desktopReleaseWorkflowPath,
  errorMessage,
  gitRemoteUrl,
  isMain,
  repoFromRemoteUrl,
  run,
  valueArg,
} from "./macos-release-github-common.mjs";

export {
  defaultReleaseTag,
  desktopReleaseWorkflowFile,
  desktopReleaseWorkflowPath,
} from "./macos-release-github-common.mjs";

if (isMain(import.meta.url)) {
  const status = runDesktopWorkflowCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runDesktopWorkflowCommand(argv = [], runtime = {}) {
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
        "macOS desktop release workflow readiness failed:",
        options.errors,
      );
      return 1;
    }

    const errors = options.dispatch
      ? desktopWorkflowDispatchErrors(options)
      : desktopWorkflowReadinessErrors(options);

    if (errors.length > 0) {
      writeFailure(writeError, "macOS desktop release workflow readiness failed:", errors);
      return 1;
    }

    const context = desktopWorkflowContext(options);
    if (options.dispatch) {
      log(`macOS desktop release workflow dispatched for ${context.tag}`);
      if (!context.publish) {
        log("Dry-run artifacts only; publish=false");
      }
    } else {
      log(`macOS desktop release workflow is ready for ${context.tag}`);
      log("Run npm run release:macos:workflow-dispatch to trigger a dry-run build");
    }
    return 0;
  } catch (error) {
    writeError(`macOS desktop release workflow command failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function desktopWorkflowReadinessErrors(options = {}) {
  const context = desktopWorkflowContext(options);
  if (context.errors.length > 0) {
    return context.errors;
  }

  return [
    ...localWorktreeErrors(options),
    ...remoteDesktopWorkflowErrors(context, options),
    ...githubEnvironmentCheckErrors({
      ...options,
      repo: context.repo,
      environment: context.environment,
    }),
    ...releaseTagErrors(context, options),
  ];
}

export function desktopWorkflowDispatchErrors(options = {}) {
  const context = desktopWorkflowContext(options);
  if (context.errors.length > 0) {
    return context.errors;
  }

  const readinessErrors = desktopWorkflowReadinessErrors({
    ...options,
    repo: context.repo,
    environment: context.environment,
    tag: context.tag,
  });
  if (readinessErrors.length > 0) {
    return readinessErrors;
  }

  const publishErrors = publishAcceptanceErrors(context, options);
  if (publishErrors.length > 0) {
    return publishErrors;
  }

  const args = [
    "workflow",
    "run",
    desktopReleaseWorkflowFile,
    "--repo",
    context.repo,
    "--raw-field",
    `tag=${context.tag}`,
    "--raw-field",
    `publish=${context.publish ? "true" : "false"}`,
  ];
  if (context.publish) {
    args.push("--raw-field", `accepted_run_id=${context.acceptedRunId}`);
  }
  if (context.workflowRef) {
    args.push("--ref", context.workflowRef);
  }

  const result = run(options.runCommand, "gh", args);
  if (result.status !== 0) {
    return [
      `desktop release workflow dispatch failed: ${
        result.stderr || result.stdout || "gh exited non-zero"
      }`,
    ];
  }

  return [];
}

export function desktopWorkflowContext(options = {}) {
  const repo = options.repo ?? repoFromRemoteUrl(gitRemoteUrl(options.runCommand));
  const tag = options.tag ?? defaultReleaseTag(options.desktopPackageJsonPath);
  const errors = [];

  if (!repo) {
    errors.push("could not determine GitHub repository; pass --repo owner/name");
  }
  if (!tag) {
    errors.push("could not determine release tag; pass --tag vX.Y.Z");
  }

  return {
    repo,
    tag,
    errors,
    environment: options.environment ?? defaultReleaseEnvironment,
    publish: options.publish ?? false,
    acceptedRunId: `${options.acceptedRunId ?? ""}`.trim(),
    workflowRef: options.ref ?? "",
  };
}

function publishAcceptanceErrors(context, options = {}) {
  if (!context.publish) {
    return [];
  }

  if (!/^[0-9]+$/.test(context.acceptedRunId)) {
    return [
      "publish=true requires a completed accepted dry-run; " +
        "pass --accepted-run-id <id> after release:macos:workflow-accept passes",
    ];
  }

  const accept =
    options.workflowArtifactAcceptanceErrors ?? desktopWorkflowArtifactAcceptanceErrors;
  return accept({
    ...options,
    repo: context.repo,
    tag: context.tag,
    runId: context.acceptedRunId,
    requireDryRun: true,
  }).map((error) => `accepted dry-run artifact check failed: ${error}`);
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--dispatch", "--help"],
    valueArgs: [
      "--repo",
      "--environment",
      "--tag",
      "--ref",
      "--expected-commit",
      "--accepted-run-id",
      "--accepted-run",
    ],
    booleanArgs: ["--allow-dirty", "--publish"],
  });
  const expectedCommit = valueArg(argv, "--expected-commit");
  const allowDirty = booleanArg(argv, "--allow-dirty", false);

  return {
    help: argv.includes("--help"),
    errors: [
      ...errors,
      ...dirtyReleaseIntentErrors({ allowDirty, expectedCommit }),
    ],
    dispatch: argv.includes("--dispatch"),
    repo: valueArg(argv, "--repo"),
    environment: valueArg(argv, "--environment"),
    tag: valueArg(argv, "--tag"),
    ref: valueArg(argv, "--ref"),
    expectedCommit,
    allowDirty,
    publish: booleanArg(argv, "--publish", false),
    acceptedRunId: valueArg(argv, "--accepted-run-id") ?? valueArg(argv, "--accepted-run"),
  };
}

function usage() {
  return [
    "Usage: npm run release:macos:workflow-check -- [options]",
    "       npm run release:macos:workflow-dispatch -- [options]",
    "",
    "Checks or dispatches the manual macOS Desktop Release workflow.",
    "",
    "Options:",
    "  --dispatch                 Dispatch the workflow instead of checking readiness",
    "  --repo <owner/name>        GitHub repository; defaults to origin remote",
    "  --environment <name>       GitHub Environment for release secrets",
    "  --tag <tag>                Release tag; defaults to desktop package version",
    "  --ref <ref>                Workflow ref to dispatch",
    "  --expected-commit <sha>    Require tag/workflow run commit to match",
    "  --allow-dirty             Allow dirty checks only with --expected-commit",
    "  --publish[=true|false]    Attach accepted release assets when true",
    "  --accepted-run-id <id>     Accepted dry-run run id required for publish=true",
    "  --accepted-run <id>        Alias for --accepted-run-id",
    "  --help                    Show this help without checking or dispatching",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
