import { defaultCatalogFilters } from "./catalog-view-model";
import { fallbackInstallPlan } from "./fallback";
import { fallbackSnapshot } from "./fallback-data";
import { renderApp } from "./view";
import type { OperationStatus } from "./types";
import type { DesktopViewState, WorkspaceSection } from "./view-model";

const tests: Array<[string, () => void]> = [];

test("renders catalog workspace with package search", () => {
  const html = renderApp(viewState("catalog"));

  assertIncludes(html, 'data-workspace-section="catalog" aria-current="page"', "catalog nav");
  assertIncludes(html, "Package catalog", "catalog heading");
  assertIncludes(html, 'id="catalog-search"', "catalog search");
  assertIncludes(html, 'aria-label="Setup status"', "catalog setup readiness");
  assertEqual(
    countOccurrences(html, 'aria-label="Setup status"'),
    1,
    "catalog setup panel count",
  );
  assertIncludes(html, "https://www.fabfilter.com/products/pro-q-4-equalizer-plug-in", "package homepage");
  assertIncludes(html, "FabFilter EQ", "package alias");
  assertIncludes(html, "4.01", "package previous version");
  assertIncludes(html, "com.fabfilter.Pro-Q.4", "package bundle ID");
  assertExcludes(html, "Operation history", "diagnostics hidden");
});

test("locks install inspector actions during install operations", () => {
  const html = renderApp({
    ...viewState("catalog"),
    selectedSlug: "surge-xt",
    installPlan: fallbackInstallPlan("surge-xt"),
    lifecycleOperation: { operationId: "op-install", canceling: false },
  });

  assertIncludes(
    html,
    'id="review-install" type="button" aria-label="Install operation running for surge-xt" title="Install operation running" disabled',
    "review install locked",
  );
  assertIncludes(
    html,
    'data-install-url-slug="surge-xt" data-install-url-format="AU" aria-label="Install operation running for AU" title="Install operation running" disabled',
    "direct install format locked",
  );
  assertIncludes(html, "Cancel operation", "install cancel action remains available");
});

test("renders install scope controls for ready install plans", () => {
  const html = renderApp({
    ...viewState("catalog"),
    selectedSlug: "surge-xt",
    installPlan: fallbackInstallPlan("surge-xt", "system"),
    installScope: "system",
  });

  assertIncludes(html, 'data-install-scope="user"', "user install scope control");
  assertIncludes(html, 'data-install-scope="system"', "system install scope control");
  assertIncludes(
    html,
    'data-install-scope="system" aria-pressed="true"',
    "system install scope selected",
  );
  assertIncludes(html, "/Library/Audio/Plug-Ins/", "system destination rendered");
});

test("keeps first-run setup visible outside catalog", () => {
  const libraryHtml = renderApp(viewState("library"));
  const runtimeHtml = renderApp(viewState("runtime"));

  assertIncludes(libraryHtml, 'aria-label="Setup status"', "library setup readiness");
  assertIncludes(libraryHtml, "data-setup-service-action", "library setup service action");
  assertIncludes(runtimeHtml, 'aria-label="Setup status"', "runtime setup readiness");
  assertIncludes(runtimeHtml, "data-setup-service-action", "runtime setup service action");
  assertEqual(
    countOccurrences(runtimeHtml, 'aria-label="Setup status"'),
    1,
    "runtime setup panel count",
  );
});

test("hides global setup when readiness is complete", () => {
  const state = viewState("library");
  const html = renderApp({
    ...state,
    serviceSession: readyServiceSession(),
    snapshot: readySnapshot(state),
  });

  assertExcludes(html, 'aria-label="Setup status"', "ready setup hidden");
  assertIncludes(html, "Installed library", "library still rendered");
});

test("renders library workspace without catalog table", () => {
  const html = renderApp(viewState("library"));

  assertIncludes(html, 'data-workspace-section="library" aria-current="page"', "library nav");
  assertIncludes(html, "Installed library", "library heading");
  assertIncludes(html, "surge-xt", "library rows");
  assertIncludes(html, 'data-library-health="external"', "external library health");
  assertIncludes(html, 'data-library-health="update-ready"', "update-ready library health");
  assertIncludes(html, 'data-update-all-packages', "update all action");
  assertIncludes(html, "Update ready", "update all label");
  assertExcludes(html, 'id="catalog-search"', "catalog search hidden");
});

