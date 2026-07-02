import { fallbackSnapshot } from "./fallback-data";
import { setupChecklistSection } from "./setup-checklist";
import type {
  DesktopServiceSession,
  DesktopSnapshot,
} from "./types";

const tests: Array<[string, () => void]> = [];

test("renders first-run setup actions for preview service", () => {
  const html = setupChecklistSection(fallbackSnapshot, fallbackSnapshot.service, null);

  assertIncludes(html, 'aria-label="Setup status"', "setup panel");
  assertIncludes(html, "Local service", "service item");
  assertIncludes(html, "Preview", "preview state");
  assertIncludes(html, "data-setup-service-action", "service action");
});

test("renders registry sync action for empty catalog", () => {
  const html = setupChecklistSection(
    { ...fallbackSnapshot, catalog: { status: "catalog_empty" } },
    readyService(),
    null,
  );

  assertIncludes(html, "Catalog", "catalog item");
  assertIncludes(html, "Empty", "empty catalog state");
  assertIncludes(html, "data-setup-sync-action", "sync action");
  assertButtonDisabled(html, "data-setup-sync-action", false, "sync action ready");
});

test("renders model store initialize action from diagnostics warning", () => {
  const html = setupChecklistSection(
    {
      ...readySnapshot(),
      diagnostics: {
        summary: { ok: 3, warnings: 1, failures: 0 },
        checks: [
          {
            name: "Model store",
            status: "warning",
            detail: "not initialized: ~/.apm",
          },
        ],
      },
    },
    readyService(),
    null,
  );

  assertIncludes(html, "Model store", "model store item");
  assertIncludes(html, "Initialize", "model store state");
  assertIncludes(html, "data-setup-model-store-action", "model store action");
  assertButtonDisabled(
    html,
    "data-setup-model-store-action",
    false,
    "model store action ready",
  );
});

test("disables service-backed setup repair actions until local service is ready", () => {
  const html = setupChecklistSection(
    {
      ...readySnapshot(),
      catalog: { status: "catalog_empty" },
      diagnostics: {
        summary: { ok: 2, warnings: 2, failures: 0 },
        checks: [
          {
            name: "Registry cache",
            status: "warning",
            detail: "catalog source has not been synced",
          },
          {
            name: "Model store",
            status: "warning",
            detail: "missing directories: manifests",
          },
        ],
      },
    },
    stoppedService(),
    null,
  );

  assertIncludes(html, "Start local service first", "service-required label");
  assertButtonDisabled(html, "data-setup-sync-action", true, "sync action locked");
  assertButtonDisabled(
    html,
    "data-setup-model-store-action",
    true,
    "model store action locked",
  );
  assertButtonDisabled(
    html,
    "data-setup-diagnostics-action",
    false,
    "diagnostics navigation stays available",
  );
});

test("renders diagnostics action from doctor warnings", () => {
  const html = setupChecklistSection(
    {
      ...readySnapshot(),
      diagnostics: {
        summary: { ok: 3, warnings: 1, failures: 0 },
        checks: [
          {
            name: "Registry cache",
            status: "warning",
            detail: "catalog source has not been synced",
          },
          {
            name: "Model store",
            status: "ok",
            detail: "directories ready: ~/.apm",
          },
        ],
      },
    },
    readyService(),
    null,
  );

  assertIncludes(html, "Diagnostics", "diagnostics item");
  assertIncludes(html, "1 warnings", "diagnostics warning state");
  assertIncludes(html, "data-setup-diagnostics-action", "diagnostics action");
  assertIncludes(html, "Open diagnostics", "diagnostics action label");
});

test("renders diagnostics action from doctor failures", () => {
  const html = setupChecklistSection(
    {
      ...readySnapshot(),
      diagnostics: {
        summary: { ok: 3, warnings: 2, failures: 1 },
        checks: [
          {
            name: "State file",
            status: "failure",
            detail: "state path is not writable",
          },
          {
            name: "Model store",
            status: "ok",
            detail: "directories ready: ~/.apm",
          },
        ],
      },
    },
    readyService(),
    null,
  );

  assertIncludes(html, "Diagnostics", "diagnostics item");
  assertIncludes(html, "1 failed", "diagnostics failure state");
  assertIncludes(html, "data-setup-diagnostics-action", "diagnostics action");
});

test("disables model store initialize action while it is running", () => {
  const html = setupChecklistSection(
    {
      ...readySnapshot(),
      diagnostics: {
        summary: { ok: 3, warnings: 1, failures: 0 },
        checks: [
          {
            name: "Model store",
            status: "warning",
            detail: "missing directories: manifests",
          },
        ],
      },
    },
    readyService(),
    null,
    { modelStoreInitializing: true },
  );

  assertIncludes(html, "Initializing model store", "model store busy label");
  assertButtonDisabled(
    html,
    "data-setup-model-store-action",
    true,
    "model store action disabled",
  );
});

test("disables model store initialize action during a model action", () => {
  const html = setupChecklistSection(
    {
      ...readySnapshot(),
      diagnostics: {
        summary: { ok: 3, warnings: 1, failures: 0 },
        checks: [
          {
            name: "Model store",
            status: "warning",
            detail: "missing directories: manifests",
          },
        ],
      },
    },
    readyService(),
    null,
    { modelActionLocked: true },
  );

  assertIncludes(html, "Model action running", "model action lock label");
  assertButtonDisabled(
    html,
    "data-setup-model-store-action",
    true,
    "model store action locked",
  );
});

test("hides setup panel when readiness is complete", () => {
  const html = setupChecklistSection(readySnapshot(), readyService(), null);

  assertEqual(html, "", "ready setup panel");
});

runTests();

function readySnapshot(): DesktopSnapshot {
  return {
    ...fallbackSnapshot,
    diagnostics: {
      summary: { ok: 1, warnings: 0, failures: 0 },
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

function readyService(): DesktopServiceSession {
  return {
    ...fallbackSnapshot.service,
    status: "started",
    pid: 123,
    token_available: true,
    message: "Local service ready",
  };
}

function stoppedService(): DesktopServiceSession {
  return {
    ...readyService(),
    status: "not_started",
    pid: null,
    token_available: false,
    message: "Local service is not running",
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

function assertEqual(actual: string, expected: string, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

function assertButtonDisabled(
  html: string,
  marker: string,
  expected: boolean,
  message: string,
) {
  const button = buttonMarkup(html, marker);
  const disabled = /\sdisabled(?:\s|>|=)/.test(button);
  if (disabled !== expected) {
    throw new Error(`${message}: expected disabled=${expected}, got ${disabled}`);
  }
}

function buttonMarkup(html: string, marker: string) {
  const markerIndex = html.indexOf(marker);
  if (markerIndex < 0) {
    throw new Error(`expected rendered HTML to include button marker ${marker}`);
  }
  const start = html.lastIndexOf("<button", markerIndex);
  const end = html.indexOf("</button>", markerIndex);
  if (start < 0 || end < 0) {
    throw new Error(`expected marker ${marker} to be inside a button`);
  }
  return html.slice(start, end);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
