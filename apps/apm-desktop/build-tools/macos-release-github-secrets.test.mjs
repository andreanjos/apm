import {
  githubSecretInstallErrors,
  githubSecretTemplate,
  parseSecretEnvFile,
  releaseSecretEnvFileStatus,
  runGithubSecretCommand,
  secretEnvironment,
  secretInstallEntries,
  secretValueErrors,
  writeGithubSecretTemplate,
} from "./macos-release-github-secrets.mjs";
import { requiredReleaseEnvironmentSecrets } from "./macos-release.mjs";
import { defaultReleaseTag, repoRoot } from "./macos-release-github-common.mjs";
import {
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

const tests = [];

test("reports missing local secret inputs", () => {
  const errors = githubSecretInstallErrors({
    repo: "andreanjos/apm",
    env: completeSecretEnv({ APPLE_API_KEY_BASE64: "" }),
  }).join("\n");

  assertIncludes(errors, "APPLE_API_KEY_BASE64", "missing secret input");
});

test("validates local secret value shapes before upload", () => {
  const errors = githubSecretInstallErrors({
    repo: "andreanjos/apm",
    env: completeSecretEnv({
      APM_MACOS_SIGNING_IDENTITY: "Apple Development: Example",
      APPLE_API_KEY_BASE64: base64("not a private key"),
    }),
  }).join("\n");

  assertIncludes(errors, "Developer ID Application", "signing identity shape");
  assertIncludes(errors, "private key", "api key shape");
});

test("dry run accepts complete local secret inputs without gh calls", () => {
  let callCount = 0;
  assertDeepEqual(
    githubSecretInstallErrors({
      repo: "andreanjos/apm",
      env: completeSecretEnv(),
      runCommand: () => {
        callCount += 1;
        return { status: 1, stdout: "", stderr: "should not run" };
      },
    }),
    [],
    "dry run errors",
  );
  assertEqual(callCount, 0, "dry run command calls");
});

test("reads complete local secret inputs from a private env file", () => {
  withTempDir((dir) => {
    const envFile = writeCompleteSecretEnvFile(resolve(dir, ".env.release.local"));

    const secretEnv = secretEnvironment({ envFile });
    assertDeepEqual(secretEnv.errors, [], "env file errors");
    assertEqual(
      secretEnv.values.APM_MACOS_SIGNING_IDENTITY,
      completeSecretEnv().APM_MACOS_SIGNING_IDENTITY,
      "parsed signing identity",
    );

    assertDeepEqual(
      githubSecretInstallErrors({
        repo: "andreanjos/apm",
        envFile,
        runCommand: () => {
          throw new Error("dry run should not call gh or git for external private env files");
        },
      }),
      [],
      "env-file dry run errors",
    );
  });
});

test("uploads local secret inputs from an env file through gh stdin", () => {
  withTempDir((dir) => {
    const calls = [];
    const envFile = writeCompleteSecretEnvFile(resolve(dir, ".env.release.local"));

    assertDeepEqual(
      githubSecretInstallErrors({
        repo: "andreanjos/apm",
        envFile,
        apply: true,
        runCommand: fakeGh(calls),
      }),
      [],
      "env-file upload errors",
    );

    const firstSecret = calls.find((call) => call.args[0] === "secret");
    assertEqual(
      firstSecret.options.input,
      completeSecretEnv().APM_MACOS_CERTIFICATE_BASE64,
      "stdin from env file",
    );
  });
});

test("reports missing local secret inputs from the selected source", () => {
  withTempDir((dir) => {
    const envFile = writeCompleteSecretEnvFile(resolve(dir, ".env.release.local"), {
      APPLE_API_KEY_BASE64: "",
    });

    const errors = githubSecretInstallErrors({
      repo: "andreanjos/apm",
      envFile,
    }).join("\n");

    assertIncludes(errors, "APPLE_API_KEY_BASE64", "missing env-file secret");
    assertIncludes(errors, `release secret env file ${envFile}`, "env-file source");
  });
});

test("accepts repo-local secret env files only when private and ignored", () => {
  const calls = [];
  const status = releaseSecretEnvFileStatus(resolve(repoRoot, ".env.release.local"), {
    existsSync: () => true,
    statSync: () => ({ mode: 0o600 }),
    runCommand: (command, args) => {
      calls.push({ command, args });
      return { status: 0, stdout: "", stderr: "" };
    },
  });

  assertDeepEqual(status.errors, [], "repo-local env-file errors");
  assertEqual(status.ignored, true, "repo-local env-file ignored");
  assertDeepEqual(
    calls,
    [{ command: "git", args: ["check-ignore", "-q", "--", ".env.release.local"] }],
    "git ignore check",
  );
});

test("rejects unsafe or tracked local secret env files", () => {
  const unsafe = releaseSecretEnvFileStatus("tracked.env", {
    existsSync: () => true,
    statSync: () => ({ mode: 0o644 }),
    runCommand: () => ({ status: 1, stdout: "", stderr: "" }),
  }).errors.join("\n");

  assertIncludes(unsafe, "private mode 600", "private mode error");
  assertIncludes(unsafe, "ignored by git", "gitignore error");
});

test("reports local secret env file parse errors", () => {
  withTempDir((dir) => {
    const envFile = resolve(dir, ".env.release.local");
    writeFileSync(envFile, "export APM_MACOS_CERTIFICATE_BASE64\n", {
      encoding: "utf8",
      mode: 0o600,
    });

    const parsed = parseSecretEnvFile(envFile);
    assertIncludes(parsed.errors.join("\n"), "NAME=value", "parse error");
  });
});

test("uploads environment secrets through gh stdin", () => {
  const calls = [];
  assertDeepEqual(
    githubSecretInstallErrors({
      repo: "andreanjos/apm",
      env: completeSecretEnv(),
      apply: true,
      runCommand: fakeGh(calls),
    }),
    [],
    "secret upload errors",
  );

  const secretCalls = calls.filter((call) => call.args[0] === "secret");
  assertEqual(secretCalls.length, requiredReleaseEnvironmentSecrets().length, "secret call count");
  assertDeepEqual(
    secretCalls[0].args,
    [
      "secret",
      "set",
      "APM_MACOS_CERTIFICATE_BASE64",
      "--repo",
      "andreanjos/apm",
      "--env",
      "macos-desktop-release",
    ],
    "first gh secret args",
  );
  assertEqual(
    secretCalls[0].options.input,
    completeSecretEnv().APM_MACOS_CERTIFICATE_BASE64,
    "stdin",
  );
  assertEqual(
    secretCalls[0].args.includes(completeSecretEnv().APM_MACOS_CERTIFICATE_BASE64),
    false,
    "secret value must not appear in argv",
  );
  assertEqual(
    calls.some((call) => call.args.join(" ").includes("--method PUT")),
    true,
    "bootstrap before upload",
  );
  assertIncludes(calls.at(-1).args.join(" "), "/secrets", "inventory check after upload");
});

test("derives GitHub repository from origin remote before upload", () => {
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
    githubSecretInstallErrors({
      env: completeSecretEnv(),
      apply: true,
      runCommand,
    }),
    [],
    "origin-derived upload errors",
  );
  const secretCall = calls.find((call) => call.args[0] === "secret");
  assertEqual(secretCall.args[4], "andreanjos/apm", "origin-derived repo");
});

