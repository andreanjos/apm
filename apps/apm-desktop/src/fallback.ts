import type {
  DesktopInstallResult,
  DesktopRemoveResult,
  DesktopScanResult,
  DesktopUpdateResult,
  InstallHandoffResult,
  InstallPlanResult,
  InstallPlanStatus,
  InstallScope,
  PackageDetailsResult,
  PackageSummary,
  PackageUpdateSummary,
  SetPackagePinResult,
} from "./types";
import { fallbackSnapshot } from "./fallback-data";

export { fallbackSnapshot } from "./fallback-data";
export {
  fallbackImportModelCatalogPackage,
  fallbackImportModelManifest,
  fallbackInstallModelPackage,
  fallbackPlanModelChain,
  fallbackPlanModelRun,
  fallbackPullModelWeights,
  fallbackRemoveModelPackage,
  fallbackRunModel,
} from "./model-fallback";

export function fallbackInstallPlan(
  slug: string,
  installScope: InstallScope = "user",
): InstallPlanResult {
  const catalog =
    fallbackSnapshot.catalog.status === "matches"
      ? fallbackSnapshot.catalog.packages
      : [];
  const item = catalog.find((packageItem) => packageItem.slug === slug);

  if (!item) {
    return { status: "not_found", query: slug, suggestions: [] };
  }

  if (!item.is_installable) {
    return {
      status: "not_installable",
      slug: item.slug,
      name: item.name,
      product_type: item.product_type,
    };
  }

  const status = previewPlanStatus(item);

  return {
    status: "plan",
    plan: {
      slug: item.slug,
      name: item.name,
      vendor: item.vendor,
      version: item.version,
      status,
      destination: status === "ready" ? installScopeDestination(installScope) : null,
      scope: installScope,
      installed_version: item.installed_version ?? null,
      formats: item.formats.map((format) => ({
        format: format.format,
        install_type: format.install_type,
        download_type: format.download_type,
        source:
          previewFormatSource(item.slug, format.install_type, format.download_type),
        bundle_path: format.bundle_path,
        has_checksum: format.has_checksum,
      })),
      installer: status.startsWith("vendor_installer")
        ? {
            key: "vendor-manager",
            name: "Vendor manager",
            download_url: "https://example.com",
            homepage: "https://example.com",
            installed_app_path: null,
          }
        : null,
      message: fallbackPlanMessage(item, status, installScope),
    },
  };
}

export function fallbackPackageDetails(slug: string): PackageDetailsResult {
  if (fallbackSnapshot.catalog.status !== "matches") {
    return { status: "catalog_empty" };
  }

  const item = fallbackSnapshot.catalog.packages.find((packageItem) => packageItem.slug === slug);
  if (!item) {
    return { status: "not_found" };
  }

  return {
    status: "found",
    package: {
      summary: item,
      aliases: [],
      homepage: fallbackHomepage(slug),
      purchase_url: item.is_paid ? fallbackPurchaseUrl(slug) : null,
      available_versions: [item.version],
      bundle_ids: [],
    },
  };
}

