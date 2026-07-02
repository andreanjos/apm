import { writeFileSync } from "node:fs";
import { requiredReleaseEnvironmentSecrets } from "./macos-release.mjs";
import {
  defaultReleaseEnvironment,
  githubEnvironmentBootstrapErrors,
  githubEnvironmentCheckErrors,
} from "./macos-release-github-env.mjs";
import {
  argumentErrors,
  defaultReleaseTag,
  errorMessage,
  gitRemoteUrl,
  isMain,
  repoFromRemoteUrl,
  run,
  valueArg,
} from "./macos-release-github-common.mjs";

if (isMain(import.meta.url)) {
  const status = runGithubSecretCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runGithubSecretCommand(argv = [], runtime = {}) {
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
        "macOS release GitHub Environment secret setup failed:",
        options.errors,
      );
      return 1;
    }

    if (options.printTemplate) {
      const result = writeGithubSecretTemplate(options);
      if (result.errors.length > 0) {
        writeFailure(
          writeError,
          "macOS release GitHub Environment secret template failed:",
          result.errors,
        );
        return 1;
      }
      if (result.written) {
        log(`wrote macOS release GitHub Environment secret template: ${result.path}`);
      } else {
        log(result.template);
      }
      return 0;
    }

    const errors = githubSecretInstallErrors(options);
    if (errors.length > 0) {
      writeFailure(
        writeError,
        "macOS release GitHub Environment secret setup failed:",
        errors,
      );
      return 1;
    }

    if (options.apply) {
      log("macOS release GitHub Environment secrets stored");
      log("Remote secret inventory verified");
    } else {
      log("macOS release GitHub Environment secret inputs are valid");
      log("Dry run only; pass --apply to store secrets in GitHub");
    }
    return 0;
  } catch (error) {
    writeError(`macOS release GitHub Environment secret setup failed: ${errorMessage(error)}`);
    return 1;
  }
}

export function githubSecretInstallErrors(options = {}) {
  const context = githubSecretInstallContext(options);
  if (context.errors.length > 0) {
    return context.errors;
  }

  const entries = secretInstallEntries(options.env ?? process.env);
  const entryErrors = secretInstallEntryErrors(entries);
  if (entryErrors.length > 0) {
    return entryErrors;
  }

  if (!options.apply) {
    return [];
  }

  const bootstrapErrors = githubEnvironmentBootstrapErrors({
    ...options,
    repo: context.repo,
    environment: context.environment,
  });
  if (bootstrapErrors.length > 0) {
    return bootstrapErrors;
  }

  const errors = [];
  for (const entry of entries) {
    const result = run(
      options.runCommand,
      "gh",
      [
        "secret",
        "set",
        entry.name,
        "--repo",
        context.repo,
        "--env",
        context.environment,
      ],
      { input: entry.value },
    );
    if (result.status !== 0) {
      errors.push(
        `${entry.name} upload failed: ${result.stderr || result.stdout || "gh exited non-zero"}`,
      );
    }
  }

  if (errors.length > 0) {
    return errors;
  }

  return githubEnvironmentCheckErrors({
    ...options,
    repo: context.repo,
    environment: context.environment,
  });
}

export function secretInstallEntries(env = process.env) {
  return requiredReleaseEnvironmentSecrets().map((name) => {
    const value = env[name] ?? "";
    return {
      name,
      present: value.length > 0,
      value,
    };
  });
}

export function githubSecretTemplate(options = {}) {
  const tag = (options.tag ?? defaultReleaseTag(options.desktopPackageJsonPath)) || "v<version>";
  const repo = options.repo ?? "andreanjos/apm";
  const lines = [
    "# apm macOS desktop release secrets",
    "# Save this to an ignored file such as ../../.env.release.local.",
    "# Prefer: npm run release:macos:github-secrets-template -- --output ../../.env.release.local",
    "# Fill these values locally, source the file, then run the dry run first:",
    "# source ../../.env.release.local",
    `# npm run release:macos:github-secrets -- --repo ${repo}`,
    "# Upload only after the dry run passes, then verify readiness:",
    `# npm run release:macos:github-secrets -- --repo ${repo} --apply`,
    `# npm run release:macos:github-check -- --repo ${repo}`,
    `# npm run release:macos:status -- --repo ${repo} --tag ${tag} --markdown`,
    "#",
    "# Generate file-backed base64 values with:",
    '# export APM_MACOS_CERTIFICATE_BASE64="$(base64 -i /path/to/DeveloperIDApplication.p12 | tr -d \'\\n\')"',
    '# export APPLE_API_KEY_BASE64="$(base64 -i /path/to/AuthKey_XXXXXXXXXX.p8 | tr -d \'\\n\')"',
    "",
  ];

  for (const secret of requiredReleaseEnvironmentSecrets()) {
    lines.push(`# ${secretDescription(secret)}`);
    lines.push(`export ${secret}=""`);
    lines.push("");
  }

  return `${lines.join("\n").trimEnd()}\n`;
}

export function writeGithubSecretTemplate(options = {}) {
  const template = githubSecretTemplate(options);
  if (!options.output) {
    return { written: false, template, errors: [] };
  }

  try {
    writeFileSync(options.output, template, {
      encoding: "utf8",
      flag: "wx",
      mode: 0o600,
    });
    return { written: true, path: options.output, template: "", errors: [] };
  } catch (error) {
    if (error?.code === "EEXIST") {
      return {
        written: false,
        template: "",
        errors: [`refusing to overwrite existing secret template: ${options.output}`],
      };
    }
    return {
      written: false,
      template: "",
      errors: [
        `could not write secret template to ${options.output}: ${errorMessage(error)}`,
      ],
    };
  }
}