test("locks library row actions during library operations", () => {
  const html = renderApp({
    ...viewState("library"),
    libraryOperation: { operationId: "op-library", canceling: false },
  });

  assertIncludes(html, "Library operation running for surge-xt", "locked library row label");
  assertIncludes(
    html,
    'data-update-slug="surge-xt" aria-label="Library operation running for surge-xt" title="Library operation running" disabled',
    "locked update action",
  );
  assertIncludes(
    html,
    'data-remove-slug="surge-xt" aria-label="Library operation running for surge-xt" title="Library operation running" disabled',
    "locked remove action",
  );
  assertIncludes(
    html,
    'data-pin-slug="surge-xt" aria-label="Library operation running for surge-xt" disabled',
    "locked pin action",
  );
  assertIncludes(html, "Cancel operation", "library cancel action remains available");
});

test("renders current health for managed packages with no update", () => {
  const state = viewState("library");
  const installed = fallbackSnapshot.installed.find((item) => item.slug === "surge-xt");
  if (!installed || fallbackSnapshot.updates.status !== "ready") {
    throw new Error("fallback library fixture should include surge-xt and ready updates");
  }
  const html = renderApp({
    ...state,
    snapshot: {
      ...state.snapshot,
      installed: [{ ...installed, version: "1.3.4" }],
      updates: { ...fallbackSnapshot.updates, updates: [] },
    },
    updateAllCount: 0,
  });

  assertIncludes(html, 'data-library-health="current"', "current library health");
});

test("renders runtime and diagnostics workspaces independently", () => {
  const runtimeHtml = renderApp(viewState("runtime"));
  const diagnosticsHtml = renderApp(viewState("diagnostics"));

  assertIncludes(runtimeHtml, "Audio-AI runtime", "runtime title");
  assertIncludes(runtimeHtml, "Model packages", "model panel");
  assertExcludes(runtimeHtml, "System readiness", "diagnostics hidden from runtime");
  assertIncludes(diagnosticsHtml, "System diagnostics", "diagnostics title");
  assertIncludes(
    diagnosticsHtml,
    "data-refresh-diagnostics-action",
    "doctor refresh action",
  );
  assertIncludes(diagnosticsHtml, "Operation history", "operation history");
  assertExcludes(diagnosticsHtml, "Model packages", "runtime hidden from diagnostics");
});

test("locks diagnostics retries during active operations", () => {
  const state = viewState("diagnostics");
  const html = renderApp({
    ...state,
    snapshot: {
      ...state.snapshot,
      recovery: {
        interrupted_count: 1,
        retryable_count: 1,
        candidates: [],
      },
      operations: [
        operationStatus({
          operation_id: "op-sync-failed",
          kind: "registry_sync",
          state: "failed",
          request: { kind: "registry_sync" },
        }),
      ],
    },
    syncOperation: { operationId: "op-sync-active", canceling: false },
  });

  assertIncludes(
    html,
    'data-retry-operation-id="op-sync-failed"',
    "diagnostics retry action rendered",
  );
  assertIncludes(
    html,
    'aria-label="Sync operation running; retry unavailable"',
    "diagnostics operation retry locked label",
  );
  assertIncludes(
    html,
    'title="Sync operation running"',
    "diagnostics operation retry locked title",
  );
  assertIncludes(
    html,
    'aria-label="Operation running; retry unavailable"',
    "diagnostics recovery retry locked label",
  );
  assertIncludes(
    html,
    'title="Operation running"',
    "diagnostics recovery retry locked title",
  );
});

test("locks diagnostics retries during local model actions", () => {
  const state = viewState("diagnostics");
  const html = renderApp({
    ...state,
    snapshot: {
      ...state.snapshot,
      operations: [
        operationStatus({
          operation_id: "op-model-failed",
          kind: "model_run",
          state: "failed",
          request: {
            kind: "model_run",
            name: "demucs",
            version: "4.0.1",
            request: {
              input_path: "/tmp/mix.wav",
              output_path: "/tmp/stems",
              params: {},
            },
          },
        }),
      ],
    },
    importingCatalogModelId: "demucs@4.0.1",
  });

  assertIncludes(
    html,
    'data-retry-operation-id="op-model-failed"',
    "diagnostics model retry action rendered",
  );
  assertIncludes(
    html,
    'aria-label="Model action running; retry unavailable"',
    "diagnostics model retry locked label",
  );
  assertIncludes(
    html,
    'title="Model action running"',
    "diagnostics model retry locked title",
  );
});

