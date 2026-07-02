import type { DesktopDistribution } from "./types";

export type ReleaseReadinessTone = "info" | "warn";

export type ReleaseReadinessItem = {
  key: string;
  label: string;
  value: string;
  detail: string;
  tone: ReleaseReadinessTone;
};

export type ReleaseReadiness = {
  summary: ReleaseReadinessItem;
  checks: ReleaseReadinessItem[];
};

export function desktopReleaseReadiness(
  distribution: DesktopDistribution,
): ReleaseReadiness | null {
  switch (distribution.channel) {
    case "browser_preview":
    case "development":
      return null;
    case "preview_bundle":
      return {
        summary: {
          key: "summary",
          label: "Release readiness",
          value: "Public gate required",
          detail: "Preview bundles are internal until the signed release workflow passes.",
          tone: "warn",
        },
        checks: publicReleaseChecks("warn"),
      };
    case "public_release":
      return {
        summary: {
          key: "summary",
          label: "Release readiness",
          value: "Verifier proof required",
          detail: "The public channel selects the release gate; the verifier remains the proof.",
          tone: "info",
        },
        checks: publicReleaseChecks("info"),
      };
  }
}

function publicReleaseChecks(tone: ReleaseReadinessTone): ReleaseReadinessItem[] {
  return [
    {
      key: "status",
      label: "Release status",
      value: "Inspect blockers",
      detail:
        "npm run release:macos:status -- --markdown prints blockers and handoff notes before dispatch.",
      tone,
    },
    {
      key: "preflight",
      label: "Release preflight",
      value: "Run check",
      detail: "npm run release:macos:check validates Tauri config and workflow rails.",
      tone,
    },
    {
      key: "bundle",
      label: "Signed bundle",
      value: "Gate required",
      detail: "npm run bundle:macos:release requires Developer ID and notarization inputs.",
      tone,
    },
    {
      key: "verifier",
      label: "Artifact verifier",
      value: "Proof required",
      detail: "npm run verify:macos:release checks Gatekeeper, stapling, DMG integrity, and the sidecar contract.",
      tone,
    },
    {
      key: "workflow",
      label: "Release workflow",
      value: "Manual acceptance",
      detail: "desktop-release.yml must pass inside the macos-desktop-release environment before publishing.",
      tone,
    },
  ];
}
