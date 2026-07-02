import type { PackageSummary } from "./types";
import type {
  CatalogAccessFilter,
  CatalogAvailabilityFilter,
  DesktopViewState,
} from "./view-model";
import {
  catalogProductTypes,
  currentCatalog,
  hasActiveCatalogFilter,
  selectedPackageFromCatalog,
  visibleCatalogFor,
} from "./catalog-view-model";
import { packageInspector } from "./package-inspector";
import { escapeHtml } from "./view-utils";

export type CatalogWorkspaceRenderData = {
  catalog: PackageSummary[];
  selectedPackage: PackageSummary | undefined;
  installedCount: number;
  installedFormats: number;
  readyCount: number;
  metricValue: number;
  metricCaption: string;
};

export function catalogWorkspaceRenderData(
  state: DesktopViewState,
  installedCount: number,
): CatalogWorkspaceRenderData {
  const catalog = visibleCatalogFor(state);
  const selectedPackage = selectedPackageFromCatalog(catalog, state.selectedSlug);
  const totalMatches =
    state.snapshot.catalog.status === "matches"
      ? state.snapshot.catalog.total_matches
      : 0;
  const installedFormats = new Set(
    state.snapshot.installed.flatMap((item) =>
      item.formats.map((format) => format.format),
    ),
  ).size;
  const readyCount = catalog.filter((item) => item.installed).length;
  const activeCatalogFilter = hasActiveCatalogFilter(
    state.catalogSearchQuery,
    state.catalogFilters,
  );

  return {
    catalog,
    selectedPackage,
    installedCount,
    installedFormats,
    readyCount,
    metricValue: activeCatalogFilter ? catalog.length : totalMatches,
    metricCaption: activeCatalogFilter
      ? "Filtered package matches"
      : "Visible package matches",
  };
}

export function catalogWorkspaceMarkup(
  state: DesktopViewState,
  data: CatalogWorkspaceRenderData,
) {
  return `
    <section class="metrics" aria-label="Package manager status">
      ${metric("Sources", state.snapshot.source_count.toString(), "Configured registries")}
      ${metric("Catalog", data.metricValue.toLocaleString(), data.metricCaption)}
      ${metric("Installed", data.installedCount.toString(), "Tracked local packages")}
      ${metric("Formats", data.installedFormats.toString(), "Detected bundle types")}
    </section>

    <section class="content-grid">
      <div class="panel catalog-panel">
        <div class="panel-header">
          <div>
            <p class="eyebrow">Browse</p>
            <h2>Package catalog</h2>
          </div>
          <span class="status-pill">${data.readyCount} ready locally</span>
        </div>
        ${catalogFilterBar(state)}
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Vendor</th>
                <th>Type</th>
                <th>Version</th>
                <th>Access</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              ${data.catalog.map((item) => packageRow(item, item.slug === data.selectedPackage?.slug)).join("") || emptyRow(catalogEmptyMessage(state), 6)}
            </tbody>
          </table>
        </div>
      </div>

      <aside class="panel inspector-panel">
        <div class="panel-header">
          <div>
            <p class="eyebrow">Selected</p>
            <h2>${escapeHtml(data.selectedPackage?.name ?? "No package selected")}</h2>
          </div>
          <i data-lucide="info" class="panel-icon" aria-hidden="true"></i>
        </div>
        ${packageInspector(data.selectedPackage, state)}
      </aside>
    </section>
  `;
}

export function catalogSearchMarkup(query: string) {
  return `
    <label class="search-box" for="catalog-search">
      <i data-lucide="search" aria-hidden="true"></i>
      <input id="catalog-search" type="search" value="${escapeHtml(query)}" placeholder="Search packages" aria-label="Search packages" autocomplete="off" spellcheck="false">
      <kbd>Cmd K</kbd>
    </label>
  `;
}

function metric(label: string, value: string, caption: string) {
  return `
    <div class="metric">
      <span>${label}</span>
      <strong>${value}</strong>
      <small>${caption}</small>
    </div>
  `;
}

function catalogFilterBar(state: DesktopViewState) {
  const productTypes = catalogProductTypes(currentCatalog(state.snapshot));
  return `
    <div class="catalog-toolbar" aria-label="Catalog filters">
      <div class="filter-group">
        <span id="catalog-availability-label">Status</span>
        <div class="segmented-control" aria-labelledby="catalog-availability-label">
          ${availabilityFilterButton("all", "All", state.catalogFilters.availability)}
          ${availabilityFilterButton("installed", "Installed", state.catalogFilters.availability)}
          ${availabilityFilterButton("available", "Available", state.catalogFilters.availability)}
        </div>
      </div>
      <label class="filter-group type-filter" for="catalog-type-filter">
        <span>Type</span>
        <select id="catalog-type-filter">
          <option value=""${state.catalogFilters.productType === null ? " selected" : ""}>All types</option>
          ${productTypes.map((type) => catalogTypeOption(type, state.catalogFilters.productType)).join("")}
        </select>
      </label>
      <div class="filter-group">
        <span id="catalog-access-label">Access</span>
        <div class="segmented-control" aria-labelledby="catalog-access-label">
          ${accessFilterButton("all", "All", state.catalogFilters.access)}
          ${accessFilterButton("free", "Free", state.catalogFilters.access)}
          ${accessFilterButton("paid", "Paid", state.catalogFilters.access)}
        </div>
      </div>
    </div>
  `;
}

function catalogTypeOption(type: string, selectedType: string | null) {
  return `
    <option value="${escapeHtml(type)}"${selectedType === type ? " selected" : ""}>
      ${escapeHtml(type)}
    </option>
  `;
}

function availabilityFilterButton(
  filter: CatalogAvailabilityFilter,
  label: string,
  current: CatalogAvailabilityFilter,
) {
  return `
    <button class="segmented-button" type="button" data-catalog-availability-filter="${filter}" aria-pressed="${current === filter}">
      ${label}
    </button>
  `;
}

function accessFilterButton(
  filter: CatalogAccessFilter,
  label: string,
  current: CatalogAccessFilter,
) {
  return `
    <button class="segmented-button" type="button" data-catalog-access-filter="${filter}" aria-pressed="${current === filter}">
      ${label}
    </button>
  `;
}

function packageRow(item: PackageSummary, selected: boolean) {
  return `
    <tr class="package-row${selected ? " selected" : ""}" data-package-slug="${escapeHtml(item.slug)}" tabindex="0">
      <td data-label="Name">
        <strong>${escapeHtml(item.name)}</strong>
        <small>${escapeHtml(item.slug)}</small>
      </td>
      <td data-label="Vendor">${escapeHtml(item.vendor)}</td>
      <td data-label="Type">${escapeHtml(item.product_type)}</td>
      <td data-label="Version">${escapeHtml(item.version)}</td>
      <td data-label="Access">${item.is_paid ? "paid" : "free"}</td>
      <td data-label="Status">${item.installed ? `<span class="ready">installed</span>` : "available"}</td>
    </tr>
  `;
}

function emptyRow(message: string, span: number) {
  return `<tr><td colspan="${span}" class="empty-cell">${escapeHtml(message)}</td></tr>`;
}

function catalogEmptyMessage(state: DesktopViewState) {
  if (currentCatalog(state.snapshot).length === 0) {
    return "No catalog data yet. Run sync to populate the registry cache.";
  }
  if (state.catalogSearchQuery.trim().length === 0) {
    return "No packages match these filters.";
  }
  return "No packages match this search.";
}
