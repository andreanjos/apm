import { requiredReleaseEnvironmentSecrets } from "./macos-release.mjs";
import {
  argumentErrors,
  errorMessage,
  ghJson,
  gitRemoteUrl,
  isMain,
  repoFromRemoteUrl,
  valueArg,
} from "./macos-release-github-common.mjs";

export const defaultReleaseEnvironment = "macos-desktop-release";

export { repoFromRemoteUrl } from "./macos-release-github-common.mjs";

if (isMain(import.meta.url)) {
  const status = runGithubEnvironmentCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runGithubEnvironmentCommand(argv = [], runtime = {}) {
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
      writeFailure(
        writeError,
        "macOS release GitHub Environment command failed:",
        options.errors,
      );
      return 1;
    }

    const action = options.create ? "bootstrap" : "check";
    const errors = options.create
      ? githubEnvironmentBootstrapErrors(options)
      : githubEnvironmentCheckErrors(options);
    if (errors.length > 0) {
      writeFailure(writeError, `macOS release GitHub Environment ${action} failed:`, errors);
      return 1;
    }

    log(
      options.create
        ? "macOS release GitHub Environment bootstrap passed"
        : "macOS release GitHub Environment check passed",
    );
    if (options.create) {
      log(
        "Add required environment secrets, then run npm run release:macos:github-check",
      );
    }
    return 0;
  } catch (error) {
    writeError(`macOS release GitHub Environment command failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function githubEnvironmentBootstrapErrors(options = {}) {
  const context = githubEnvironmentContext(options);
  if (context.errors.length > 0) {
    return context.errors;
  }

  const env = ghJson(
    options.runCommand,
    [
      "api",
      "--method",
      "PUT",
      `repos/${context.repo}/environments/${encodeURIComponent(context.environment)}`,
      "--input",
      "-",
    ],
    `GitHub Environment ${context.environment} bootstrap`,
    `${JSON.stringify(environmentBootstrapRequest())}\n`,
  );
  if (env.error) {
    return [env.error];
  }
  if (env.value?.name && env.value.name !== context.environment) {
    return [`GitHub Environment name must be ${context.environment}`];
  }

  return [];
}

export function githubEnvironmentCheckErrors(options = {}) {
  const context = githubEnvironmentContext(options);
  const environment = context.environment;
  const repo = context.repo;
  const errors = [];

  if (context.errors.length > 0) {
    return context.errors;
  }

  const env = ghJson(
    options.runCommand,
    ["api", `repos/${repo}/environments/${encodeURIComponent(environment)}`],
    `GitHub Environment ${environment}`,
  );
  if (env.error) {
    if (env.error.includes("HTTP 404")) {
      const requiredSecrets = requiredReleaseEnvironmentSecrets().join(", ");
      return [
        `GitHub Environment ${environment} was not found for ${repo}. ` +
          `Create it and configure required secrets: ${requiredSecrets}`,
      ];
    }
    return [env.error];
  } else if (env.value?.name !== environment) {
    errors.push(`GitHub Environment name must be ${environment}`);
  }

  const secrets = ghJson(
    options.runCommand,
    ["api", `repos/${repo}/environments/${encodeURIComponent(environment)}/secrets`],
    `GitHub Environment ${environment} secrets`,
  );
  if (secrets.error) {
    errors.push(secrets.error);
  } else {
    errors.push(...secretInventoryErrors(secrets.value, environment));
  }

  return errors;
}

export function environmentBootstrapRequest() {
  return {
    wait_timer: 0,
    deployment_branch_policy: null,
  };
}

export function secretInventoryErrors(secretInventory, environment) {
  if (!Array.isArray(secretInventory?.secrets)) {
    return [`GitHub Environment ${environment} secrets response must include a secrets array`];
  }

  const names = new Set(secretInventory.secrets.map((secret) => secret?.name));
  return requiredReleaseEnvironmentSecrets()
    .filter((secret) => !names.has(secret))
    .map((secret) => `GitHub Environment ${environment} is missing secret ${secret}`);
}

function githubEnvironmentContext(options) {
  const environment = options.environment ?? defaultReleaseEnvironment;
  const repo = options.repo ?? repoFromRemoteUrl(gitRemoteUrl(options.runCommand));
  const errors = repo ? [] : ["could not determine GitHub repository; pass --repo owner/name"];
  return { environment, repo, errors };
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--create", "--help"],
    valueArgs: ["--repo", "--environment"],
  });

  return {
    help: argv.includes("--help"),
    errors,
    create: argv.includes("--create"),
    repo: valueArg(argv, "--repo"),
    environment: valueArg(argv, "--environment"),
  };
}

function usage() {
  return [
    "Usage: npm run release:macos:github-check -- [options]",
    "       npm run release:macos:github-bootstrap -- [options]",
    "",
    "Checks or bootstraps the macOS Desktop Release GitHub Environment.",
    "",
    "Options:",
    "  --create              Create or update the GitHub Environment shell",
    "  --repo <owner/name>   GitHub repository; defaults to origin remote",
    "  --environment <name>  GitHub Environment for release secrets",
    "  --help                Show this help without checking or bootstrapping",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
