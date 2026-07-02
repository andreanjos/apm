import type {
  InstallEvent,
  InstallPlanFormat,
  InstallPlanResult,
  InstallScope,
  PackageDetails,
  PackageDetailsResult,
  PackageFormatSummary,
  PackageInstallPlan,
  PackageSummary,
} from "./types";
import type {
  DesktopViewState,
  LifecycleNotice,
  OperationControlState,
} from "./view-model";
import {
  formatInstallStatus,
  installPlanTitle,
} from "./install-plan-labels";
import {
  lifecycleNoticeMarkup,
  lifecycleTimelineMarkup,
  operationCancelMarkup,
} from "./lifecycle-markup";
import { escapeHtml, formatLabel } from "./view-utils";

const LIFECYCLE_ACTION_LOCKED_LABEL = "Install operation running";

type PackageInspectorState = Pick<
  DesktopViewState,
  | "installPlan"
  | "installScope"
  | "installStatus"
  | "packageDetails"
  | "packageDetailsLoading"
  | "packageDetailsError"
  | "lifecycleNotice"
  | "lifecycleOperation"
  | "lifecycleEvents"
>;

export function packageInspector(
  item: PackageSummary | undefined,
  state: PackageInspectorState,
) {
  if (!item) {
    return `<p class="muted">Sync the registry or choose a catalog item to inspect package details.</p>`;
  }

  const lifecycleActionLocked = state.lifecycleOperation !== null;
  const handoff = handoffButtonState(
    state.installPlan,
    item,
    lifecycleActionLocked,
  );
  const details = packageDetailsFor(item, state.packageDetails);
  const reviewLabel = lifecycleActionLocked
    ? `${LIFECYCLE_ACTION_LOCKED_LABEL} for ${item.slug}`
    : "Review install";
  const reviewTitle = lifecycleActionLocked
    ? LIFECYCLE_ACTION_LOCKED_LABEL
    : item.is_installable
      ? "Review install plan"
      : "Catalog item is not directly installable";

  return `
    ${packageDescription(item)}
    <dl class="detail-list">
      <div><dt>Slug</dt><dd>${escapeHtml(item.slug)}</dd></div>
      <div><dt>Vendor</dt><dd>${escapeHtml(item.vendor)}</dd></div>
      <div><dt>Version</dt><dd>${escapeHtml(item.version)}</dd></div>
      <div><dt>Category</dt><dd>${escapeHtml(formatCategory(item))}</dd></div>
      <div><dt>Type</dt><dd>${escapeHtml(item.product_type)}</dd></div>
      <div><dt>Access</dt><dd>${item.is_paid ? "Paid" : "Free"}</dd></div>
      <div><dt>License</dt><dd>${escapeHtml(item.license)}</dd></div>
      <div><dt>Homepage</dt><dd>${detailUrlMarkup(details?.homepage, state)}</dd></div>
      ${item.is_paid || details?.purchase_url ? `<div><dt>Purchase</dt><dd>${detailUrlMarkup(details?.purchase_url, state)}</dd></div>` : ""}
      ${detailTokenRow("Aliases", details?.aliases)}
      ${knownVersionsRow(item.version, details?.available_versions)}
      ${detailTokenRow("Bundle IDs", details?.bundle_ids)}
      <div><dt>Installed</dt><dd>${item.installed ? escapeHtml(item.installed_version ?? item.version) : "No"}</dd></div>
    </dl>
    ${formatSummary(item)}
    <div class="action-row">
      <button class="primary-action" id="review-install" type="button" aria-label="${escapeHtml(reviewLabel)}" title="${escapeHtml(reviewTitle)}" ${item.is_installable && !lifecycleActionLocked ? "" : "disabled"}>
        <i data-lucide="shield-check" aria-hidden="true"></i>
        Review install
      </button>
      <button class="secondary-action" id="open-handoff" type="button" ${handoff.enabled ? "" : "disabled"} title="${escapeHtml(handoff.title)}">
        <i data-lucide="external-link" aria-hidden="true"></i>
        ${escapeHtml(handoff.label)}
      </button>
    </div>
    ${installPlanView(
      state.installPlan,
      state.installScope,
      item,
      state.installStatus,
      state.lifecycleNotice,
      state.lifecycleOperation,
      state.lifecycleEvents,
      lifecycleActionLocked,
    )}
  `;
}

