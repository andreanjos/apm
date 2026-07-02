import { existsSync, readdirSync, statSync } from "node:fs";
import { basename, resolve } from "node:path";
import {
  argumentErrors,
  booleanArg,
  errorMessage,
  isMain,
  repoRoot,
  run,
} from "./macos-release-github-common.mjs";
import {
  dmgArtifactErrors,
  previewSignatureErrors,
  selectDmgArtifacts,
} from "./macos-verify.mjs";

const bundleRoot = resolve(repoRoot, "target/release/bundle");
const defaultAppPath = resolve(bundleRoot, "macos/apm.app");
const defaultDmgDir = resolve(bundleRoot, "dmg");

if (isMain(import.meta.url)) {
  const status = runPreviewOpenCommand(process.argv.slice(2));
  if (status !== 0) {
    process.exit(status);
  }
}

export function runPreviewOpenCommand(argv, runtime = {}) {
  const parsed = optionsFromArgs(argv);
  const log = runtime.log ?? console.log;
  const writeError = runtime.error ?? console.error;

  if (parsed.help) {
    log(usage());
    return 0;
  }

  if (parsed.errors.length > 0) {
    writeFailure(writeError, parsed.errors);
    return 1;
  }

  try {
    const result = openMacosPreview({
      ...runtime.openOptions,
      dmg: parsed.dmg,
      dryRun: parsed.dryRun,
    });
    if (result.errors.length > 0) {
      writeFailure(writeError, result.errors);
      return 1;
    }
    log(
      `${parsed.dryRun ? "Verified" : "Opened"} ${result.target.type} preview: ${result.target.path}`,
    );
    return 0;
  } catch (caught) {
    writeError(`Failed to open macOS preview: ${errorMessage(caught)}`);
    return 1;
  }
}

export function openMacosPreview(options = {}) {
  const target = macosPreviewLaunchTarget(options);
  if (target.errors.length > 0) {
    return { target: null, errors: target.errors };
  }

  const platform = options.platform ?? process.platform;
  if (platform !== "darwin") {
    return {
      target: target.value,
      errors: ["opening macOS preview artifacts requires macOS"],
    };
  }

  const verifyTarget = options.verifyTarget ?? previewOpenVerificationErrors;
  const verificationErrors = verifyTarget(target.value);
  if (verificationErrors.length > 0) {
    return {
      target: target.value,
      errors: [
        ...verificationErrors,
        "run npm run bundle:macos:verified first",
      ],
    };
  }

  if (options.dryRun) {
    return { target: target.value, errors: [] };
  }

  const opened = run(options.runCommand, "open", [target.value.path]);
  if (opened.status !== 0) {
    return {
      target: target.value,
      errors: [
        `open failed for ${basename(target.value.path)}: ` +
          `${opened.stderr || opened.stdout || "open exited non-zero"}`,
      ],
    };
  }

  return { target: target.value, errors: [] };
}

export function macosPreviewLaunchTarget(options = {}) {
  return options.dmg
    ? macosPreviewDmgTarget(options.dmgDir ?? defaultDmgDir)
    : macosPreviewAppTarget(options.appPath ?? defaultAppPath);
}

export function previewOpenVerificationErrors(target) {
  if (target.type === "dmg") {
    return dmgArtifactErrors([target.path]);
  }
  return previewSignatureErrors(target.path);
}

export function optionsFromArgs(argv) {
  const normalizedArgv = argv.map((arg) => (arg === "-h" ? "--help" : arg));
  return {
    dmg: booleanArg(normalizedArgv, "--dmg", false),
    dryRun: booleanArg(normalizedArgv, "--dry-run", false),
    help: normalizedArgv.includes("--help"),
    errors: argumentErrors(normalizedArgv, {
      booleanArgs: ["--dmg", "--dry-run"],
      flagArgs: ["--help"],
    }),
  };
}

function macosPreviewAppTarget(appPath) {
  if (!directoryExists(appPath)) {
    return {
      value: null,
      errors: [
        `missing preview app bundle: ${appPath}`,
        "run npm run bundle:macos:verified first",
      ],
    };
  }
  return {
    value: {
      type: "app",
      path: appPath,
    },
    errors: [],
  };
}

function macosPreviewDmgTarget(dmgDir) {
  if (!directoryExists(dmgDir)) {
    return {
      value: null,
      errors: [
        `missing preview DMG directory: ${dmgDir}`,
        "run npm run bundle:macos:verified first",
      ],
    };
  }

  const dmgs = selectDmgArtifacts(readdirSync(dmgDir), dmgDir).sort(
    comparePreviewArtifactRecency,
  );
  if (dmgs.length === 0) {
    return {
      value: null,
      errors: [
        `missing apm preview DMG under ${dmgDir}`,
        "run npm run bundle:macos:verified first",
      ],
    };
  }

  return {
    value: {
      type: "dmg",
      path: dmgs[dmgs.length - 1],
    },
    errors: [],
  };
}

function directoryExists(path) {
  return existsSync(path) && statSync(path).isDirectory();
}

function comparePreviewArtifactRecency(left, right) {
  const leftMtime = statSync(left).mtimeMs;
  const rightMtime = statSync(right).mtimeMs;
  return leftMtime === rightMtime ? left.localeCompare(right) : leftMtime - rightMtime;
}

function usage() {
  return [
    "Usage: npm run open:macos:preview -- [--dmg] [--dry-run]",
    "",
    "Options:",
    "  --dmg[=true|false]  Open the newest local preview DMG instead of apm.app",
    "  --dry-run[=true|false]  Verify the selected preview artifact without opening it",
    "  -h, --help          Show this help without opening anything",
  ].join("\n");
}

function writeFailure(writeError, errors) {
  writeError("macOS preview open failed:");
  for (const error of errors) {
    writeError(`- ${error}`);
  }
}
