import { fallbackSnapshot } from "./fallback-data";
import {
  desktopDiagnosticsSummary,
  diagnosticsSummarySection,
} from "./diagnostics-summary";
import type {
  DesktopDistribution,
  DesktopSnapshot,
  DiagnosticsReport,
} from "./types";

const tests: Array<[string, () => void]> = [];

test("renders browser preview distribution as informational", () => {
  const summary = desktopDiagnosticsSummary(fallbackSnapshot, fallbackSnapshot.service);
  const distribution = diagnostic(summary, "distribution");

  assertEqual(distribution.value, "Browser preview", "browser preview value");
  assertEqual(distribution.tone, "info", "browser preview tone");

  const html = diagnosticsSummarySection(fallbackSnapshot, fallbackSnapshot.service);
  assertExcludes(
    html,
    "data-diagnostics-release-readiness",
    "browser preview release readiness",
  );
});

test("marks preview bundles as needing the public release gate", () => {
  const snapshot = snapshotWithDistribution({
    channel: "preview_bundle",
    app_version: "0.1.1",
    build_profile: "release",
    sidecar_policy: "bundled_cli",
    release_gate: "required",
    signing: "not_checked",
    notarization: "not_checked",
    message: "Preview bundle; run the public release gate before distributing.",
  });
  const summary = desktopDiagnosticsSummary(snapshot, fallbackSnapshot.service);
  const distribution = diagnostic(summary, "distribution");

  assertEqual(distribution.value, "Preview 0.1.1", "preview bundle value");
  assertEqual(distribution.tone, "warn", "preview bundle tone");

  const html = diagnosticsSummarySection(snapshot, fallbackSnapshot.service);
  assertIncludes(
    html,
    "data-diagnostics-release-readiness",
    "preview bundle release readiness",
  );
  assertIncludes(html, "npm run bundle:macos:release", "preview bundle release gate command");
  assertIncludes(
    html,
    "npm run release:macos:status -- --markdown",
    "preview bundle status command",
  );
});

test("renders public release distribution verifier policy", () => {
  const html = diagnosticsSummarySection(
    snapshotWithDistribution({
      channel: "public_release",
      app_version: "0.1.1",
      build_profile: "release",
      sidecar_policy: "bundled_cli",
      release_gate: "selected",
      signing: "developer_id_required",
      notarization: "required",
      message: "Built through the public macOS release gate.",
    }),
    fallbackSnapshot.service,
  );

  assertIncludes(html, "Release 0.1.1", "release value");
  assertIncludes(html, "Developer ID signing", "release verifier detail");
  assertIncludes(html, 'data-diagnostic="distribution"', "distribution card");
  assertIncludes(html, "Verifier proof required", "release readiness summary");
  assertIncludes(
    html,
    "npm run release:macos:status -- --markdown",
    "release status command",
  );
  assertIncludes(html, "npm run verify:macos:release", "release verifier command");
  assertIncludes(html, "macos-desktop-release", "release environment");

  const distribution = diagnostic(
    desktopDiagnosticsSummary(
      snapshotWithDistribution({
        channel: "public_release",
        app_version: "0.1.1",
        build_profile: "release",
        sidecar_policy: "bundled_cli",
        release_gate: "selected",
        signing: "developer_id_required",
        notarization: "required",
        message: "Built through the public macOS release gate.",
      }),
      fallbackSnapshot.service,
    ),
    "distribution",
  );
  assertEqual(distribution.tone, "info", "public release channel is not verifier proof");
});

test("renders pending v3 work from the service contract as informational", () => {
  const summary = desktopDiagnosticsSummary(fallbackSnapshot, fallbackSnapshot.service);
  const integration = diagnostic(summary, "v3-integration");

  assertEqual(integration.value, "4 open items", "pending v3 item count");
  assertEqual(integration.tone, "info", "pending v3 work is informational");
  assertIncludes(
    integration.detail,
    "release-channel artifact acceptance",
    "pending v3 detail",
  );

  const html = diagnosticsSummarySection(fallbackSnapshot, fallbackSnapshot.service);
  assertIncludes(html, 'data-diagnostic="v3-integration"', "v3 integration card");
  assertIncludes(html, "v3 integration", "v3 integration label");
  assertIncludes(
    html,
    "data-diagnostics-pending-runtime-work",
    "pending runtime work list",
  );
  assertIncludes(
    html,
    "data-pending-runtime-work=\"4\"",
    "all pending runtime work items render",
  );
  assertIncludes(
    html,
    "signed privileged helper",
    "privileged helper pending item",
  );
  assertIncludes(
    html,
    "runtime-session checkpoints",
    "runtime checkpoint pending item",
  );
  assertIncludes(html, "native MLX/Core ML adapters", "native adapter pending item");
});

