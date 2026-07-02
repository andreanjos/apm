import {
  argumentErrors,
  booleanArg,
  commitMatchesSha,
  defaultReleaseTag,
  dirtyReleaseIntentErrors,
  errorMessage,
  isMain,
  run,
  shortSha,
  valueArg,
} from "./macos-release-github-common.mjs";
import { localWorktreeStatus } from "./macos-release-github-readiness.mjs";

if (isMain(import.meta.url)) {
  const status = runReleaseTagCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runReleaseTagCommand(argv = [], runtime = {}) {
  const log = runtime.log ?? console.log;
  const writeError = runtime.error ?? console.error;

  try {
    const options = {
      ...optionsFromArgs(argv),
      runCommand: runtime.runCommand,
    };
    if (options.help) {
      log(usage());
      return 0;
    }
    if (options.errors.length > 0) {
      writeFailure(writeError, "macOS release tag command failed:", options.errors);
      return 1;
    }

    const plan = releaseTagPlan(options);
    if (plan.errors.length > 0) {
      writeFailure(writeError, "macOS release tag command failed:", plan.errors);
      return 1;
    }

    if (options.apply) {
      const errors = applyReleaseTagPlan(plan, options);
      if (errors.length > 0) {
        writeFailure(writeError, "macOS release tag command failed:", errors);
        return 1;
      }
      log(formatAppliedPlan(plan));
      return 0;
    }

    log(formatDryRunPlan(plan));
    return 0;
  } catch (error) {
    writeError(`macOS release tag command failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function releaseTagPlan(options = {}) {
  const tag = options.tag ?? defaultReleaseTag(options.desktopPackageJsonPath);
  const remote = options.remote ?? "origin";
  const result = {
    tag,
    remote,
    expectedCommit: "",
    remoteRefSha: "",
    remoteTargetSha: "",
    action: "unknown",
    commands: [],
    worktree: null,
    errors: [],
  };

  if (!tag) {
    result.errors.push("could not determine release tag; pass --tag vX.Y.Z");
  }
  if (!remote) {
    result.errors.push("could not determine git remote; pass --remote <name>");
  }
  if (result.errors.length > 0) {
    return result;
  }

  const expected = resolveExpectedCommit(options);
  if (expected.error) {
    result.errors.push(expected.error);
    return result;
  }
  result.expectedCommit = expected.sha;

  result.worktree = localWorktreeStatus(options);
  if (result.worktree.errors.length > 0) {
    result.errors.push(...result.worktree.errors);
    return result;
  }

  const remoteTag = remoteTagStatus({ ...options, tag, remote });
  if (remoteTag.error) {
    result.errors.push(remoteTag.error);
    return result;
  }
  result.remoteRefSha = remoteTag.refSha;
  result.remoteTargetSha = remoteTag.targetSha;

  if (remoteTag.exists && commitMatchesSha(remoteTag.targetSha, result.expectedCommit)) {
    result.action = "noop";
    return result;
  }

  result.action = remoteTag.exists ? "move" : "create";
  result.commands = tagCommands(result);
  return result;
}

function applyReleaseTagPlan(plan, options) {
  if (plan.action === "noop") {
    return [];
  }

  const push = plan.commands[0];
  const pushResult = run(options.runCommand, push.command, push.args);
  if (pushResult.status !== 0) {
    return [
      `remote tag push failed: ${pushResult.stderr || pushResult.stdout || "git exited non-zero"}`,
    ];
  }

  return [];
}

function resolveExpectedCommit(options) {
  const explicit = `${options.expectedCommit ?? ""}`.trim();
  const ref = explicit || "HEAD";
  const result = run(options.runCommand, "git", ["rev-parse", `${ref}^{commit}`]);
  if (result.status !== 0) {
    return {
      error: `expected commit ${ref} could not be resolved: ${
        result.stderr || result.stdout || "git exited non-zero"
      }`,
    };
  }
  const sha = result.stdout.trim();
  return sha ? { sha } : { error: `expected commit ${ref} resolved to an empty value` };
}

function remoteTagStatus(options) {
  const ref = `refs/tags/${options.tag}`;
  const result = run(options.runCommand, "git", [
    "ls-remote",
    "--tags",
    options.remote,
    ref,
    `${ref}^{}`,
  ]);
  if (result.status !== 0) {
    return {
      error: `remote tag lookup failed: ${result.stderr || result.stdout || "git exited non-zero"}`,
    };
  }

  const status = { exists: false, refSha: "", targetSha: "" };
  for (const line of result.stdout.split(/\r?\n/).filter(Boolean)) {
    const [sha, name] = line.trim().split(/\s+/);
    if (name === ref) {
      status.exists = true;
      status.refSha = sha;
      status.targetSha ||= sha;
    } else if (name === `${ref}^{}`) {
      status.targetSha = sha;
    }
  }
  return status;
}

function tagCommands(plan) {
  const ref = `refs/tags/${plan.tag}`;
  const pushArgs = plan.remoteRefSha
    ? [
        "push",
        `--force-with-lease=${ref}:${plan.remoteRefSha}`,
        plan.remote,
        `${plan.expectedCommit}:${ref}`,
      ]
    : ["push", plan.remote, `${plan.expectedCommit}:${ref}`];

  return [{ command: "git", args: pushArgs }];
}

function formatDryRunPlan(plan) {
  if (plan.action === "noop") {
    return (
      `release tag ${plan.tag} already points at ${shortSha(plan.expectedCommit)} ` +
      `on ${plan.remote}; no tag update needed.\n`
    );
  }

  return [
    `macOS release tag ${plan.action} plan for ${plan.tag}`,
    `expected commit: ${plan.expectedCommit}`,
    `current remote target: ${plan.remoteTargetSha ? shortSha(plan.remoteTargetSha) : "missing"}`,
    "",
    "Dry run only. Rerun with --apply after confirming this is the intended release commit.",
    "",
    "Commands:",
    ...plan.commands.map((command) => `  ${command.command} ${command.args.join(" ")}`),
    "",
  ].join("\n");
}

function formatAppliedPlan(plan) {
  if (plan.action === "noop") {
    return (
      `release tag ${plan.tag} already points at ${shortSha(plan.expectedCommit)} ` +
      `on ${plan.remote}; no tag update needed.\n`
    );
  }

  return (
    `release tag ${plan.tag} ${plan.action === "create" ? "created" : "moved"} ` +
    `to ${shortSha(plan.expectedCommit)} on ${plan.remote}\n`
  );
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--help"],
    valueArgs: ["--tag", "--remote", "--expected-commit", "--untracked-files"],
    booleanArgs: ["--apply", "--allow-dirty"],
  });
  const expectedCommit = valueArg(argv, "--expected-commit");
  const allowDirty = booleanArg(argv, "--allow-dirty", false);
  const apply = booleanArg(argv, "--apply", false);
  const untrackedFiles = valueArg(argv, "--untracked-files") ?? "normal";

  return {
    help: argv.includes("--help"),
    errors: [
      ...errors,
      ...dirtyReleaseIntentErrors({ allowDirty, expectedCommit }),
      ...applyIntentErrors({ apply, expectedCommit }),
      ...untrackedFileModeErrors(untrackedFiles),
    ],
    tag: valueArg(argv, "--tag"),
    remote: valueArg(argv, "--remote") ?? "origin",
    expectedCommit,
    apply,
    allowDirty,
    untrackedFiles,
  };
}

function applyIntentErrors(options) {
  if (!options.apply || `${options.expectedCommit ?? ""}`.trim()) {
    return [];
  }
  return ["--apply requires --expected-commit <sha> so tag moves are explicit"];
}

function untrackedFileModeErrors(value) {
  return ["normal", "all", "no"].includes(value)
    ? []
    : ["--untracked-files must be one of: normal, all, no"];
}

function usage() {
  return [
    "Usage: npm run release:macos:tag -- [options]",
    "",
    "Prints or applies the release tag move needed before Desktop Release dispatch.",
    "Dry run is the default. Applying requires --apply and --expected-commit.",
    "",
    "Options:",
    "  --tag <tag>                Release tag; defaults to the desktop package version",
    "  --remote <name>            Git remote to inspect and push; defaults to origin",
    "  --expected-commit <sha>    Commit the release tag must point at",
    "  --apply[=true|false]       Apply the remote tag update",
    "  --allow-dirty[=true|false] Allow dirty checks only with --expected-commit",
    "  --untracked-files <mode>   Git untracked inventory: normal, all, or no",
    "  --help                     Show this help without inspecting or updating tags",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