function packageDetailsFor(
  item: PackageSummary,
  result: PackageDetailsResult | null,
): PackageDetails | null {
  if (result?.status !== "found" || result.package.summary.slug !== item.slug) {
    return null;
  }
  return result.package;
}

function detailUrlMarkup(
  url: string | null | undefined,
  state: PackageInspectorState,
) {
  if (state.packageDetailsLoading) {
    return "Loading";
  }
  if (state.packageDetailsError) {
    return escapeHtml(`Unavailable: ${state.packageDetailsError}`);
  }
  if (!url) {
    return "Not listed";
  }
  if (!isSafeExternalUrl(url)) {
    return escapeHtml(url);
  }
  return `<a class="detail-link" href="${escapeHtml(url)}" target="_blank" rel="noreferrer">${escapeHtml(url)}</a>`;
}

function isSafeExternalUrl(url: string) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

function knownVersionsRow(catalogVersion: string, versions: string[] | undefined) {
  return detailTokenRow(
    "Other versions",
    cleanDetailValues(versions).filter((version) => version !== catalogVersion.trim()),
  );
}

function detailTokenRow(label: string, values: string[] | undefined) {
  const cleanValues = cleanDetailValues(values);
  if (cleanValues.length === 0) {
    return "";
  }

  return `
    <div>
      <dt>${escapeHtml(label)}</dt>
      <dd class="detail-token-list">
        ${cleanValues.map((value) => `<span>${escapeHtml(value)}</span>`).join("")}
      </dd>
    </div>
  `;
}

function cleanDetailValues(values: string[] | undefined) {
  return Array.from(
    new Set(values?.map((value) => value.trim()).filter((value) => value.length > 0) ?? []),
  );
}

function packageDescription(item: PackageSummary) {
  const description = item.description.trim();
  if (description.length === 0) {
    return "";
  }
  return `<p class="package-description">${escapeHtml(description)}</p>`;
}

function formatSummary(item: PackageSummary) {
  if (item.formats.length === 0) {
    return "";
  }
  return `
    <section class="package-detail-section">
      <div class="plan-kicker">Formats</div>
      <div class="format-summary-list">
        ${item.formats.map(formatSummaryRow).join("")}
      </div>
    </section>
  `;
}

function formatSummaryRow(format: PackageFormatSummary) {
  const checksum = format.has_checksum ? "checksum verified" : "no checksum";
  const path = format.bundle_path
    ? `<small class="format-summary-path">${escapeHtml(format.bundle_path)}</small>`
    : "";
  return `
    <div class="format-summary-row">
      <strong>${escapeHtml(formatLabel(format.format))}</strong>
      <span>${escapeHtml(format.download_type)} / ${escapeHtml(format.install_type)} / ${checksum}</span>
      ${path}
    </div>
  `;
}

function handoffButtonState(
  result: InstallPlanResult | null,
  item: PackageSummary,
  lifecycleActionLocked: boolean,
) {
  if (lifecycleActionLocked) {
    return {
      enabled: false,
      label: "Open handoff",
      title: LIFECYCLE_ACTION_LOCKED_LABEL,
    };
  }

  if (!result || result.status !== "plan" || result.plan.slug !== item.slug) {
    return {
      enabled: false,
      label: "Open handoff",
      title: "Review install before opening a handoff",
    };
  }

  switch (result.plan.status) {
    case "manual_required":
      return {
        enabled: true,
        label: "Open download",
        title: "Open the package download page",
      };
    case "privileged_installer_required":
      return {
        enabled: true,
        label: "Open PKG",
        title: "Open the PKG download page; apm will not run privileged installers yet",
      };
    case "app_store_required":
      return {
        enabled: true,
        label: "Open App Store",
        title: "Open the Mac App Store listing",
      };
    case "vendor_installer_available":
      return {
        enabled: true,
        label: "Open vendor app",
        title: "Open the installed vendor manager",
      };
    case "vendor_installer_required":
      return {
        enabled: true,
        label: "Get vendor app",
        title: "Open the vendor manager download page",
      };
    case "ready":
      return {
        enabled: false,
        label: "Choose per format",
        title: "Choose an archive from the install review format list",
      };
    case "already_installed":
      return {
        enabled: false,
        label: "No handoff",
        title: "This package is already installed",
      };
  }
}

