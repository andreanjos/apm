import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdtempSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";

const buildToolsDir = dirname(fileURLToPath(import.meta.url));
const desktopRoot = resolve(buildToolsDir, "..");
const testOutputRoot = resolve(desktopRoot, ".tmp");
mkdirSync(testOutputRoot, { recursive: true });
const outDir = mkdtempSync(resolve(testOutputRoot, "unit-tests-"));
const tsc = resolve(
  desktopRoot,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tsc.cmd" : "tsc",
);

try {
  run(tsc, ["-p", "tsconfig.test.json", "--outDir", outDir]);
  writeFileSync(
    resolve(outDir, "package.json"),
    `${JSON.stringify({ type: "commonjs" }, null, 2)}\n`,
  );

  const testFiles = findTestFiles(outDir);
  if (testFiles.length === 0) {
    throw new Error("no compiled unit tests found");
  }
  for (const testFile of testFiles) {
    run("node", [testFile]);
  }
  for (const testFile of findBuildToolTestFiles(buildToolsDir)) {
    run("node", [testFile]);
  }
} finally {
  rmSync(outDir, { recursive: true, force: true });
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: desktopRoot,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}

function findTestFiles(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) {
        return findTestFiles(path);
      }
      return entry.isFile() && entry.name.endsWith(".test.js") ? [path] : [];
    })
    .sort();
}

function findBuildToolTestFiles(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".test.mjs"))
    .map((entry) => resolve(directory, entry.name))
    .sort();
}
