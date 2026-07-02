import { existsSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import {
  releaseAssetNames,
  verifyReleaseEvidenceManifest,
} from "./macos-release-assets.mjs";
import { releasePreflightErrors } from "./macos-release.mjs";
import { defaultReleaseEnvironment, githubEnvironmentCheckErrors } from "./macos-release-github-env.mjs";
import {
  localWorktreeStatus,
  releaseTagErrors,
  remoteDesktopWorkflowErrors,
} from "./macos-release-github-readiness.mjs";
import {
  argumentErrors,
  booleanArg,
  defaultReleaseTag,
  dirtyReleaseIntentErrors,
  errorMessage,
  gitRemoteUrl,
  isMain,
  repoRoot,
  repoFromRemoteUrl,
  run,
  shortSha,
  valueArg,
} from "./macos-release-github-common.mjs";

const defaultDesktopReleaseDir = resolve(repoRoot, "desktop-release");

if (isMain(import.meta.url)) {
  const status = runMacosReleaseStatusCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runMacosReleaseStatusCommand(argv = [], runtime = {}) {
  const log = runtime.log ?? console.log;
  const writeError = runtime.error ?? console.error;

  try {
    const options = {
      ...optionsFromArgs(argv),
      runCommand: runtime.runCommand,
      releasePreflightErrors: runtime.releasePreflightErrors,
    };
    if (options.help) {
      log(usage());
      return 0;
    }
    if (options.errors.length > 0) {
      writeFailure(writeError, "macOS release status failed:", options.errors);
      return 1;
    }

    const report = macosReleaseStatusReport(options);
    if (options.format === "json") {
      log(`${JSON.stringify(report, null, 2)}\n`);
    } else if (options.format === "markdown") {
      log(formatMacosReleaseStatusMarkdown(report));
    } else {
      log(formatMacosReleaseStatus(report));
    }
    if (options.check && !report.ready) {
      return 1;
    }
    return 0;
  } catch (error) {
    writeError(`macOS release status failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function macosReleaseStatusReport(options = {}) {
  const context = releaseStatusContext(options);
  const localEvidence =
    options.localReleaseEvidence ?? localReleaseEvidenceStatus(context, options);
  const localWorktree = options.localWorktree ?? localWorktreeStatus(options);
  const checks = [
    releaseStatusCheck("context", "Release status context", context.errors),
    releaseStatusCheck(
      "local_release_preflight",
      "Local release preflight",
      localReleasePreflightErrors(options),
    ),
    releaseStatusCheck(
      "local_release_evidence",
      "Local release evidence",
      localEvidence.errors,
    ),
    releaseStatusCheck(
      "local_worktree",
      "Local release worktree",
      localWorktree.errors,
    ),
  ];

  if (context.errors.length === 0) {
    checks.push(
      releaseStatusCheck(
        "remote_desktop_workflow",
        "Remote Desktop Release workflow",
        remoteDesktopWorkflowErrors(context, options),
      ),
      releaseStatusCheck(
        "release_environment_secrets",
        "GitHub release environment secrets",
        githubEnvironmentCheckErrors({
          ...options,
          repo: context.repo,
          environment: context.environment,
        }),
      ),
      releaseStatusCheck(
        "release_tag",
        "Release tag",
        releaseTagErrors(context, options),
      ),
    );
  }

  const blockers = checks.flatMap((check) =>
    check.errors.map((error) => `${check.label}: ${error}`),
  );
  const nextSteps = releaseStatusNextSteps(checks, context);
  return {
    ready: blockers.length === 0,
    repo: context.repo,
    tag: context.tag,
    environment: context.environment,
    localSecretTemplate: context.localSecretTemplate,
    localEvidence,
    localWorktree,
    checks,
    blockers,
    nextSteps,
  };
}

export function formatMacosReleaseStatus(report) {
  const lines = [
    `macOS desktop release status: ${report.ready ? "ready" : "not ready"}`,
    `repo: ${report.repo || "unknown"}`,
    `tag: ${report.tag || "unknown"}`,
    `environment: ${report.environment}`,
    "",
  ];

  lines.push(...formatLocalEvidenceText(report.localEvidence));
  lines.push(...formatLocalWorktreeText(report.localWorktree));

  for (const check of report.checks) {
    lines.push(`- [${check.status}] ${check.label}`);
    for (const error of check.errors) {
      lines.push(`  - ${error}`);
    }
  }

  if (report.blockers.length > 0) {
    lines.push("", "Blockers:");
    for (const blocker of report.blockers) {
      lines.push(`- ${blocker}`);
    }
  }

  if (report.nextSteps?.length > 0) {
    lines.push("", "Next steps:");
    for (const step of report.nextSteps) {
      lines.push(`- ${step}`);
    }
  }

  return `${lines.join("\n")}\n`;
}

export function formatMacosReleaseStatusMarkdown(report) {
  const lines = [
    "# macOS Desktop Release Status",
    "",
    `- Status: ${report.ready ? "ready" : "not ready"}`,
    `- Repo: \`${markdownText(report.repo || "unknown")}\``,
    `- Tag: \`${markdownText(report.tag || "unknown")}\``,
    `- Environment: \`${markdownText(report.environment)}\``,
    "",
    "## Local Evidence",
    "",
    ...formatLocalEvidenceMarkdown(report.localEvidence),
    "",
    ...formatLocalWorktreeMarkdown(report.localWorktree),
    ...(hasLocalWorktreeChanges(report.localWorktree) ? [""] : []),
    "## Checks",
    "",
  ];

  for (const check of report.checks) {
    lines.push(`- [${check.status === "pass" ? "x" : " "}] ${markdownText(check.label)}`);
    for (const error of check.errors) {
      lines.push(`  - ${markdownText(error)}`);
    }
  }

  lines.push("", "## Blockers", "");
  if (report.blockers.length === 0) {
    lines.push("- None");
  } else {
    for (const blocker of report.blockers) {
      lines.push(`- ${markdownText(blocker)}`);
    }
  }

  lines.push("", "## Next Steps", "");
  if (report.nextSteps?.length > 0) {
    report.nextSteps.forEach((step, index) => {
      lines.push(`${index + 1}. ${markdownText(step)}`);
    });
  } else {
    lines.push("- None");
  }

  return `${lines.join("\n")}\n`;
}

function markdownText(value) {
  return `${value ?? ""}`
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

export function localReleaseEvidenceStatus(context = {}, options = {}) {
  const version = releaseEvidenceVersion(context.tag);
  const outputDir = options.desktopReleaseDir ?? defaultDesktopReleaseDir;
  const result = {
    path: "",
    exists: false,
    generatedAt: "",
    artifacts: [],
    errors: [],
  };

  if (!version) {
    result.errors.push("release tag is required before checking local release evidence");
    return result;
  }

  result.path = resolve(outputDir, releaseAssetNames(version).evidence);
  result.exists = (options.existsSync ?? existsSync)(result.path);
  if (!result.exists) {
    result.errors.push(
      `missing local release evidence JSON: ${result.path}; run npm run verify:v3:local`,
    );
    return result;
  }

  result.errors.push(...verifyReleaseEvidenceManifest(result.path, outputDir, version));
  if (result.errors.length > 0) {
    return result;
  }

  const evidence = JSON.parse(readFileSync(result.path, "utf8"));
  result.generatedAt = evidence.generated_at ?? "";
  result.artifacts = evidence.artifacts.map((artifact) => ({
    role: artifact.role,
    filename: artifact.filename,
    sha256: artifact.sha256,
  }));
  return result;
}

function releaseEvidenceVersion(tag) {
  return `${tag ?? ""}`.trim().replace(/^v/, "");
}

function formatLocalEvidenceText(evidence) {
  if (!evidence?.exists || evidence.errors?.length > 0) {
    return ["local evidence: unavailable", ""];
  }

  return [
    `local evidence: ${evidence.generatedAt || "unknown generated_at"}`,
    ...evidence.artifacts.map((artifact) =>
      `  - ${artifact.filename}: ${artifact.sha256}`,
    ),
    "",
  ];
}

function formatLocalEvidenceMarkdown(evidence) {
  if (!evidence?.exists || evidence.errors?.length > 0) {
    return ["- Unavailable; see the Local release evidence check."];
  }

  return [
    `- Generated At: \`${markdownText(evidence.generatedAt || "unknown")}\``,
    ...evidence.artifacts.map((artifact) =>
      `- \`${markdownText(artifact.filename)}\`: ` +
        `\`${markdownText(artifact.sha256)}\``,
    ),
  ];
}

function formatLocalWorktreeText(worktree) {
  if (!hasLocalWorktreeChanges(worktree)) {
    return [];
  }

  return [
    `local worktree changes${worktreeModeSuffix(worktree)}:`,
    ...worktree.changes.map((change) => `  - ${change}`),
    "",
  ];
}

function formatLocalWorktreeMarkdown(worktree) {
  if (!hasLocalWorktreeChanges(worktree)) {
    return [];
  }

  return [
    `## Local Worktree Changes${worktreeModeSuffix(worktree)}`,
    "",
    ...worktree.changes.map((change) => `- \`${markdownText(change)}\``),
  ];
}

function hasLocalWorktreeChanges(worktree) {
  return Array.isArray(worktree?.changes) && worktree.changes.length > 0;
}

function worktreeModeSuffix(worktree) {
  const mode = worktree?.untrackedFiles ?? "normal";
  return mode === "normal" ? "" : ` (--untracked-files=${mode})`;
}

function releaseStatusContext(options) {
  const repo = options.repo ?? repoFromRemoteUrl(gitRemoteUrl(options.runCommand));
  const tag = options.tag ?? defaultReleaseTag(options.desktopPackageJsonPath);
  const environment = options.environment ?? defaultReleaseEnvironment;
  const expectedCommit = `${options.expectedCommit ?? ""}`.trim();
  const localSecretTemplate =
    options.localSecretTemplate ?? localSecretTemplateStatus(options);
  const errors = [];

  if (!repo) {
    errors.push("could not determine GitHub repository; pass --repo owner/name");
  }
  if (!tag) {
    errors.push("could not determine release tag; pass --tag vX.Y.Z");
  }

  return { repo, tag, environment, expectedCommit, localSecretTemplate, errors };
}

function releaseStatusCheck(id, label, errors) {
  return {
    id,
    label,
    status: errors.length === 0 ? "pass" : "fail",
    errors,
  };
}

function releaseStatusNextSteps(checks, context) {
  return checks
    .filter((check) => check.errors.length > 0)
    .map((check) => releaseStatusNextStep(check.id, context))
    .filter(Boolean);
}

function releaseStatusNextStep(checkId, context) {
  switch (checkId) {
    case "context":
      return "Pass --repo owner/name and --tag vX.Y.Z when they cannot be derived locally.";
    case "local_release_preflight":
      return "Run npm run release:macos:check and fix the listed local release preflight errors.";
    case "local_release_evidence":
      return "Run npm run verify:v3:local to regenerate local release evidence before handoff.";
    case "local_worktree":
      return (
        "Commit or stash local changes before dispatching Desktop Release; use " +
        "--allow-dirty only with --expected-commit for an intentional older committed release."
      );
    case "remote_desktop_workflow":
      return "Merge and push .github/workflows/desktop-release.yml to the remote default branch.";
    case "release_environment_secrets":
      return releaseEnvironmentSecretNextStep(context);
    case "release_tag":
      return releaseTagNextStep(context);
    default:
      return "";
  }
}

function releaseTagNextStep(context) {
  if (context.expectedCommit) {
    return (
      `Move or recreate ${context.tag} so it points at ` +
      `${shortSha(context.expectedCommit)}, or rerun with the intended committed ` +
      "release SHA."
    );
  }

  return (
    `Move or recreate ${context.tag} after the release commit lands, or pass ` +
    "--expected-commit <sha> for an intentional older release."
  );
}

function releaseEnvironmentSecretNextStep(context) {
  const template = context.localSecretTemplate;
  const path = template?.displayPath ?? "../../.env.release.local";
  const validate = `npm run release:macos:github-secrets -- --repo ${context.repo}`;
  if (template?.exists && template.ignored && template.private) {
    return (
      `Fill and source the existing ${path}, run ${validate}, ` +
      "then rerun it with --apply after the dry run passes."
    );
  }
  if (template?.exists) {
    return (
      `Fix ${path} so it is ignored by Git and mode 600, then fill and source it, ` +
      `run ${validate}, then rerun it with --apply after the dry run passes.`
    );
  }
  return (
    `Run npm run release:macos:github-secrets-template -- --output ${path}, ` +
    `fill and source that local env file, run ${validate}, ` +
    "then rerun it with --apply after the dry run passes."
  );
}

function localSecretTemplateStatus(options = {}) {
  const path = options.localSecretTemplatePath ?? resolve(repoRoot, ".env.release.local");
  const displayPath = options.localSecretTemplateDisplayPath ?? "../../.env.release.local";
  const gitPath = options.localSecretTemplateGitPath ?? ".env.release.local";
  const exists = (options.existsSync ?? existsSync)(path);
  const ignored = gitIgnored(gitPath, options);
  const result = {
    path,
    displayPath,
    exists,
    ignored: ignored.value,
    private: false,
    error: ignored.error,
  };

  if (!exists) {
    return result;
  }

  try {
    result.private = ((options.statSync ?? statSync)(path).mode & 0o077) === 0;
  } catch (error) {
    result.error = errorMessage(error);
  }
  return result;
}

function gitIgnored(path, options) {
  const result = run(options.runCommand, "git", ["check-ignore", "-q", "--", path]);
  if (result.status === 0) {
    return { value: true, error: "" };
  }
  if (result.status === 1) {
    return { value: false, error: "" };
  }
  return {
    value: false,
    error: result.stderr || result.stdout || "git check-ignore exited non-zero",
  };
}

function localReleasePreflightErrors(options) {
  const preflight = options.releasePreflightErrors ?? releasePreflightErrors;
  return preflight({
    ...options,
    checkOnly: true,
  });
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--check", "--json", "--markdown", "--help"],
    valueArgs: [
      "--repo",
      "--tag",
      "--ref",
      "--expected-commit",
      "--environment",
      "--untracked-files",
    ],
    booleanArgs: ["--allow-dirty"],
  });

  const format = outputFormatFromArgs(argv);
  const expectedCommit = valueArg(argv, "--expected-commit");
  const allowDirty = booleanArg(argv, "--allow-dirty", false);
  const untrackedFiles = valueArg(argv, "--untracked-files") ?? "normal";
  return {
    help: argv.includes("--help"),
    errors: [
      ...errors,
      ...outputFormatErrors(argv),
      ...untrackedFileModeErrors(untrackedFiles),
      ...dirtyReleaseIntentErrors({ allowDirty, expectedCommit }),
    ],
    check: argv.includes("--check"),
    format,
    repo: valueArg(argv, "--repo"),
    tag: valueArg(argv, "--tag"),
    ref: valueArg(argv, "--ref"),
    expectedCommit,
    environment: valueArg(argv, "--environment"),
    allowDirty,
    untrackedFiles,
  };
}

function outputFormatFromArgs(argv) {
  const json = argv.includes("--json");
  const markdown = argv.includes("--markdown");
  if (json) {
    return "json";
  }
  if (markdown) {
    return "markdown";
  }
  return "text";
}

function outputFormatErrors(argv) {
  return argv.includes("--json") && argv.includes("--markdown")
    ? ["pass only one status output format: --json or --markdown"]
    : [];
}

function untrackedFileModeErrors(value) {
  return ["normal", "all", "no"].includes(value)
    ? []
    : [`--untracked-files must be one of: normal, all, no`];
}

function usage() {
  return [
    "Usage: npm run release:macos:status -- [options]",
    "",
    "Prints non-dispatching macOS Desktop Release readiness status.",
    "",
    "Options:",
    "  --repo <owner/name>       GitHub repository; defaults to origin remote",
    "  --tag <tag>               Release tag; defaults to the desktop package version",
    "  --ref <ref>               Git ref used when checking the remote workflow",
    "  --expected-commit <sha>   Commit the release tag or workflow run must match",
    "  --environment <name>      GitHub Environment for release secrets",
    "  --allow-dirty[=bool]      Allow dirty checks only with --expected-commit",
    "  --untracked-files <mode>  Git untracked inventory: normal, all, or no",
    "  --check                   Exit non-zero when blockers remain",
    "  --json                    Print JSON status for automation",
    "  --markdown                Print paste-ready markdown handoff notes",
    "  --help                    Show this help without running release checks",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
