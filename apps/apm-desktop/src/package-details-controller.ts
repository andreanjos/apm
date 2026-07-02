import type { PackageDetailsResult } from "./types";

export type PackageDetailsControllerState = {
  packageDetails: PackageDetailsResult | null;
  packageDetailsLoading: boolean;
  packageDetailsError: string | null;
};

type PackageDetailsControllerHost = {
  loadPackageDetails(slug: string): Promise<PackageDetailsResult>;
  formatError(error: unknown): string;
  render(): void;
};

export function createPackageDetailsController(host: PackageDetailsControllerHost) {
  let details: PackageDetailsResult | null = null;
  let loading = false;
  let error: string | null = null;
  let slug: string | null = null;
  let requestId = 0;

  function state(selectedSlug: string | null): PackageDetailsControllerState {
    const isCurrentSelection = slug !== null && slug === selectedSlug;
    return {
      packageDetails: isCurrentSelection ? details : null,
      packageDetailsLoading: isCurrentSelection && loading,
      packageDetailsError: isCurrentSelection ? error : null,
    };
  }

  function clear() {
    requestId += 1;
    details = null;
    loading = false;
    error = null;
    slug = null;
  }

  async function load(selectedSlug: string | null) {
    if (!selectedSlug) {
      clear();
      host.render();
      return;
    }

    const currentRequestId = requestId + 1;
    requestId = currentRequestId;

    details = null;
    loading = true;
    error = null;
    slug = selectedSlug;
    host.render();

    try {
      const result = await host.loadPackageDetails(selectedSlug);
      if (requestId !== currentRequestId) {
        return;
      }
      details = result;
    } catch (loadError) {
      if (requestId !== currentRequestId) {
        return;
      }
      error = host.formatError(loadError);
    } finally {
      if (requestId === currentRequestId) {
        loading = false;
        host.render();
      }
    }
  }

  return {
    clear,
    load,
    state,
  };
}