function installPlanView(
  result: InstallPlanResult | null,
  installScope: InstallScope,
  item: PackageSummary,
  installStatus: string,
  lifecycleNotice: LifecycleNotice | null,
  operation: OperationControlState | null,
  lifecycleEvents: InstallEvent[],
  lifecycleActionLocked: boolean,
) {
  if (!result) {
    return `
      <div class="plan-panel idle">
        <div class="plan-kicker">Install review</div>
        <p>${escapeHtml(installStatus)}</p>
        ${lifecycleNoticeMarkup(lifecycleNotice)}
        ${operationCancelMarkup("lifecycle", operation)}
        ${lifecycleTimelineMarkup(lifecycleEvents)}
      </div>
    `;
  }

  if (result.status !== "plan") {
    return installPlanProblem(result);
  }

  if (result.plan.slug !== item.slug) {
    return `
      <div class="plan-panel idle">
        <div class="plan-kicker">Install review</div>
        <p>Review install to load a plan for ${escapeHtml(item.name)}.</p>
        ${lifecycleNoticeMarkup(lifecycleNotice)}
        ${operationCancelMarkup("lifecycle", operation)}
        ${lifecycleTimelineMarkup(lifecycleEvents)}
      </div>
    `;
  }

  const plan = result.plan;
  const scopeControl =
    plan.status === "ready"
      ? installScopeControl(installScope, lifecycleActionLocked)
      : "";
  const installer = plan.installer
    ? `
      <div class="installer-box">
        <strong>${escapeHtml(plan.installer.name)}</strong>
        <span>${escapeHtml(plan.installer.installed_app_path ? "installed locally" : "download required")}</span>
      </div>
    `
    : "";

  return `
    <div class="plan-panel ${plan.status}">
      <div class="plan-header">
        <div>
          <div class="plan-kicker">Install review</div>
          <strong>${escapeHtml(installPlanTitle(plan.status))}</strong>
        </div>
        <span>${escapeHtml(formatInstallStatus(plan.status))}</span>
      </div>
      <p>${escapeHtml(plan.message)}</p>
      ${lifecycleNoticeMarkup(lifecycleNotice)}
      ${operationCancelMarkup("lifecycle", operation)}
      ${lifecycleTimelineMarkup(lifecycleEvents)}
      ${scopeControl}
      <dl class="plan-facts">
        <div><dt>Version</dt><dd>${escapeHtml(plan.version)}</dd></div>
        <div><dt>Scope</dt><dd>${escapeHtml(installScopeLabel(installScope))}</dd></div>
        <div><dt>Destination</dt><dd>${escapeHtml(plan.destination ?? "External handoff")}</dd></div>
        <div><dt>Installed</dt><dd>${escapeHtml(plan.installed_version ?? "No")}</dd></div>
      </dl>
      <div class="format-list">
        ${plan.formats.map((format) => planFormatRow(plan, format, lifecycleActionLocked)).join("")}
      </div>
      ${installer}
    </div>
  `;
}

function installScopeControl(
  selectedScope: InstallScope,
  lifecycleActionLocked: boolean,
) {
  return `
    <div class="install-scope-control" role="group" aria-label="Install destination">
      ${installScopeButton("user", selectedScope, lifecycleActionLocked)}
      ${installScopeButton("system", selectedScope, lifecycleActionLocked)}
    </div>
  `;
}

function installScopeButton(
  scope: InstallScope,
  selectedScope: InstallScope,
  lifecycleActionLocked: boolean,
) {
  const selected = scope === selectedScope;
  const activeClass = selected ? " active" : "";
  const pressed = selected ? "true" : "false";
  const disabled = lifecycleActionLocked ? " disabled" : "";
  const label = installScopeLabel(scope);
  const title =
    scope === "system"
      ? "Install into /Library/Audio/Plug-Ins"
      : "Install into ~/Library/Audio/Plug-Ins";
  return `
    <button
      class="install-scope-option${activeClass}"
      type="button"
      data-install-scope="${scope}" aria-pressed="${pressed}"
      title="${escapeHtml(title)}"${disabled}>
      ${escapeHtml(label)}
    </button>
  `;
}

function installScopeLabel(scope: InstallScope) {
  return scope === "system" ? "System library" : "User library";
}

