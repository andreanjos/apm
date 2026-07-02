import type {
  DesktopSnapshot,
  InstalledPackageSummary,
  PackageUpdateAction,
  ScanPackagesResult,
} from "./types";

export function withPreviewInstall(
  snapshot: DesktopSnapshot,
  packageItem: InstalledPackageSummary,
): DesktopSnapshot {
  const installed = [
    ...snapshot.installed.filter((item) => item.slug !== packageItem.slug),
    packageItem,
  ].sort((left, right) => left.slug.localeCompare(right.slug));

  return {
    ...snapshot,
    installed,
    catalog:
      snapshot.catalog.status === "matches"
        ? {
            ...snapshot.catalog,
            packages: snapshot.catalog.packages.map((item) =>
              item.slug === packageItem.slug
                ? {
                    ...item,
                    installed: true,
                    installed_version: packageItem.version,
                  }
                : item,
            ),
          }
        : snapshot.catalog,
    updates:
      snapshot.updates.status === "ready"
        ? {
            ...snapshot.updates,
            installed_count: installed.length,
            updates: snapshot.updates.updates.filter(
              (update) => update.slug !== packageItem.slug,
            ),
          }
        : snapshot.updates,
  };
}

export function withPreviewRemove(
  snapshot: DesktopSnapshot,
  slug: string,
): DesktopSnapshot {
  return {
    ...snapshot,
    installed: snapshot.installed.filter((item) => item.slug !== slug),
    catalog:
      snapshot.catalog.status === "matches"
        ? {
            ...snapshot.catalog,
            packages: snapshot.catalog.packages.map((item) =>
              item.slug === slug
                ? { ...item, installed: false, installed_version: null }
                : item,
            ),
          }
        : snapshot.catalog,
    updates:
      snapshot.updates.status === "ready"
        ? {
            ...snapshot.updates,
            installed_count: Math.max(0, snapshot.updates.installed_count - 1),
            updates: snapshot.updates.updates.filter((update) => update.slug !== slug),
          }
        : snapshot.updates,
  };
}

export function withPreviewPackagePin(
  snapshot: DesktopSnapshot,
  slug: string,
  pinned: boolean,
): DesktopSnapshot {
  const installed = snapshot.installed.map((item) =>
    item.slug === slug ? { ...item, pinned } : item,
  );
  const updates =
    snapshot.updates.status === "ready"
      ? snapshot.updates.updates.map((update) =>
          update.slug === slug
            ? {
                ...update,
                pinned,
                action: updateActionForPin(update.origin, pinned),
              }
            : update,
        )
      : [];

  return {
    ...snapshot,
    installed,
    updates:
      snapshot.updates.status === "ready"
        ? {
            ...snapshot.updates,
            updates,
            pinned_count: updates.filter((update) => update.action === "pinned").length,
            external_count: updates.filter((update) => update.action === "external").length,
          }
        : snapshot.updates,
  };
}

export function withPreviewScan(
  snapshot: DesktopSnapshot,
  result: ScanPackagesResult,
): DesktopSnapshot {
  const scannedPackages = new Map<string, InstalledPackageSummary>();
  for (const plugin of result.plugins) {
    if (!plugin.tracked_by_apm || !plugin.registry_slug) {
      continue;
    }

    const existing =
      scannedPackages.get(plugin.registry_slug) ??
      snapshot.installed.find((item) => item.slug === plugin.registry_slug) ??
      {
        slug: plugin.registry_slug,
        version: plugin.version,
        vendor: plugin.vendor,
        formats: [],
        source: "scan",
        pinned: false,
        origin: plugin.origin ?? "external",
      };
    const scannedFormat = { format: plugin.format.toUpperCase(), path: plugin.path };
    const formats = existing.formats.some(
      (format) => format.format === scannedFormat.format && format.path === scannedFormat.path,
    )
      ? existing.formats
      : [...existing.formats, scannedFormat];

    scannedPackages.set(plugin.registry_slug, {
      ...existing,
      version: existing.version || plugin.version,
      vendor: existing.vendor || plugin.vendor,
      formats,
    });
  }

  const installed = [
    ...snapshot.installed.filter((item) => !scannedPackages.has(item.slug)),
    ...scannedPackages.values(),
  ].sort((left, right) => left.slug.localeCompare(right.slug));

  return {
    ...snapshot,
    installed,
    catalog:
      snapshot.catalog.status === "matches"
        ? {
            ...snapshot.catalog,
            packages: snapshot.catalog.packages.map((item) => {
              const installedPackage = installed.find((candidate) => candidate.slug === item.slug);
              return installedPackage
                ? {
                    ...item,
                    installed: true,
                    installed_version: installedPackage.version,
                  }
                : item;
            }),
          }
        : snapshot.catalog,
    updates:
      snapshot.updates.status === "ready"
        ? {
            ...snapshot.updates,
            installed_count: installed.length,
          }
        : snapshot.updates,
  };
}

function updateActionForPin(
  origin: InstalledPackageSummary["origin"],
  pinned: boolean,
): PackageUpdateAction {
  if (pinned) {
    return "pinned";
  }
  return origin === "external" ? "external" : "installable";
}