export function secretInstallEntryErrors(entries) {
  return entries.flatMap((entry) => {
    if (!entry.present) {
      return [`${entry.name} must be set in the local environment before uploading secrets`];
    }
    return secretValueErrors(entry.name, entry.value);
  });
}

export function secretValueErrors(name, value) {
  if (name === "APM_MACOS_SIGNING_IDENTITY") {
    return value.startsWith("Developer ID Application:")
      ? []
      : ["APM_MACOS_SIGNING_IDENTITY must start with Developer ID Application:"];
  }
  if (name === "APM_MACOS_PROVIDER_SHORT_NAME") {
    return /^[A-Z0-9]{3,20}$/.test(value)
      ? []
      : ["APM_MACOS_PROVIDER_SHORT_NAME must be an Apple provider short name"];
  }
  if (name === "APPLE_API_KEY") {
    return /^[A-Z0-9]{10}$/.test(value)
      ? []
      : ["APPLE_API_KEY must be a 10-character App Store Connect key ID"];
  }
  if (name === "APPLE_API_ISSUER") {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value)
      ? []
      : ["APPLE_API_ISSUER must be an App Store Connect issuer UUID"];
  }
  if (name === "APM_MACOS_CERTIFICATE_BASE64") {
    const decoded = decodedBase64(name, value);
    if (decoded.error) {
      return [decoded.error];
    }
    return decoded.value[0] === 0x30
      ? []
      : ["APM_MACOS_CERTIFICATE_BASE64 must decode to a DER .p12 payload"];
  }
  if (name === "APPLE_API_KEY_BASE64") {
    const decoded = decodedBase64(name, value);
    if (decoded.error) {
      return [decoded.error];
    }
    const text = decoded.value.toString("utf8");
    return text.includes("-----BEGIN PRIVATE KEY-----") &&
      text.includes("-----END PRIVATE KEY-----")
      ? []
      : ["APPLE_API_KEY_BASE64 must decode to an App Store Connect private key"];
  }
  return [];
}

function secretDescription(name) {
  return {
    APM_MACOS_CERTIFICATE_BASE64: "base64-encoded .p12 Developer ID Application certificate",
    APM_MACOS_CERTIFICATE_PASSWORD: "password for the .p12 certificate",
    APM_MACOS_KEYCHAIN_PASSWORD: "temporary CI keychain password",
    APM_MACOS_SIGNING_IDENTITY: "exact Developer ID Application: signing identity",
    APM_MACOS_PROVIDER_SHORT_NAME: "Apple notarization provider short name",
    APPLE_API_KEY: "10-character App Store Connect API key ID",
    APPLE_API_ISSUER: "App Store Connect issuer UUID",
    APPLE_API_KEY_BASE64: "base64-encoded AuthKey_*.p8 contents",
  }[name] ?? "required release secret";
}

function decodedBase64(name, value) {
  const clean = value.replace(/\s+/g, "");
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(clean)) {
    return { error: `${name} must be base64 encoded` };
  }

  const decoded = Buffer.from(clean, "base64");
  const encoded = decoded.toString("base64").replace(/=+$/, "");
  const normalized = clean.replace(/=+$/, "");
  if (decoded.length === 0 || encoded !== normalized) {
    return { error: `${name} must be base64 encoded` };
  }

  return { value: decoded };
}

function githubSecretInstallContext(options) {
  const environment = options.environment ?? defaultReleaseEnvironment;
  const repo = options.repo ?? repoFromRemoteUrl(gitRemoteUrl(options.runCommand));
  const errors = repo ? [] : ["could not determine GitHub repository; pass --repo owner/name"];
  return { environment, repo, errors };
}

function optionsFromArgs(argv) {
  const errors = argumentErrors(argv, {
    flagArgs: ["--apply", "--print-template", "--template", "--help"],
    valueArgs: ["--output", "--repo", "--tag", "--environment"],
  });

  return {
    help: argv.includes("--help"),
    errors,
    apply: argv.includes("--apply"),
    printTemplate: argv.includes("--print-template") || argv.includes("--template"),
    output: valueArg(argv, "--output"),
    repo: valueArg(argv, "--repo"),
    tag: valueArg(argv, "--tag"),
    environment: valueArg(argv, "--environment"),
  };
}

function usage() {
  return [
    "Usage: npm run release:macos:github-secrets -- [options]",
    "       npm run release:macos:github-secrets-template -- [options]",
    "",
    "Validates, writes, or uploads macOS Desktop Release GitHub Environment secrets.",
    "",
    "Options:",
    "  --apply                 Upload validated local secrets to GitHub",
    "  --print-template        Print a local .env release secret template",
    "  --template              Alias for --print-template",
    "  --output <path>         Write the template without overwriting existing files",
    "  --repo <owner/name>     GitHub repository; defaults to origin remote for uploads",
    "  --tag <tag>             Release tag to include in generated template guidance",
    "  --environment <name>    GitHub Environment for release secrets",
    "  --help                  Show this help without validating, writing, or uploading",
  ].join("\n");
}

function writeFailure(writeError, header, errors) {
  writeError(header);
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