runTests();

function viewState(workspaceSection: WorkspaceSection): DesktopViewState {
  const catalog = fallbackSnapshot.catalog;
  if (catalog.status !== "matches") {
    throw new Error("fallback catalog fixture should include package matches");
  }

  return {
    serviceSession: fallbackSnapshot.service,
    snapshot: fallbackSnapshot,
    workspaceSection,
    selectedSlug: "fabfilter-pro-q",
    catalogSearchQuery: "",
    catalogFilters: defaultCatalogFilters(),
    packageDetails: {
      status: "found",
      package: {
        summary: catalog.packages[0],
        aliases: ["Pro-Q", "FabFilter EQ"],
        homepage: "https://www.fabfilter.com/products/pro-q-4-equalizer-plug-in",
        purchase_url: "https://www.fabfilter.com/shop/",
        available_versions: ["4.02", "4.01"],
        bundle_ids: ["com.fabfilter.Pro-Q.4"],
      },
    },
    packageDetailsLoading: false,
    packageDetailsError: null,
    installPlan: null,
    installScope: "user",
    installStatus: "No install plan loaded",
    lifecycleNotice: null,
    syncStatus: "Preview data",
    pendingArchiveInstall: null,
    pendingUrlInstall: null,
    pendingInstallHandoff: null,
    pendingUpdateAllPackages: null,
    pendingUpdatePackage: null,
    pendingRemovePackage: null,
    updateAllCount: 2,
    syncOperation: null,
    lifecycleOperation: null,
    libraryOperation: null,
    lifecycleEvents: [],
    libraryNotice: null,
    libraryEvents: [],
    diagnosticsNotice: null,
    diagnosticsRefreshing: false,
    modelEvents: [],
    modelOperation: null,
    modelNotice: null,
    modelStoreInitializing: false,
    modelRunPlan: null,
    modelChainPlan: null,
    modelChainSteps: [],
    planningModelChain: false,
    modelImporting: false,
    importingCatalogModelId: null,
    installingModelId: null,
    planningModelId: null,
    pullingModelId: null,
    removingModelId: null,
    runningModelId: null,
    modelSearchQuery: "",
    retryingOperationId: null,
    retryingRecovery: false,
  };
}

function test(name: string, run: () => void) {
  tests.push([name, run]);
}

function runTests() {
  let failureCount = 0;
  for (const [name, run] of tests) {
    try {
      run();
      console.log(`ok ${name}`);
    } catch (error) {
      failureCount += 1;
      console.error(`not ok ${name}`);
      console.error(errorMessage(error));
    }
  }
  if (failureCount > 0) {
    throw new Error(`${failureCount} unit ${failureCount === 1 ? "test" : "tests"} failed.`);
  }
}

function assertIncludes(html: string, expected: string, message: string) {
  if (!html.includes(expected)) {
    throw new Error(`${message}: expected rendered HTML to include ${expected}`);
  }
}

function assertExcludes(html: string, unexpected: string, message: string) {
  if (html.includes(unexpected)) {
    throw new Error(`${message}: expected rendered HTML not to include ${unexpected}`);
  }
}

function assertEqual(actual: number, expected: number, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

function countOccurrences(text: string, pattern: string) {
  return text.split(pattern).length - 1;
}

function readySnapshot(state: DesktopViewState): DesktopViewState["snapshot"] {
  return {
    ...state.snapshot,
    diagnostics: {
      summary: { ok: 4, warnings: 0, failures: 0 },
      checks: [
        {
          name: "Model store",
          status: "ok",
          detail: "directories ready: ~/.apm",
        },
      ],
    },
  };
}

function readyServiceSession(): DesktopViewState["serviceSession"] {
  return {
    ...fallbackSnapshot.service,
    status: "started",
    pid: 123,
    token_available: true,
    message: "Local service ready",
  };
}

function operationStatus(overrides: Partial<OperationStatus> = {}): OperationStatus {
  return {
    operation_id: "op-1",
    kind: "install_url",
    request: null,
    state: "running",
    created_at: "2026-06-30T00:00:00Z",
    started_at: null,
    finished_at: null,
    result: null,
    error: null,
    events: [],
    ...overrides,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