function installPlanProblem(result: Exclude<InstallPlanResult, { status: "plan" }>) {
  const message = (() => {
    switch (result.status) {
      case "catalog_empty":
        return "The catalog cache is empty. Sync registries before reviewing installs.";
      case "not_found":
        return result.suggestions.length > 0
          ? `No package matched ${result.query}. Try ${result.suggestions.join(", ")}.`
          : `No package matched ${result.query}.`;
      case "not_installable":
        return `${result.name} is a ${result.product_type} catalog item, not a direct install target.`;
      case "version_not_found":
        return `${result.slug} does not have version ${result.requested_version}.`;
      case "format_unavailable":
        return `${result.slug} is not available as ${result.requested_format ?? "the requested format"}.`;
    }
  })();

  return `
    <div class="plan-panel problem">
      <div class="plan-kicker">Install review</div>
      <p>${escapeHtml(message)}</p>
    </div>
  `;
}

function planFormatRow(
  plan: PackageInstallPlan,
  format: InstallPlanFormat,
  lifecycleActionLocked: boolean,
) {
  const checksum = format.has_checksum ? "verified" : "no checksum";
  const archiveAction =
    planFormatAction(plan, format, lifecycleActionLocked) ??
    `<small>${escapeHtml(checksum)}</small>`;

  return `
    <div class="format-row">
      <strong>${escapeHtml(formatLabel(format.format))}</strong>
      <span>${escapeHtml(format.download_type)} / ${escapeHtml(format.install_type)}</span>
      ${archiveAction}
    </div>
  `;
}

function planFormatAction(
  plan: PackageInstallPlan,
  format: InstallPlanFormat,
  lifecycleActionLocked: boolean,
) {
  const label = formatLabel(format.format);
  const archiveLabel = archiveTypeLabel(format.install_type);
  const lockedLabel = `${LIFECYCLE_ACTION_LOCKED_LABEL} for ${label}`;

  if (canInstallFromUrl(plan, format)) {
    return formatInstallButton(
      [
        ["data-install-url-slug", plan.slug],
        ["data-install-url-format", format.format],
      ],
      lifecycleActionLocked ? lockedLabel : `Download and install ${label}`,
      lifecycleActionLocked
        ? LIFECYCLE_ACTION_LOCKED_LABEL
        : `Download and install ${label} ${archiveLabel} archive`,
      "package",
      lifecycleActionLocked,
    );
  }

  if (canInstallFromArchive(plan, format)) {
    return formatInstallButton(
      [
        ["data-install-slug", plan.slug],
        ["data-install-format", format.format],
      ],
      lifecycleActionLocked ? lockedLabel : `Choose ${label} archive`,
      lifecycleActionLocked
        ? LIFECYCLE_ACTION_LOCKED_LABEL
        : `Choose a local ${archiveLabel} archive for ${label}`,
      "folder-open",
      lifecycleActionLocked,
    );
  }

  return null;
}

function formatInstallButton(
  dataAttributes: Array<[string, string]>,
  label: string,
  title: string,
  icon: string,
  disabled: boolean,
) {
  const renderedDataAttributes = dataAttributes
    .map(([name, value]) => `${name}="${escapeHtml(value)}"`)
    .join(" ");
  return `
      <button class="format-install-button" type="button" ${renderedDataAttributes} aria-label="${escapeHtml(label)}" title="${escapeHtml(title)}" ${disabled ? "disabled" : ""}>
        <i data-lucide="${escapeHtml(icon)}" aria-hidden="true"></i>
      </button>
    `;
}

function formatCategory(item: PackageSummary) {
  return item.subcategory ? `${item.category} / ${item.subcategory}` : item.category;
}

function canInstallFromArchive(
  plan: PackageInstallPlan,
  format: InstallPlanFormat,
) {
  return (
    plan.status === "ready" &&
    !canInstallFromUrl(plan, format) &&
    isSupportedArchiveInstallType(format.install_type)
  );
}

function canInstallFromUrl(plan: PackageInstallPlan, format: InstallPlanFormat) {
  return (
    plan.status === "ready" &&
    format.download_type.toLowerCase() === "direct" &&
    format.source.trim().length > 0 &&
    isSupportedArchiveInstallType(format.install_type)
  );
}

function isSupportedArchiveInstallType(installType: string) {
  const normalized = installType.toLowerCase();
  return normalized === "zip" || normalized === "dmg";
}

function archiveTypeLabel(installType: string) {
  return installType.toUpperCase();
}
