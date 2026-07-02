import type { DesktopSnapshot, PackageSummary } from "./types";
import type {
  CatalogFilters,
  DesktopViewState,
} from "./view-model";

export function defaultCatalogFilters(): CatalogFilters {
  return {
    availability: "all",
    productType: null,
    access: "all",
  };
}

export function currentCatalog(snapshot: DesktopSnapshot) {
  return snapshot.catalog.status === "matches" ? snapshot.catalog.packages : [];
}

export function visibleCatalogFor(state: DesktopViewState) {
  return filterCatalog(
    currentCatalog(state.snapshot),
    state.catalogSearchQuery,
    state.catalogFilters,
  );
}

export function selectedPackageFor(state: DesktopViewState) {
  return selectedPackageFromCatalog(visibleCatalogFor(state), state.selectedSlug);
}

export function selectedPackageFromCatalog(
  catalog: PackageSummary[],
  selectedSlug: string | null,
) {
  return catalog.find((item) => item.slug === selectedSlug) ?? catalog[0];
}

function filterCatalog(
  catalog: PackageSummary[],
  query: string,
  filters: CatalogFilters,
) {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return catalog.filter((item) => {
    if (!matchesCatalogFilters(item, filters)) {
      return false;
    }
    if (tokens.length === 0) {
      return true;
    }
    const searchText = packageSearchText(item);
    return tokens.every((token) => searchText.includes(token));
  });
}

export function catalogProductTypes(catalog: PackageSummary[]) {
  return [...new Set(catalog.map((item) => item.product_type))]
    .filter(Boolean)
    .sort((a, b) => a.localeCompare(b));
}

export function normalizeCatalogFilters(
  snapshot: DesktopSnapshot,
  filters: CatalogFilters,
): CatalogFilters {
  if (filters.productType === null) {
    return filters;
  }
  return catalogProductTypes(currentCatalog(snapshot)).includes(filters.productType)
    ? filters
    : { ...filters, productType: null };
}

export function hasActiveCatalogFilter(query: string, filters: CatalogFilters) {
  return (
    query.trim().length > 0 ||
    filters.availability !== "all" ||
    filters.productType !== null ||
    filters.access !== "all"
  );
}

function matchesCatalogFilters(item: PackageSummary, filters: CatalogFilters) {
  if (filters.availability === "installed" && !item.installed) {
    return false;
  }
  if (filters.availability === "available" && item.installed) {
    return false;
  }
  if (filters.productType !== null && item.product_type !== filters.productType) {
    return false;
  }
  if (filters.access === "free" && item.is_paid) {
    return false;
  }
  if (filters.access === "paid" && !item.is_paid) {
    return false;
  }
  return true;
}

function packageSearchText(item: PackageSummary) {
  return [
    item.name,
    item.slug,
    item.vendor,
    item.product_type,
    item.category,
    item.subcategory ?? "",
    item.license,
    item.description,
    ...item.formats.map((format) => format.format),
  ]
    .join(" ")
    .toLowerCase();
}
