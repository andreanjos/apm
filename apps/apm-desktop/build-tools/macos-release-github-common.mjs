import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const buildToolsDir = dirname(fileURLToPath(import.meta.url));
export const desktopRoot = resolve(buildToolsDir, "..");
export const desktopPackageJsonPath = resolve(desktopRoot, "package.json");
export const repoRoot = resolve(desktopRoot, "../..");
export const desktopReleaseWorkflowFile = "desktop-release.yml";
export const desktopReleaseWorkflowPath = ".github/workflows/desktop-release.yml";

export function ghJson(runCommand, args, label, input) {
  const result = run(runCommand, "gh", args, { input });
  if (result.status !== 0) {
    return {
      error: `${label} request failed: ${result.stderr || result.stdout || "gh exited non-zero"}`,
    };
  }

  try {
    return { value: JSON.parse(result.stdout) };
  } catch (error) {
    return { error: `${label} response was not valid JSON: ${errorMessage(error)}` };
  }
}

export function gitRemoteUrl(runCommand) {
  const result = run(runCommand, "git", ["remote", "get-url", "origin"]);
  return result.status === 0 ? result.stdout : "";
}

export function gitHeadCommit(runCommand) {
  const result = run(runCommand, "git", ["rev-parse", "HEAD"]);
  return result.status === 0 ? result.stdout.trim() : "";
}

export function gitWorkingTreeStatus(runCommand, options = {}) {
  const untrackedFiles = options.untrackedFiles ?? "normal";
  const result = run(runCommand, "git", [
    "status",
    "--porcelain",
    `--untracked-files=${untrackedFiles}`,
  ]);
  if (result.status !== 0) {
    return {
      changes: [],
      error: result.stderr || result.stdout || "git status exited non-zero",
      untrackedFiles,
    };
  }
  return {
    changes: result.stdout
      .split(/\r?\n/)
      .map((line) => line.trimEnd())
      .filter(Boolean),
    error: "",
    untrackedFiles,
  };
}

export function commitMatchesSha(actualSha, expectedSha) {
  const actual = `${actualSha ?? ""}`.trim().toLowerCase();
  const expected = `${expectedSha ?? ""}`.trim().toLowerCase();
  return Boolean(actual && expected && actual.startsWith(expected));
}

export function shortSha(sha) {
  return `${sha ?? ""}`.trim().slice(0, 12) || "unknown";
}

export function repoFromRemoteUrl(remoteUrl) {
  const trimmed = `${remoteUrl ?? ""}`.trim();
  const ssh = trimmed.match(/^git@github\.com:([^/]+\/[^/]+?)(?:\.git)?$/);
  if (ssh) {
    return ssh[1];
  }
  const https = trimmed.match(/^https:\/\/github\.com\/([^/]+\/[^/]+?)(?:\.git)?$/);
  if (https) {
    return https[1];
  }
  return null;
}

export function defaultReleaseTag(packageJsonPath = desktopPackageJsonPath) {
  try {
    const version = JSON.parse(readFileSync(packageJsonPath, "utf8")).version;
    return version ? `v${version}` : "";
  } catch {
    return "";
  }
}

export function run(runCommand, command, args, options = {}) {
  if (runCommand) {
    return runCommand(command, args, options);
  }
  return spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    input: options.input,
  });
}

export function valueArg(argv, name) {
  const prefix = `${name}=`;
  const inline = argv.find((arg) => arg.startsWith(prefix));
  if (inline) {
    return inline.slice(prefix.length);
  }
  const index = argv.indexOf(name);
  if (index < 0) {
    return undefined;
  }
  const value = argv[index + 1];
  return value && !value.startsWith("--") ? value : undefined;
}

export function booleanArg(argv, name, defaultValue) {
  const value = valueArg(argv, name);
  if (value !== undefined) {
    return /^(1|true|yes)$/i.test(value);
  }
  return argv.includes(name) ? true : defaultValue;
}

export function dirtyReleaseIntentErrors(options = {}) {
  if (!options.allowDirty) {
    return [];
  }
  if (`${options.expectedCommit ?? ""}`.trim()) {
    return [];
  }
  return [
    "--allow-dirty requires --expected-commit <sha> so dirty checks stay tied " +
      "to an explicit committed release",
  ];
}

export function argumentErrors(argv, spec = {}) {
  const valueArgs = new Set(spec.valueArgs ?? []);
  const booleanArgs = new Set(spec.booleanArgs ?? []);
  const flagArgs = new Set(spec.flagArgs ?? []);
  const errors = [];

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const [name, inlineValue] = splitArg(arg);

    if (!name.startsWith("--")) {
      errors.push(`unexpected argument value: ${arg}`);
    } else if (valueArgs.has(name)) {
      if (inlineValue !== null) {
        if (!inlineValue) {
          errors.push(`${name} requires a value`);
        }
      } else if (argv[index + 1] && !argv[index + 1].startsWith("--")) {
        index += 1;
      } else {
        errors.push(`${name} requires a value`);
      }
    } else if (booleanArgs.has(name)) {
      if (inlineValue !== null) {
        if (!isBooleanArgValue(inlineValue)) {
          errors.push(`invalid boolean value for ${name}: ${inlineValue}`);
        }
      } else if (isBooleanArgValue(argv[index + 1])) {
        index += 1;
      }
    } else if (flagArgs.has(name)) {
      if (inlineValue !== null) {
        errors.push(`${name} does not accept a value`);
      }
    } else {
      errors.push(`unknown argument: ${arg}`);
    }
  }

  return errors;
}

export function isMain(scriptUrl) {
  return process.argv[1] && resolve(process.argv[1]) === fileURLToPath(scriptUrl);
}

export function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function splitArg(arg) {
  const index = arg.indexOf("=");
  return index < 0 ? [arg, null] : [arg.slice(0, index), arg.slice(index + 1)];
}

function isBooleanArgValue(value) {
  return /^(0|1|false|true|no|yes)$/i.test(`${value ?? ""}`);
}