test("reports gh secret upload failures", () => {
  const errors = githubSecretInstallErrors({
    repo: "andreanjos/apm",
    env: completeSecretEnv(),
    apply: true,
    runCommand: fakeGh([], { failSecretSet: "secret rejected" }),
  }).join("\n");

  assertIncludes(errors, "secret rejected", "gh failure");
});

test("summarizes local secret input presence", () => {
  const entries = secretInstallEntries(completeSecretEnv({ APPLE_API_KEY_BASE64: "" }));
  const missing = entries.filter((entry) => !entry.present).map((entry) => entry.name);

  assertDeepEqual(missing, ["APPLE_API_KEY_BASE64"], "missing secret entries");
});

test("prints a safe local secret template", () => {
  const template = githubSecretTemplate();
  const defaultTag = defaultReleaseTag();

  for (const secret of requiredReleaseEnvironmentSecrets()) {
    assertIncludes(template, `export ${secret}=""`, `${secret} export`);
  }
  assertIncludes(template, "base64 -i /path/to/DeveloperIDApplication.p12", "p12 command");
  assertIncludes(template, "base64 -i /path/to/AuthKey_XXXXXXXXXX.p8", "api key command");
  assertIncludes(template, ".env.release.local", "ignored env file hint");
  assertIncludes(template, "--output ../../.env.release.local", "non-overwrite output command");
  assertIncludes(
    template,
    "github-secrets -- --repo andreanjos/apm --env-file ../../.env.release.local",
    "dry run command",
  );
  assertIncludes(
    template,
    "github-secrets -- --repo andreanjos/apm --env-file ../../.env.release.local --apply",
    "apply command",
  );
  assertIncludes(template, "github-check -- --repo andreanjos/apm", "remote inventory check");
  assertIncludes(
    template,
    `release:macos:status -- --repo andreanjos/apm --tag ${defaultTag} --markdown`,
    "status report command",
  );
  assertIncludes(
    template,
    `release:macos:tag -- --tag ${defaultTag} --expected-commit "$(git rev-parse HEAD)"`,
    "release tag command",
  );
  assertEqual(
    template.includes(completeSecretEnv().APM_MACOS_CERTIFICATE_BASE64),
    false,
    "no p12 value",
  );
  assertEqual(
    template.includes(completeSecretEnv().APPLE_API_KEY_BASE64),
    false,
    "no api key value",
  );
});