export function fallbackInstallHandoff(slug: string): InstallHandoffResult {
  const planResult = fallbackInstallPlan(slug);
  if (planResult.status !== "plan") {
    return { status: "plan_unavailable", plan: planResult };
  }

  const { plan } = planResult;
  if (plan.status === "manual_required") {
    const source = plan.formats.find((format) => format.source.trim().length > 0)?.source;
    if (source) {
      return {
        status: "open",
        plan,
        handoff: {
          kind: "manual_download",
          label: "Open download page",
          target: { kind: "url", url: source },
          message: `Opening the ${plan.name} download page. Install it manually, then run apm scan.`,
        },
      };
    }
  }

  if (plan.status === "privileged_installer_required") {
    const source = plan.formats.find((format) => format.source.trim().length > 0)?.source;
    if (source) {
      return {
        status: "open",
        plan,
        handoff: {
          kind: "privileged_installer",
          label: "Open PKG download",
          target: { kind: "url", url: source },
          message: `Opening the ${plan.name} PKG download. Review the vendor installer prompt manually, then run apm scan after installation.`,
        },
      };
    }
  }

  if (plan.status === "app_store_required") {
    const source = plan.formats.find((format) => format.source.trim().length > 0)?.source;
    if (source) {
      return {
        status: "open",
        plan,
        handoff: {
          kind: "app_store",
          label: "Open App Store",
          target: { kind: "url", url: source },
          message: `Opening the ${plan.name} App Store listing. Install it with the App Store app, then run apm scan.`,
        },
      };
    }
  }

  if (plan.status === "vendor_installer_required" && plan.installer) {
    return {
      status: "open",
      plan,
      handoff: {
        kind: "vendor_download",
        label: `Get ${plan.installer.name}`,
        target: { kind: "url", url: plan.installer.download_url },
        message: `Opening the ${plan.installer.name} download page. Install ${plan.name}, then run apm scan.`,
      },
    };
  }

  return {
    status: "no_handoff",
    plan,
    reason: "No external handoff is needed for this package.",
  };
}

function fallbackHomepage(slug: string) {
  switch (slug) {
    case "fabfilter-pro-q":
      return "https://www.fabfilter.com/products/pro-q-4-equalizer-plug-in";
    case "surge-xt":
      return "https://surge-synthesizer.github.io/";
    case "ott":
      return "https://xferrecords.com/freeware";
    default:
      return null;
  }
}

function fallbackPurchaseUrl(slug: string) {
  switch (slug) {
    case "fabfilter-pro-q":
      return "https://www.fabfilter.com/shop/";
    default:
      return null;
  }
}

function previewPlanStatus(item: PackageSummary): InstallPlanStatus {
  if (item.installed && item.installed_version === item.version) {
    return "already_installed";
  }
  if (item.formats.some((format) => format.download_type === "managed")) {
    return "vendor_installer_required";
  }
  if (item.formats.some((format) => format.install_type === "mas")) {
    return "app_store_required";
  }
  if (item.formats.some((format) => format.install_type === "pkg")) {
    return "privileged_installer_required";
  }
  if (item.formats.some((format) => format.download_type === "manual")) {
    return "manual_required";
  }
  return "ready";
}

export function fallbackInstallFromArchive(
  slug: string,
  format: string,
  installScope: InstallScope = "user",
): DesktopInstallResult {
  const planResult = fallbackInstallPlan(slug, installScope);
  if (planResult.status !== "plan") {
    return {
      status: "completed",
      result: { status: "plan_unavailable", plan: planResult },
      events: [],
    };
  }

  const { plan } = planResult;
  if (plan.status !== "ready") {
    return {
      status: "completed",
      result: {
        status: "external_handoff_required",
        plan,
        reason: plan.message,
      },
      events: [],
    };
  }

  const selectedFormat = plan.formats.find(
    (candidate) => candidate.format.toLowerCase() === format.toLowerCase(),
  );
  if (!selectedFormat) {
    return {
      status: "completed",
      result: {
        status: "format_required",
        plan,
        available_formats: plan.formats.map((candidate) => candidate.format),
        reason: `${plan.name} is not available as ${format}.`,
      },
      events: [],
    };
  }

  return {
    status: "completed",
    result: {
      status: "installed",
      package: {
        slug: plan.slug,
        version: plan.version,
        vendor: plan.vendor,
        formats: [
          {
            format: selectedFormat.format,
            path: installBundlePath(plan.name, selectedFormat.format, installScope),
          },
        ],
        source: "preview",
        pinned: false,
        origin: "apm",
      },
    },
    events: [
      {
        event: "install_started",
        slug: plan.slug,
        version: plan.version,
        format_count: 1,
      },
      {
        event: "install_format_started",
        slug: plan.slug,
        format: selectedFormat.format,
      },
      {
        event: "install_archive_verified",
        slug: plan.slug,
        format: selectedFormat.format,
        path: "/Preview Downloads/apm-preview.zip",
        sha256: "preview",
      },
      {
        event: "install_archive_install_started",
        slug: plan.slug,
        format: selectedFormat.format,
        install_type: selectedFormat.install_type,
        path: "/Preview Downloads/apm-preview.zip",
      },
      {
        event: "install_quarantine_removal_started",
        slug: plan.slug,
        format: selectedFormat.format,
        path: installBundlePath(plan.name, selectedFormat.format, installScope),
      },
      {
        event: "install_format_placed",
        slug: plan.slug,
        format: selectedFormat.format,
        path: installBundlePath(plan.name, selectedFormat.format, installScope),
      },
      { event: "install_state_recording_started", slug: plan.slug },
      { event: "install_state_recorded", slug: plan.slug },
      { event: "install_finished", slug: plan.slug, installed_format_count: 1 },
    ],
  };
}

