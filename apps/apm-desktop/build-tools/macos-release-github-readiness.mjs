import { desktopWorkflowErrors } from "./macos-release.mjs";
import {
  commitMatchesSha,
  desktopReleaseWorkflowFile,
  desktopReleaseWorkflowPath,
  gitHeadCommit,
  gitWorkingTreeStatus,
  ghJson,
  run,
  shortSha,
} from "./macos-release-github-common.mjs";

export function localWorktreeErrors(options = {}) {
  return localWorktreeStatus(options).errors;
}

export function localWorktreeStatus(options = {}) {
  const status = {
    allowedDirty: Boolean(options.allowDirty),
    changes: [],
    errors: [],
    untrackedFiles: options.untrackedFiles ?? "normal",
  };

  const worktree = gitWorkingTreeStatus(options.runCommand, {
    untrackedFiles: status.untrackedFiles,
  });
  if (worktree.error) {
    status.errors.push(`local worktree status check failed: ${worktree.error.trim()}`);
    return status;
  }

  status.untrackedFiles = worktree.untrackedFiles;
  const changes = worktree.changes;
  status.changes = changes;
  if (changes.length === 0) {
    return status;
  }
  if (options.allowDirty) {
    return status;
  }

  const sample = changes.slice(0, 3).join(", ");
  const suffix = changes.length > 3 ? `, ... +${changes.length - 3} more` : "";
  status.errors.push(
    `working tree has uncommitted changes (${sample}${suffix}); ` +
      "commit or stash them before dispatching Desktop Release because the " +
      "workflow builds the release tag, not local files",
  );
  return status;
}

export function remoteDesktopWorkflowErrors(context, options = {}) {
  const workflow = ghJson(
    options.runCommand,
    ["api", `repos/${context.repo}/actions/workflows/${desktopReleaseWorkflowFile}`],
    "desktop release workflow",
  );
  if (workflow.error) {
    if (workflow.error.includes("HTTP 404")) {
      return [
        `desktop release workflow is not visible on ${context.repo}; ` +
          `merge ${desktopReleaseWorkflowPath} to the default branch before dispatching`,
      ];
    }
    return [workflow.error];
  }

  const errors = [];
  if (workflow.value?.path !== desktopReleaseWorkflowPath) {
    errors.push(`desktop release workflow path must be ${desktopReleaseWorkflowPath}`);
  }
  if (workflow.value?.state !== "active") {
    errors.push("desktop release workflow must be active in GitHub Actions");
  }

  const yaml = remoteWorkflowYaml(context, options);
  if (yaml.error) {
    errors.push(yaml.error);
  } else {
    errors.push(...desktopWorkflowErrors(yaml.value));
  }

  return errors;
}

export function releaseTagErrors(context, options = {}) {
  const tagRef = ghJson(
    options.runCommand,
    ["api", `repos/${context.repo}/git/ref/tags/${encodeURIComponent(context.tag)}`],
    `release tag ${context.tag}`,
  );
  if (tagRef.error) {
    if (tagRef.error.includes("HTTP 404")) {
      return [
        `release tag ${context.tag} is not present on ${context.repo}; ` +
          "push the release tag before dispatching Desktop Release",
      ];
    }
    return [tagRef.error];
  }

  const target = releaseTagTargetCommit(context, tagRef.value, options);
  if (target.error) {
    return [target.error];
  }

  const expectedCommit = expectedReleaseCommit(options);
  if (!expectedCommit) {
    return [];
  }

  const actual = target.sha.toLowerCase();
  const expected = expectedCommit.toLowerCase();
  if (commitMatchesSha(actual, expected)) {
    return [];
  }

  return [
    `release tag ${context.tag} points to ${shortSha(actual)}, but expected ` +
      `${shortSha(expected)}; ${releaseTagMismatchGuidance(
        context,
        actual,
        expected,
        options,
      )}`,
  ];
}

function releaseTagMismatchGuidance(context, actual, expected, options = {}) {
  if (`${options.expectedCommit ?? ""}`.trim()) {
    return (
      `move ${context.tag} to ${shortSha(expected)} or rerun with ` +
      `--expected-commit ${shortSha(actual)} if ${shortSha(actual)} is the ` +
      "intended release commit"
    );
  }

  return (
    "move the tag after merging the release commit or pass --expected-commit " +
    "<sha> when intentionally dispatching an older release"
  );
}

function remoteWorkflowYaml(context, options) {
  const args = [
    "workflow",
    "view",
    desktopReleaseWorkflowFile,
    "--repo",
    context.repo,
    "--yaml",
  ];
  const workflowRef = context.workflowRef ?? options.ref;
  if (workflowRef) {
    args.push("--ref", workflowRef);
  }

  const result = run(options.runCommand, "gh", args);
  if (result.status !== 0) {
    return {
      error: `desktop release workflow YAML read failed: ${
        result.stderr || result.stdout || "gh exited non-zero"
      }`,
    };
  }
  return { value: result.stdout };
}

function releaseTagTargetCommit(context, tagRef, options) {
  const object = tagRef?.object ?? {};
  if (!object.sha) {
    return { error: `release tag ${context.tag} response did not include a target sha` };
  }
  if (!object.type || object.type === "commit") {
    return { sha: object.sha };
  }
  if (object.type !== "tag") {
    return {
      error: `release tag ${context.tag} points to unsupported Git object type ${object.type}`,
    };
  }

  const tagObject = ghJson(
    options.runCommand,
    ["api", `repos/${context.repo}/git/tags/${object.sha}`],
    `annotated release tag ${context.tag}`,
  );
  if (tagObject.error) {
    return { error: tagObject.error };
  }
  const target = tagObject.value?.object ?? {};
  if (!target.sha) {
    return {
      error: `annotated release tag ${context.tag} response did not include a target sha`,
    };
  }
  if (target.type && target.type !== "commit") {
    return {
      error:
        `annotated release tag ${context.tag} points to unsupported Git object type ` +
        target.type,
    };
  }
  return { sha: target.sha };
}

function expectedReleaseCommit(options) {
  const explicit = `${options.expectedCommit ?? ""}`.trim();
  if (explicit) {
    return explicit;
  }

  return gitHeadCommit(options.runCommand);
}
