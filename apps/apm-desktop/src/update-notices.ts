import type {
  InstallPackageResult,
  UpdatePackageResult,
} from "./types";
import { installPlanStatusLabel } from "./install-plan-labels";
import type { LifecycleNotice } from "./view-model";

export function updateResultNotice(
  result: Exclude<UpdatePackageResult, { status: "updated" }>,
): LifecycleNotice {
  switch (result.status) {
    case "catalog_empty":
      return {
        tone: "error",
        message: "The catalog cache is empty. Sync registries first.",
      };
    case "not_installed":
      return { tone: "error", message: `${result.slug} is not installed.` };
    case "not_found":
      return {
        tone: "error",
        message: `${result.slug} is no longer in the catalog.`,
      };
    case "up_to_date":
      return {
        tone: "info",
        message: `${result.slug} is already current at ${result.version}.`,
      };
    case "pinned":
      return {
        tone: "info",
        message: `${result.update.slug} is pinned. Unpin it before updating.`,
      };
    case "external":
      return {
        tone: "info",
        message: `${result.update.slug} is managed outside apm.`,
      };
    case "install_unavailable":
      return {
        tone: "error",
        message: installUnavailableMessage(result.result),
      };
  }
}

function installUnavailableMessage(result: InstallPackageResult) {
  switch (result.status) {
    case "plan_unavailable":
      return installPlanStatusLabel(result.plan);
    case "already_installed":
      return `${result.plan.name} is already installed.`;
    case "external_handoff_required":
    case "format_required":
    case "archive_required":
    case "unsupported_install_type":
      return result.reason;
    case "installed":
      return `${result.package.slug} installed.`;
  }
}