export function fallbackInstallFromUrl(
  slug: string,
  format: string,
  installScope: InstallScope = "user",
): DesktopInstallResult {
  const planResult = fallbackInstallPlan(slug, installScope);
  if (planResult.status !== "plan") {
    return {
      status: "completed",
      result: { status: "plan_unavailable", plan: planResult },
      events: [],
    };
  }

  const { plan } = planResult;
  const selectedFormat = plan.formats.find(
    (candidate) => candidate.format.toLowerCase() === format.toLowerCase(),
  );
  if (plan.status !== "ready" || !selectedFormat) {
    return fallbackInstallFromArchive(slug, format, installScope);
  }

  const archivePath = `~/Library/Caches/apm/downloads/${plan.slug}-${plan.version}-${format.toLowerCase()}.zip`;
  const bundlePath = installBundlePath(plan.name, selectedFormat.format, installScope);
  return {
    status: "completed",
    result: {
      status: "installed",
      package: {
        slug: plan.slug,
        version: plan.version,
        vendor: plan.vendor,
        formats: [{ format: selectedFormat.format, path: bundlePath }],
        source: "preview",
        pinned: false,
        origin: "apm",
      },
    },
    events: [
      {
        event: "install_started",
        slug: plan.slug,
        version: plan.version,
        format_count: 1,
      },
      {
        event: "install_format_started",
        slug: plan.slug,
        format: selectedFormat.format,
      },
      {
        event: "install_download_started",
        slug: plan.slug,
        format: selectedFormat.format,
        url: selectedFormat.source,
      },
      {
        event: "install_download_progress",
        slug: plan.slug,
        format: selectedFormat.format,
        bytes: 1024,
        total_bytes: 2048,
      },
      {
        event: "install_download_finished",
        slug: plan.slug,
        format: selectedFormat.format,
        path: archivePath,
        bytes: 2048,
      },
      {
        event: "install_archive_verified",
        slug: plan.slug,
        format: selectedFormat.format,
        path: archivePath,
        sha256: selectedFormat.has_checksum ? "preview" : "",
      },
      {
        event: "install_archive_install_started",
        slug: plan.slug,
        format: selectedFormat.format,
        install_type: selectedFormat.install_type,
        path: archivePath,
      },
      {
        event: "install_quarantine_removal_started",
        slug: plan.slug,
        format: selectedFormat.format,
        path: bundlePath,
      },
      {
        event: "install_format_placed",
        slug: plan.slug,
        format: selectedFormat.format,
        path: bundlePath,
      },
      { event: "install_state_recording_started", slug: plan.slug },
      { event: "install_state_recorded", slug: plan.slug },
      { event: "install_finished", slug: plan.slug, installed_format_count: 1 },
    ],
  };
}

