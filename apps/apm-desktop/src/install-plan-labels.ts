import type {
  InstallPlanResult,
  InstallPlanStatus,
} from "./types";

export function installPlanStatusLabel(result: InstallPlanResult) {
  switch (result.status) {
    case "plan":
      return installPlanTitle(result.plan.status);
    case "catalog_empty":
      return "Catalog is empty";
    case "not_found":
      return "Package not found";
    case "not_installable":
      return "Not directly installable";
    case "version_not_found":
      return "Version not found";
    case "format_unavailable":
      return "Format unavailable";
  }
}

export function installPlanTitle(status: InstallPlanStatus) {
  switch (status) {
    case "ready":
      return "Ready for install";
    case "already_installed":
      return "Already installed";
    case "manual_required":
      return "Manual handoff";
    case "privileged_installer_required":
      return "PKG handoff";
    case "app_store_required":
      return "App Store handoff";
    case "vendor_installer_available":
      return "Vendor app ready";
    case "vendor_installer_required":
      return "Vendor app required";
  }
}

export function formatInstallStatus(status: InstallPlanStatus) {
  return status.replaceAll("_", " ");
}
