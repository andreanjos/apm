import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const buildToolsDir = dirname(fileURLToPath(import.meta.url));
const desktopRoot = resolve(buildToolsDir, "..");
const repoRoot = resolve(desktopRoot, "../..");
const sidecarDir = resolve(desktopRoot, "src-tauri/sidecars");

run("cargo", ["build", "-p", "apm-cli", "--release"], repoRoot);

const hostTriple = rustHostTriple();
const binaryName = process.platform === "win32" ? "apm.exe" : "apm";
const source = resolve(repoRoot, "target/release", binaryName);
const destination = resolve(
  sidecarDir,
  `apm-cli-${hostTriple}${process.platform === "win32" ? ".exe" : ""}`,
);

if (!existsSync(source)) {
  throw new Error(`expected release CLI at ${source}`);
}
mkdirSync(sidecarDir, { recursive: true });
copyFileSync(source, destination);
chmodSync(destination, 0o755);

console.log(`staged apm sidecar: ${destination}`);

function rustHostTriple() {
  const result = spawnSync("rustc", ["-vV"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || "failed to inspect rust host triple");
  }

  const match = result.stdout.match(/^host: (.+)$/m);
  if (!match) {
    throw new Error("rustc -vV did not report a host triple");
  }
  return match[1];
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}