export function fallbackUpdatePackage(
  slug: string,
  format: string | null = null,
): DesktopUpdateResult {
  const update = fallbackUpdateFor(slug);
  if (!update) {
    return {
      status: "completed",
      result: { status: "up_to_date", slug, version: "current" },
      events: [],
    };
  }

  if (update.action === "pinned") {
    return { status: "completed", result: { status: "pinned", update }, events: [] };
  }
  if (update.action === "external") {
    return { status: "completed", result: { status: "external", update }, events: [] };
  }

  const trackedFormats = [
    ...new Set(
      fallbackSnapshot.installed
        .find((item) => item.slug === slug)
        ?.formats.map((trackedFormat) => trackedFormat.format) ?? [],
    ),
  ];
  const selectedFormats = format ? [format] : trackedFormats;
  const installResult = fallbackInstallFormatsFromUrl(slug, selectedFormats);
  if (installResult.status === "failed") {
    return installResult;
  }
  if (installResult.result.status !== "installed") {
    return {
      status: "completed",
      result: {
        status: "install_unavailable",
        update,
        result: installResult.result,
      },
      events: installResult.events,
    };
  }

  return {
    status: "completed",
    result: {
      status: "updated",
      update,
      package: installResult.result.package,
    },
    events: installResult.events,
  };
}

function fallbackInstallFormatsFromUrl(
  slug: string,
  formats: string[],
): DesktopInstallResult {
  if (formats.length === 0) {
    const planResult = fallbackInstallPlan(slug);
    if (planResult.status !== "plan") {
      return {
        status: "completed",
        result: { status: "plan_unavailable", plan: planResult },
        events: [],
      };
    }

    return {
      status: "completed",
      result: {
        status: "format_required",
        plan: planResult.plan,
        available_formats: [],
        reason: "Update execution needs at least one tracked format.",
      },
      events: [],
    };
  }

  const results = formats.map((format) => fallbackInstallFromUrl(slug, format));
  const unavailable = results.find(
    (result) => result.status === "failed" || result.result.status !== "installed",
  );
  if (unavailable) {
    return unavailable;
  }

  const installedResults = results.flatMap((result) =>
    result.status === "completed" && result.result.status === "installed"
      ? [result.result.package]
      : [],
  );
  const firstPackage = installedResults[0];
  if (!firstPackage) {
    return results[0];
  }

  return {
    status: "completed",
    result: {
      status: "installed",
      package: {
        ...firstPackage,
        formats: installedResults.flatMap((packageItem) => packageItem?.formats ?? []),
      },
    },
    events: [
      {
        event: "install_started",
        slug,
        version: firstPackage.version,
        format_count: formats.length,
      },
      ...results.flatMap((result) =>
        result.events.filter(
          (event) =>
            event.event !== "install_started" &&
            event.event !== "install_state_recording_started" &&
            event.event !== "install_state_recorded" &&
            event.event !== "install_finished",
        ),
      ),
      { event: "install_state_recording_started", slug },
      { event: "install_state_recorded", slug },
      { event: "install_finished", slug, installed_format_count: formats.length },
    ],
  };
}

export function fallbackRemovePackage(slug: string): DesktopRemoveResult {
  const packageItem = fallbackSnapshot.installed.find((item) => item.slug === slug);
  if (!packageItem) {
    return {
      status: "completed",
      result: { status: "not_installed", slug },
      events: [],
    };
  }

  if (packageItem.origin !== "apm") {
    return {
      status: "completed",
      result: {
        status: "external_install_present",
        package: packageItem,
        reason: "This package was discovered by scan; apm will not delete externally installed files.",
      },
      events: [],
    };
  }

  return {
    status: "completed",
    result: {
      status: "removed",
      package: packageItem,
      removed_formats: packageItem.formats.map((format) => ({
        format: format.format,
        path: format.path,
        existed: true,
      })),
      state_only: false,
    },
    events: [
      {
        event: "remove_started",
        slug: packageItem.slug,
        version: packageItem.version,
        format_count: packageItem.formats.length,
      },
      ...packageItem.formats.map((format) => ({
        event: "remove_format_removed" as const,
        slug: packageItem.slug,
        format: format.format,
        path: format.path,
      })),
      { event: "remove_state_recorded", slug: packageItem.slug },
      {
        event: "remove_finished",
        slug: packageItem.slug,
        removed_format_count: packageItem.formats.length,
      },
    ],
  };
}

