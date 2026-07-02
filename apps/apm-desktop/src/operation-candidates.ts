import type {
  InstallPlanResult,
  InstalledPackageSummary,
  PackageUpdateSummary,
} from "./types";
import { fileNameFromPath, formatLabel } from "./view-utils";
import type {
  InstallHandoffCandidate,
  ArchiveInstallCandidate,
  UpdatePackageCandidate,
  UrlInstallCandidate,
} from "./view-model";

export function archiveInstallCandidate(
  installPlan: InstallPlanResult | null,
  slug: string,
  format: string,
  archivePath: string,
): ArchiveInstallCandidate {
  if (!installPlan || installPlan.status !== "plan" || installPlan.plan.slug !== slug) {
    throw new Error("Review the install plan before choosing an archive.");
  }

  const selectedFormat = installPlan.plan.formats.find(
    (candidate) => candidate.format.toLowerCase() === format.toLowerCase(),
  );
  if (!selectedFormat) {
    throw new Error(`${installPlan.plan.name} is not available as ${formatLabel(format)}.`);
  }

  return {
    slug,
    name: installPlan.plan.name,
    format,
    installType: selectedFormat.install_type,
    version: installPlan.plan.version,
    destination: installPlan.plan.destination ?? "Format-specific destination",
    installScope: installPlan.plan.scope,
    archivePath,
    archiveName: fileNameFromPath(archivePath),
    checksum: selectedFormat.has_checksum
      ? "Registry checksum will be verified"
      : "No registry checksum listed",
  };
}

export function urlInstallCandidate(
  installPlan: InstallPlanResult | null,
  slug: string,
  format: string,
): UrlInstallCandidate {
  if (!installPlan || installPlan.status !== "plan" || installPlan.plan.slug !== slug) {
    throw new Error("Review the install plan before downloading.");
  }

  const selectedFormat = installPlan.plan.formats.find(
    (candidate) => candidate.format.toLowerCase() === format.toLowerCase(),
  );
  if (!selectedFormat || selectedFormat.source.trim().length === 0) {
    throw new Error(`${installPlan.plan.name} does not list a direct ${formatLabel(format)} URL.`);
  }

  return {
    slug,
    name: installPlan.plan.name,
    format,
    version: installPlan.plan.version,
    destination: installPlan.plan.destination ?? "Format-specific destination",
    installScope: installPlan.plan.scope,
    source: selectedFormat.source,
    checksum: selectedFormat.has_checksum
      ? "Registry checksum will be verified"
      : "No registry checksum listed",
  };
}

export function installHandoffCandidate(
  installPlan: InstallPlanResult | null,
  slug: string,
): InstallHandoffCandidate {
  if (!installPlan || installPlan.status !== "plan" || installPlan.plan.slug !== slug) {
    throw new Error("Review the install plan before opening a handoff.");
  }

  const { plan } = installPlan;
  const source = handoffTarget(plan);
  if (!source) {
    throw new Error(`${plan.name} does not list a handoff target.`);
  }

  return {
    slug,
    name: plan.name,
    vendor: plan.vendor,
    version: plan.version,
    statusLabel: handoffStatusLabel(plan.status),
    target: source,
    message: plan.message,
    actionLabel: handoffActionLabel(plan.status),
    privileged: plan.status === "privileged_installer_required",
  };
}

export function updatePackageCandidate(
  installed: InstalledPackageSummary[],
  update: PackageUpdateSummary,
): UpdatePackageCandidate {
  const formats = [
    ...new Set(
      installed.find((item) => item.slug === update.slug)?.formats.map((format) => format.format) ??
        [],
    ),
  ];

  return {
    ...update,
    formats,
    updateFormat: formats.length <= 1 ? formats[0] ?? null : null,
  };
}

export function canRunUpdateCandidate(update: UpdatePackageCandidate) {
  return update.formats.length > 0;
}

function handoffTarget(
  plan: Extract<InstallPlanResult, { status: "plan" }>["plan"],
) {
  switch (plan.status) {
    case "vendor_installer_available":
      return plan.installer?.installed_app_path ?? plan.installer?.download_url ?? null;
    case "vendor_installer_required":
      return plan.installer?.download_url ?? null;
    case "manual_required":
    case "privileged_installer_required":
    case "app_store_required":
      return plan.formats.find((format) => format.source.trim().length > 0)?.source ?? null;
    case "ready":
    case "already_installed":
      return null;
  }
}

function handoffStatusLabel(
  status: Extract<InstallPlanResult, { status: "plan" }>["plan"]["status"],
) {
  switch (status) {
    case "manual_required":
      return "Manual download";
    case "privileged_installer_required":
      return "Privileged PKG handoff";
    case "app_store_required":
      return "Mac App Store handoff";
    case "vendor_installer_available":
      return "Vendor manager app";
    case "vendor_installer_required":
      return "Vendor manager download";
    case "ready":
      return "Direct install";
    case "already_installed":
      return "Already installed";
  }
}

function handoffActionLabel(
  status: Extract<InstallPlanResult, { status: "plan" }>["plan"]["status"],
) {
  switch (status) {
    case "manual_required":
      return "Open download";
    case "privileged_installer_required":
      return "Open PKG";
    case "app_store_required":
      return "Open App Store";
    case "vendor_installer_available":
      return "Open vendor app";
    case "vendor_installer_required":
      return "Get vendor app";
    case "ready":
    case "already_installed":
      return "Open handoff";
  }
}