test("prints explicit release handoff context in the local secret template", () => {
  const template = githubSecretTemplate({ repo: "example/apm", tag: "v9.8.7" });

  assertIncludes(
    template,
    "github-secrets -- --repo example/apm --env-file ../../.env.release.local",
    "explicit repo",
  );
  assertIncludes(
    template,
    "release:macos:status -- --repo example/apm --tag v9.8.7 --markdown",
    "explicit status context",
  );
  assertIncludes(
    template,
    'release:macos:tag -- --tag v9.8.7 --expected-commit "$(git rev-parse HEAD)"',
    "explicit release tag command",
  );
});

test("writes the local secret template without overwriting existing files", () => {
  withTempDir((dir) => {
    const output = resolve(dir, ".env.release.local");
    const result = writeGithubSecretTemplate({
      output,
      repo: "example/apm",
      tag: "v9.8.7",
    });

    assertDeepEqual(result.errors, [], "write errors");
    assertEqual(result.written, true, "write result");
    assertEqual(result.path, output, "written path");
    const template = readFileSync(output, "utf8");
    assertIncludes(
      template,
      "github-secrets -- --repo example/apm --env-file ../../.env.release.local",
      "written repo",
    );
    assertIncludes(
      template,
      "release:macos:status -- --repo example/apm --tag v9.8.7 --markdown",
      "written status context",
    );
    assertIncludes(
      template,
      'release:macos:tag -- --tag v9.8.7 --expected-commit "$(git rev-parse HEAD)"',
      "written release tag command",
    );
    assertEqual(statSync(output).mode & 0o077, 0, "secret template file mode");

    const overwrite = writeGithubSecretTemplate({ output });
    assertEqual(overwrite.written, false, "overwrite result");
    assertIncludes(overwrite.errors.join("\n"), "refusing to overwrite", "overwrite error");
    assertIncludes(readFileSync(output, "utf8"), "example/apm", "existing file unchanged");
  });
});

test("reports unwritable local secret template paths", () => {
  withTempDir((dir) => {
    const result = writeGithubSecretTemplate({ output: resolve(dir, "missing", "template") });

    assertEqual(result.written, false, "write result");
    assertIncludes(result.errors.join("\n"), "could not write secret template", "write error");
  });
});

test("validates individual secret shapes", () => {
  assertDeepEqual(
    secretValueErrors("APPLE_API_KEY", "ABC123DEFG"),
    [],
    "valid api key id",
  );
  assertIncludes(
    secretValueErrors("APPLE_API_ISSUER", "not-a-uuid").join("\n"),
    "issuer UUID",
    "issuer uuid error",
  );
  assertIncludes(
    secretValueErrors("APM_MACOS_CERTIFICATE_BASE64", base64("not der")).join("\n"),
    ".p12",
    "certificate der error",
  );
});