export function fallbackScanLibrary(): DesktopScanResult {
  return {
    status: "completed",
    result: {
      scanned_count: 2,
      visible_count: 2,
      matched_count: 2,
      tracked_count: 1,
      adopted_count: 1,
      learned_bundle_id_count: 1,
      au_count: 1,
      vst3_count: 1,
      plugins: [
        {
          name: "Valhalla Supermassive",
          version: "4.0.0",
          vendor: "Valhalla DSP",
          format: "vst3",
          scope: "user",
          path: "~/Library/Audio/Plug-Ins/VST3/ValhallaSupermassive.vst3",
          tracked_by_apm: true,
          origin: "external",
          registry_slug: "valhalla-supermassive",
          match_method: "name_only",
        },
        {
          name: "Surge XT",
          version: "1.3.4",
          vendor: "Surge Synth Team",
          format: "au",
          scope: "user",
          path: "~/Library/Audio/Plug-Ins/Components/Surge XT.component",
          tracked_by_apm: true,
          origin: "apm",
          registry_slug: "surge-xt",
          match_method: "bundle_id",
        },
      ],
    },
    events: [
      { event: "scan_started" },
      {
        event: "scan_finished",
        scanned_count: 2,
        matched_count: 2,
        adopted_count: 1,
      },
    ],
  };
}

export function fallbackSetPackagePin(
  slug: string,
  pinned: boolean,
): SetPackagePinResult {
  const packageItem = fallbackSnapshot.installed.find((item) => item.slug === slug);
  if (!packageItem) {
    return { status: "not_installed", slug };
  }

  const status = packageItem.pinned === pinned ? "unchanged" : "changed";
  return {
    status,
    package: { ...packageItem, pinned },
    pinned,
  };
}

function fallbackUpdateFor(slug: string): PackageUpdateSummary | null {
  return fallbackSnapshot.updates.status === "ready"
    ? fallbackSnapshot.updates.updates.find((update) => update.slug === slug) ?? null
    : null;
}

function fallbackPlanMessage(
  item: PackageSummary,
  status: InstallPlanStatus,
  installScope: InstallScope,
) {
  switch (status) {
    case "already_installed":
      return `${item.name} is already installed at version ${item.installed_version ?? item.version}.`;
    case "manual_required":
      return `${item.name} requires manual installation. Install it externally, then run apm scan.`;
    case "privileged_installer_required":
      return `${item.name} requires a PKG installer. apm will not run privileged installers until a privileged helper or escalation design exists.`;
    case "app_store_required":
      return `${item.name} is distributed through the Mac App Store. Open the listing, install it with the App Store app, then run apm scan.`;
    case "vendor_installer_available":
    case "vendor_installer_required":
      return `Use the vendor manager to install ${item.name}, then run apm scan.`;
    case "ready":
      return `Ready to install ${item.name} v${item.version} to ${installScopeDestination(installScope)}.`;
  }
}

function installScopeDestination(installScope: InstallScope) {
  return installScope === "system"
    ? "/Library/Audio/Plug-Ins/"
    : "~/Library/Audio/Plug-Ins/";
}

function installBundlePath(name: string, format: string, installScope: InstallScope) {
  return `${installScopeDestination(installScope)}${format}/${name}.${format}`;
}

function previewFormatSource(
  slug: string,
  installType: string,
  downloadType: string,
) {
  if (downloadType === "manual") {
    return "https://valhalladsp.com/shop/reverb/valhalla-supermassive/";
  }
  if (installType === "pkg") {
    return `https://example.com/${slug}.pkg`;
  }
  if (installType === "mas") {
    return "https://apps.apple.com/us/app/app-store-synth/id123456789";
  }
  return "preview";
}