test("marks v3 integration ready when the service contract has no pending work", () => {
  const service = {
    ...fallbackSnapshot.service,
    pending_runtime_work: [],
  };
  const integration = diagnostic(
    desktopDiagnosticsSummary(fallbackSnapshot, service),
    "v3-integration",
  );

  assertEqual(integration.value, "Ready", "empty pending v3 value");
  assertEqual(integration.tone, "good", "empty pending v3 tone");

  const html = diagnosticsSummarySection(fallbackSnapshot, service);
  assertExcludes(
    html,
    "data-diagnostics-pending-runtime-work",
    "empty pending runtime work list",
  );
});

test("renders privileged installer policy from the service contract", () => {
  const summary = desktopDiagnosticsSummary(fallbackSnapshot, fallbackSnapshot.service);
  const installer = diagnostic(summary, "privileged-install");

  assertEqual(installer.value, "External handoff", "privileged install value");
  assertEqual(installer.tone, "info", "external handoff remains informational");
  assertIncludes(
    installer.detail,
    "helper/escalation design, rollback plan",
    "designed privileged gates",
  );

  const html = diagnosticsSummarySection(fallbackSnapshot, fallbackSnapshot.service);
  assertIncludes(html, 'data-diagnostic="privileged-install"', "installer policy card");
  assertIncludes(html, "Installer safety", "installer policy label");
});

test("renders privileged helper and rollback receipt design from the service contract", () => {
  const summary = desktopDiagnosticsSummary(fallbackSnapshot, fallbackSnapshot.service);
  const helper = diagnostic(summary, "privileged-helper");

  assertEqual(helper.value, "Designed", "helper design value");
  assertEqual(helper.tone, "info", "helper design remains informational");
  assertIncludes(helper.detail, "com.apm.pkg-helper", "helper bundle identifier");
  assertIncludes(
    helper.detail,
    "service/privileged-install-receipts.json",
    "rollback receipt store",
  );

  const html = diagnosticsSummarySection(fallbackSnapshot, fallbackSnapshot.service);
  assertIncludes(html, 'data-diagnostic="privileged-helper"', "helper design card");
  assertIncludes(html, "Helper design", "helper design label");
});

test("renders absent privileged helper artifacts from doctor checks", () => {
  const summary = desktopDiagnosticsSummary(fallbackSnapshot, fallbackSnapshot.service);
  const artifacts = diagnostic(summary, "privileged-helper-artifacts");

  assertEqual(artifacts.value, "Absent", "helper artifact value");
  assertEqual(artifacts.tone, "good", "absent helper artifacts are healthy");
  assertIncludes(
    artifacts.detail,
    "no apm privileged helper artifacts installed",
    "helper artifact detail",
  );

  const html = diagnosticsSummarySection(fallbackSnapshot, fallbackSnapshot.service);
  assertIncludes(
    html,
    'data-diagnostic="privileged-helper-artifacts"',
    "helper artifacts card",
  );
  assertIncludes(html, "Helper artifacts", "helper artifacts label");
});

test("warns when privileged helper artifacts exist while execution is disabled", () => {
  const snapshot = snapshotWithDiagnostics({
    summary: {
      ok: fallbackSnapshot.diagnostics.summary.ok,
      warnings: fallbackSnapshot.diagnostics.summary.warnings + 1,
      failures: fallbackSnapshot.diagnostics.summary.failures,
    },
    checks: [
      ...fallbackSnapshot.diagnostics.checks.filter(
        (check) => check.name !== "Privileged helper artifacts",
      ),
      {
        name: "Privileged helper artifacts",
        status: "warning",
        detail:
          "unexpected artifacts while PKG execution is disabled: helper at /Library/PrivilegedHelperTools/com.apm.pkg-helper",
        hint: "Current builds should use external PKG handoff only.",
      },
    ],
  });
  const summary = desktopDiagnosticsSummary(snapshot, fallbackSnapshot.service);
  const artifacts = diagnostic(summary, "privileged-helper-artifacts");

  assertEqual(artifacts.value, "Unexpected", "helper artifact warning value");
  assertEqual(artifacts.tone, "warn", "helper artifact warning tone");
  assertIncludes(artifacts.detail, "PKG execution is disabled", "warning detail");
});

runTests();

function snapshotWithDistribution(
  distribution: DesktopDistribution,
): DesktopSnapshot {
  return {
    ...fallbackSnapshot,
    distribution,
  };
}

function snapshotWithDiagnostics(
  diagnostics: DiagnosticsReport,
): DesktopSnapshot {
  return {
    ...fallbackSnapshot,
    diagnostics,
  };
}

function diagnostic(
  summary: ReturnType<typeof desktopDiagnosticsSummary>,
  key: string,
) {
  const item = summary.items.find((candidate) => candidate.key === key);
  if (!item) {
    throw new Error(`missing diagnostic item: ${key}`);
  }
  return item;
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

function assertExcludes(html: string, expected: string, message: string) {
  if (html.includes(expected)) {
    throw new Error(`${message}: expected rendered HTML to omit ${expected}`);
  }
}

function assertEqual(actual: unknown, expected: unknown, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