test("prints GitHub secret command help without validation, write, or upload", () => {
  const output = [];
  const errors = [];
  const status = runGithubSecretCommand([
    "--help",
    "--apply=false",
    "--print-template",
    "--output",
    "/definitely/missing/.env.release.local",
  ], {
    log: (line) => output.push(line),
    error: (line) => errors.push(line),
  });

  const help = output.join("\n");
  assertEqual(status, 0, "help status");
  assertIncludes(help, "Usage: npm run release:macos:github-secrets", "secrets usage");
  assertIncludes(
    help,
    "npm run release:macos:github-secrets-template",
    "template usage",
  );
  assertDeepEqual(errors, [], "help errors");
});

test("rejects unknown GitHub secret arguments before setup", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-github-secrets.mjs"),
    "--repo",
    "andreanjos/apm",
    "--apply=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(result.stderr, "--apply does not accept a value", "flag value error");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

runTests();

function completeSecretEnv(overrides = {}) {
  return {
    APM_MACOS_CERTIFICATE_BASE64: derLikeBase64(),
    APM_MACOS_CERTIFICATE_PASSWORD: "certificate-password",
    APM_MACOS_KEYCHAIN_PASSWORD: "keychain-password",
    APM_MACOS_SIGNING_IDENTITY: "Developer ID Application: Example, Inc. (ABCDE12345)",
    APM_MACOS_PROVIDER_SHORT_NAME: "ABCDE12345",
    APPLE_API_KEY: "ABC123DEFG",
    APPLE_API_ISSUER: "12345678-1234-1234-1234-123456789abc",
    APPLE_API_KEY_BASE64: base64([
      "-----BEGIN PRIVATE KEY-----",
      "example",
      "-----END PRIVATE KEY-----",
      "",
    ].join("\n")),
    ...overrides,
  };
}

function writeCompleteSecretEnvFile(path, overrides = {}) {
  const env = completeSecretEnv(overrides);
  const lines = [
    "# test release secrets",
    ...Object.entries(env).map(([name, value]) => `export ${name}="${value}"`),
    "",
  ];
  writeFileSync(path, lines.join("\n"), {
    encoding: "utf8",
    mode: 0o600,
  });
  return path;
}

function derLikeBase64() {
  return Buffer.from([0x30, 0x03, 0x02, 0x01, 0x00]).toString("base64");
}

function base64(value) {
  return Buffer.from(value, "utf8").toString("base64");
}

function fakeGh(calls, behavior = {}) {
  return (command, args, options = {}) => {
    calls.push({ command, args, options });
    if (command !== "gh") {
      return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
    }
    if (args[0] === "api") {
      return ghApiResponse(args);
    }
    if (args[0] === "secret" && behavior.failSecretSet) {
      return { status: 1, stdout: "", stderr: behavior.failSecretSet };
    }
    return {
      status: 0,
      stdout: "",
      stderr: "",
    };
  };
}

function withTempDir(run) {
  const dir = mkdtempSync(resolve(tmpdir(), "apm-secrets-test-"));
  try {
    run(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function ghApiResponse(args) {
  const key = args.join(" ");
  const bootstrapEnvironmentKey = [
    "api",
    "--method",
    "PUT",
    "repos/andreanjos/apm/environments/macos-desktop-release",
    "--input",
    "-",
  ].join(" ");
  if (key === bootstrapEnvironmentKey) {
    return {
      status: 0,
      stdout: `${JSON.stringify({ name: "macos-desktop-release" })}\n`,
      stderr: "",
    };
  }
  if (key === "api repos/andreanjos/apm/environments/macos-desktop-release") {
    return {
      status: 0,
      stdout: `${JSON.stringify({ name: "macos-desktop-release" })}\n`,
      stderr: "",
    };
  }
  if (key === "api repos/andreanjos/apm/environments/macos-desktop-release/secrets") {
    return {
      status: 0,
      stdout: `${JSON.stringify({
        secrets: requiredReleaseEnvironmentSecrets().map((name) => ({ name })),
      })}\n`,
      stderr: "",
    };
  }
  return { status: 1, stdout: "", stderr: `unexpected gh api request: ${key}` };
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
