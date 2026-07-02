import {
  environmentBootstrapRequest,
  githubEnvironmentBootstrapErrors,
  githubEnvironmentCheckErrors,
  repoFromRemoteUrl,
  runGithubEnvironmentCommand,
  secretInventoryErrors,
} from "./macos-release-github-env.mjs";
import { requiredReleaseEnvironmentSecrets } from "./macos-release.mjs";
import { repoRoot } from "./macos-release-github-common.mjs";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const tests = [];

test("parses GitHub remote URLs", () => {
  assertEqual(
    repoFromRemoteUrl("git@github.com:andreanjos/apm.git"),
    "andreanjos/apm",
    "ssh remote",
  );
  assertEqual(
    repoFromRemoteUrl("https://github.com/andreanjos/apm.git"),
    "andreanjos/apm",
    "https remote",
  );
  assertEqual(repoFromRemoteUrl("file:///tmp/apm"), null, "non-GitHub remote");
});

test("accepts configured GitHub Environment secret inventory", () => {
  assertDeepEqual(
    secretInventoryErrors(
      {
        secrets: requiredReleaseEnvironmentSecrets().map((name) => ({ name })),
      },
      "macos-desktop-release",
    ),
    [],
    "secret inventory errors",
  );
});

test("reports missing GitHub Environment secrets", () => {
  const errors = secretInventoryErrors(
    {
      secrets: requiredReleaseEnvironmentSecrets()
        .filter((name) => name !== "APPLE_API_KEY_BASE64")
        .map((name) => ({ name })),
    },
    "macos-desktop-release",
  ).join("\n");

  assertIncludes(errors, "APPLE_API_KEY_BASE64", "missing secret error");
});

test("checks GitHub Environment and secrets through gh api", () => {
  assertDeepEqual(
    githubEnvironmentCheckErrors({
      repo: "andreanjos/apm",
      runCommand: fakeGh(completeEnvironmentResponses()),
    }),
    [],
    "github environment errors",
  );
});

test("derives GitHub repository from origin remote", () => {
  const gh = fakeGh(completeEnvironmentResponses());
  const runCommand = (command, args) => {
    if (command === "git" && args.join(" ") === "remote get-url origin") {
      return {
        status: 0,
        stdout: "git@github.com:andreanjos/apm.git\n",
        stderr: "",
      };
    }
    return gh(command, args);
  };

  assertDeepEqual(
    githubEnvironmentCheckErrors({ runCommand }),
    [],
    "github environment errors from origin remote",
  );
});

test("bootstraps GitHub Environment shell", () => {
  const calls = [];
  const runCommand = fakeGh(
    {
      "api --method PUT repos/andreanjos/apm/environments/macos-desktop-release --input -": {
        name: "macos-desktop-release",
      },
    },
    calls,
  );

  assertDeepEqual(
    githubEnvironmentBootstrapErrors({
      repo: "andreanjos/apm",
      runCommand,
    }),
    [],
    "bootstrap errors",
  );
  assertDeepEqual(
    JSON.parse(calls[0].options.input),
    environmentBootstrapRequest(),
    "bootstrap request body",
  );
  assertEqual(calls.length, 1, "bootstrap call count");
});

test("reports gh api failures", () => {
  const errors = githubEnvironmentCheckErrors({
    repo: "andreanjos/apm",
    runCommand: () => ({ status: 1, stdout: "", stderr: "not authenticated" }),
  }).join("\n");

  assertIncludes(errors, "not authenticated", "gh failure");
});

test("reports missing GitHub Environment with required secret names", () => {
  const errors = githubEnvironmentCheckErrors({
    repo: "andreanjos/apm",
    runCommand: () => ({ status: 1, stdout: "", stderr: "gh: Not Found (HTTP 404)" }),
  }).join("\n");

  assertIncludes(errors, "macos-desktop-release was not found", "missing environment error");
  assertIncludes(errors, "APPLE_API_KEY_BASE64", "required secret names");
});

test("prints GitHub Environment command help without gh calls", () => {
  let callCount = 0;
  const output = [];
  const errors = [];
  const status = runGithubEnvironmentCommand([
    "--help",
    "--create=false",
    "--repo",
    "andreanjos/apm",
  ], {
    log: (line) => output.push(line),
    error: (line) => errors.push(line),
    runCommand: () => {
      callCount += 1;
      return { status: 1, stdout: "", stderr: "should not run" };
    },
  });

  const help = output.join("\n");
  assertEqual(status, 0, "help status");
  assertIncludes(help, "Usage: npm run release:macos:github-check", "check usage");
  assertIncludes(help, "npm run release:macos:github-bootstrap", "bootstrap usage");
  assertEqual(callCount, 0, "help gh calls");
  assertDeepEqual(errors, [], "help errors");
});

test("rejects unknown GitHub Environment arguments before gh calls", () => {
  const result = spawnSync(process.execPath, [
    resolve(repoRoot, "apps/apm-desktop/build-tools/macos-release-github-env.mjs"),
    "--repo",
    "andreanjos/apm",
    "--create=false",
    "--typo",
  ], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  assertEqual(result.status, 1, "exit status");
  assertIncludes(result.stderr, "--create does not accept a value", "flag value error");
  assertIncludes(result.stderr, "unknown argument: --typo", "unknown argument");
});

runTests();

function completeEnvironmentResponses() {
  return {
    "api repos/andreanjos/apm/environments/macos-desktop-release": {
      name: "macos-desktop-release",
    },
    "api repos/andreanjos/apm/environments/macos-desktop-release/secrets": {
      secrets: requiredReleaseEnvironmentSecrets().map((name) => ({ name })),
    },
  };
}

function fakeGh(responses, calls = []) {
  return (command, args, options = {}) => {
    calls.push({ command, args, options });
    if (command !== "gh") {
      return { status: 1, stdout: "", stderr: `unexpected command: ${command}` };
    }
    const key = args.join(" ");
    if (!Object.hasOwn(responses, key)) {
      return { status: 1, stdout: "", stderr: `unexpected gh request: ${key}` };
    }
    return {
      status: 0,
      stdout: `${JSON.stringify(responses[key])}\n`,
      stderr: "",
    };
  };
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
