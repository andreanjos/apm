import type {
  AvailableUpdatesResult,
  DiagnosticCheck,
  DiagnosticsReport,
  DesktopDistribution,
  DesktopServiceSession,
  DesktopSnapshot,
  PackageUpdateAction,
  PrivilegedInstallPolicy,
  PrivilegedInstallPrerequisite,
} from "./types";
import { desktopReleaseReadiness, type ReleaseReadinessItem } from "./release-readiness";
import { escapeHtml } from "./view-utils";

export type DiagnosticTone = "good" | "warn" | "bad" | "info";

export type DiagnosticItem = {
  key: string;
  label: string;
  value: string;
  detail: string;
  tone: DiagnosticTone;
};

export type DiagnosticSummary = {
  label: string;
  tone: DiagnosticTone;
  items: DiagnosticItem[];
};

const PRIVILEGED_HELPER_ARTIFACTS_CHECK = "Privileged helper artifacts";

export function desktopDiagnosticsSummary(
  snapshot: DesktopSnapshot,
  service: DesktopServiceSession,
): DiagnosticSummary {
  const items = [
    serviceDiagnostic(service),
    distributionDiagnostic(snapshot.distribution),
    pendingRuntimeWorkDiagnostic(service),
    privilegedInstallDiagnostic(service.privileged_install_policy),
    privilegedHelperDiagnostic(service.privileged_install_policy),
    privilegedHelperArtifactsDiagnostic(
      snapshot.diagnostics,
      service.privileged_install_policy,
    ),
    recoveryDiagnostic(snapshot),
    registryDiagnostic(snapshot),
    catalogDiagnostic(snapshot),
    libraryDiagnostic(snapshot),
    updatesDiagnostic(snapshot.updates),
    doctorDiagnostic(snapshot.diagnostics),
  ];
  return {
    label: summaryLabel(items),
    tone: summaryTone(items),
    items,
  };
}

function pendingRuntimeWorkDiagnostic(service: DesktopServiceSession): DiagnosticItem {
  const pending = service.pending_runtime_work;
  if (pending.length === 0) {
    return {
      key: "v3-integration",
      label: "v3 integration",
      value: "Ready",
      detail: "No pending service contract work",
      tone: "good",
    };
  }

  return {
    key: "v3-integration",
    label: "v3 integration",
    value: plural(pending.length, "open item"),
    detail: pending[0],
    tone: "info",
  };
}

function privilegedInstallDiagnostic(policy: PrivilegedInstallPolicy): DiagnosticItem {
  const missing = policy.prerequisites.filter(
    (prerequisite) => prerequisite.status === "missing",
  );
  const designed = policy.prerequisites.filter(
    (prerequisite) => prerequisite.status === "designed",
  );

  if (policy.runs_pkg_installers && missing.length > 0) {
    return {
      key: "privileged-install",
      label: "Installer safety",
      value: "Blocked",
      detail: privilegedMissingDetail(missing),
      tone: "bad",
    };
  }

  if (policy.runs_pkg_installers) {
    return {
      key: "privileged-install",
      label: "Installer safety",
      value: "Enabled",
      detail: "Privileged PKG execution gates are declared",
      tone: "good",
    };
  }

  return {
    key: "privileged-install",
    label: "Installer safety",
    value: "External handoff",
    detail:
      missing.length > 0
        ? privilegedMissingDetail(missing)
        : designed.length > 0
          ? privilegedDesignedDetail(designed)
        : "apm does not run PKG installers itself",
    tone: "info",
  };
}

function privilegedHelperDiagnostic(policy: PrivilegedInstallPolicy): DiagnosticItem {
  const { helper, rollback } = policy.design;
  const ready =
    helper.status === "designed" &&
    rollback.status === "designed" &&
    helper.requires_authorization &&
    rollback.receipt_required_before_mutation &&
    rollback.preflight_snapshot_required &&
    rollback.uninstall_requires_receipt;

  return {
    key: "privileged-helper",
    label: "Helper design",
    value: ready ? "Designed" : "Incomplete",
    detail: ready
      ? `${helper.bundle_identifier} / receipts ${rollback.receipt_store_relative_path}`
      : "Privileged helper and rollback receipt gates are incomplete",
    tone: policy.runs_pkg_installers && !ready ? "bad" : "info",
  };
}

function privilegedHelperArtifactsDiagnostic(
  report: DiagnosticsReport,
  policy: PrivilegedInstallPolicy,
): DiagnosticItem {
  const check = report.checks.find(
    (candidate) => candidate.name === PRIVILEGED_HELPER_ARTIFACTS_CHECK,
  );

  if (!check) {
    return {
      key: "privileged-helper-artifacts",
      label: "Helper artifacts",
      value: "Unknown",
      detail: "Diagnostics have not returned the helper artifact check yet",
      tone: "info",
    };
  }

  return {
    key: "privileged-helper-artifacts",
    label: "Helper artifacts",
    value: privilegedHelperArtifactsValue(check, policy),
    detail: check.detail,
    tone: privilegedHelperArtifactsTone(check),
  };
}

function privilegedHelperArtifactsValue(
  check: DiagnosticCheck,
  policy: PrivilegedInstallPolicy,
) {
  switch (check.status) {
    case "ok":
      return policy.runs_pkg_installers ? "Ready" : "Absent";
    case "warning":
      return "Unexpected";
    case "failure":
      return "Blocked";
  }
}

function privilegedHelperArtifactsTone(check: DiagnosticCheck): DiagnosticTone {
  switch (check.status) {
    case "ok":
      return "good";
    case "warning":
      return "warn";
    case "failure":
      return "bad";
  }
}

function privilegedMissingDetail(missing: PrivilegedInstallPrerequisite[]) {
  return `${plural(missing.length, "missing gate")}: ${missing
    .map((prerequisite) => privilegedPrerequisiteLabel(prerequisite.id))
    .join(", ")}`;
}

function privilegedDesignedDetail(designed: PrivilegedInstallPrerequisite[]) {
  return `${plural(designed.length, "designed gate")}: ${designed
    .map((prerequisite) => privilegedPrerequisiteLabel(prerequisite.id))
    .join(", ")}`;
}

function privilegedPrerequisiteLabel(id: PrivilegedInstallPrerequisite["id"]) {
  switch (id) {
    case "helper_or_escalation_design":
      return "helper/escalation design";
    case "explicit_user_consent":
      return "explicit consent";
    case "package_verification":
      return "package verification";
    case "audit_trail":
      return "audit trail";
    case "rollback_plan":
      return "rollback plan";
  }
}

export function diagnosticsSummarySection(
  snapshot: DesktopSnapshot,
  service: DesktopServiceSession,
) {
  const summary = desktopDiagnosticsSummary(snapshot, service);
  return `
    <section class="panel diagnostics-panel">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Diagnostics</p>
          <h2>System readiness</h2>
        </div>
        <span class="diagnostics-status ${summary.tone}">${escapeHtml(summary.label)}</span>
      </div>
      <div class="diagnostics-grid">
        ${summary.items.map(diagnosticCard).join("")}
      </div>
      ${releaseReadinessMarkup(snapshot.distribution)}
      ${pendingRuntimeWorkMarkup(service.pending_runtime_work)}
      ${doctorChecksMarkup(snapshot.diagnostics)}
    </section>
  `;
}

function diagnosticCard(item: DiagnosticItem) {
  return `
    <article class="diagnostic-item ${item.tone}" data-diagnostic="${escapeHtml(item.key)}">
      <div class="diagnostic-marker" aria-hidden="true"></div>
      <div class="diagnostic-body">
        <span>${escapeHtml(item.label)}</span>
        <strong>${escapeHtml(item.value)}</strong>
        <small>${escapeHtml(item.detail)}</small>
      </div>
    </article>
  `;
}

function serviceDiagnostic(service: DesktopServiceSession): DiagnosticItem {
  if ((service.status === "started" || service.status === "reused") && service.token_available) {
    return {
      key: "service",
      label: "Local service",
      value: service.status === "started" ? "Started" : "Reused",
      detail: service.pid
        ? `pid ${service.pid} / ${service.api_version}`
        : `${service.url} / ${service.api_version}`,
      tone: "good",
    };
  }

  if (service.status === "started" || service.status === "reused") {
    return {
      key: "service",
      label: "Local service",
      value: "Token missing",
      detail: service.token_file || "Protected service routes are unavailable",
      tone: "bad",
    };
  }

  if (service.status === "preview") {
    return {
      key: "service",
      label: "Local service",
      value: "Preview",
      detail: "Browser preview uses sample data",
      tone: "info",
    };
  }

  return {
    key: "service",
    label: "Local service",
    value: service.status === "not_started" ? "Stopped" : "Unavailable",
    detail: service.message,
    tone: service.status === "not_started" ? "warn" : "bad",
  };
}

function distributionDiagnostic(distribution: DesktopDistribution): DiagnosticItem {
  switch (distribution.channel) {
    case "browser_preview":
      return {
        key: "distribution",
        label: "Distribution",
        value: "Browser preview",
        detail: distribution.message,
        tone: "info",
      };
    case "development":
      return {
        key: "distribution",
        label: "Distribution",
        value: `Dev ${distribution.app_version}`,
        detail: distribution.message,
        tone: "info",
      };
    case "preview_bundle":
      return {
        key: "distribution",
        label: "Distribution",
        value: `Preview ${distribution.app_version}`,
        detail: distribution.message,
        tone: "warn",
      };
    case "public_release":
      return {
        key: "distribution",
        label: "Distribution",
        value: `Release ${distribution.app_version}`,
        detail: releaseGateDetail(distribution),
        tone: "info",
      };
  }
}

function releaseGateDetail(distribution: DesktopDistribution) {
  return distribution.release_gate === "selected"
    ? "Developer ID signing and notarization required by release verifier"
    : distribution.message;
}

function releaseReadinessMarkup(distribution: DesktopDistribution) {
  const readiness = desktopReleaseReadiness(distribution);
  if (!readiness) {
    return "";
  }

  return `
    <div class="doctor-check-list" data-diagnostics-release-readiness>
      ${[readiness.summary, ...readiness.checks].map(releaseReadinessItem).join("")}
    </div>
  `;
}

function releaseReadinessItem(item: ReleaseReadinessItem) {
  return `
    <article class="doctor-check-item ${item.tone}" data-release-check="${escapeHtml(item.key)}">
      <span>${escapeHtml(item.label)}</span>
      <strong>${escapeHtml(item.value)}</strong>
      <small>${escapeHtml(item.detail)}</small>
    </article>
  `;
}

function pendingRuntimeWorkMarkup(pendingWork: string[]) {
  if (pendingWork.length === 0) {
    return "";
  }

  return `
    <div class="doctor-check-list" data-diagnostics-pending-runtime-work>
      ${pendingWork.map(pendingRuntimeWorkItem).join("")}
    </div>
  `;
}

function pendingRuntimeWorkItem(item: string, index: number) {
  return `
    <article class="doctor-check-item info" data-pending-runtime-work="${index + 1}">
      <span>v3 integration</span>
      <strong>${escapeHtml(`Open item ${index + 1}`)}</strong>
      <small>${escapeHtml(item)}</small>
    </article>
  `;
}

function recoveryDiagnostic(snapshot: DesktopSnapshot): DiagnosticItem {
  const { interrupted_count, retryable_count } = snapshot.recovery;
  if (retryable_count > 0) {
    return {
      key: "recovery",
      label: "Recovery",
      value: plural(retryable_count, "retryable operation"),
      detail: "Interrupted operation can be retried from recent history",
      tone: "warn",
    };
  }

  if (interrupted_count > 0) {
    return {
      key: "recovery",
      label: "Recovery",
      value: "Manual review",
      detail: "Interrupted operation has no saved retry metadata",
      tone: "warn",
    };
  }

  return {
    key: "recovery",
    label: "Recovery",
    value: "Clear",
    detail: "No interrupted operations need attention",
    tone: "good",
  };
}

function registryDiagnostic(snapshot: DesktopSnapshot): DiagnosticItem {
  if (snapshot.source_count === 0) {
    return {
      key: "registry",
      label: "Registry",
      value: "No sources",
      detail: "Add or sync a registry before browsing packages",
      tone: "warn",
    };
  }

  return {
    key: "registry",
    label: "Registry",
    value: plural(snapshot.source_count, "source"),
    detail: "Configured package source count",
    tone: "good",
  };
}

function catalogDiagnostic(snapshot: DesktopSnapshot): DiagnosticItem {
  if (snapshot.catalog.status === "catalog_empty") {
    return {
      key: "catalog",
      label: "Catalog",
      value: "Empty",
      detail: "Sync registries to populate package metadata",
      tone: "warn",
    };
  }

  if (snapshot.catalog.total_matches === 0) {
    return {
      key: "catalog",
      label: "Catalog",
      value: "No packages",
      detail: "No package metadata matched the current catalog query",
      tone: "warn",
    };
  }

  return {
    key: "catalog",
    label: "Catalog",
    value: plural(snapshot.catalog.total_matches, "package"),
    detail: "Searchable package records loaded",
    tone: "good",
  };
}

function libraryDiagnostic(snapshot: DesktopSnapshot): DiagnosticItem {
  if (snapshot.installed.length === 0) {
    return {
      key: "library",
      label: "Library",
      value: "No installs",
      detail: "No packages are tracked on this machine yet",
      tone: "info",
    };
  }

  const apmManaged = snapshot.installed.filter((item) => item.origin === "apm").length;
  const external = snapshot.installed.length - apmManaged;
  return {
    key: "library",
    label: "Library",
    value: plural(snapshot.installed.length, "package"),
    detail: `${plural(apmManaged, "apm-managed install")} / ${plural(external, "external install")}`,
    tone: "good",
  };
}

function updatesDiagnostic(updates: AvailableUpdatesResult): DiagnosticItem {
  if (updates.status === "catalog_empty") {
    return {
      key: "updates",
      label: "Updates",
      value: "Unknown",
      detail: "Update checks need catalog metadata",
      tone: "warn",
    };
  }

  const installable = updateActionCount(updates.updates, "installable");
  const pinned = updateActionCount(updates.updates, "pinned");
  const external = updateActionCount(updates.updates, "external");

  if (installable > 0 || updates.missing_count > 0) {
    const actionCount = installable + updates.missing_count;
    return {
      key: "updates",
      label: "Updates",
      value: `${actionCount.toLocaleString()} pending`,
      detail: updateDetail(installable, pinned, external, updates.missing_count),
      tone: "warn",
    };
  }

  if (pinned > 0 || external > 0) {
    return {
      key: "updates",
      label: "Updates",
      value: "Deferred",
      detail: updateDetail(installable, pinned, external, updates.missing_count),
      tone: "info",
    };
  }

  return {
    key: "updates",
    label: "Updates",
    value: "Current",
    detail: `${plural(updates.up_to_date_count, "package")} up to date`,
    tone: "good",
  };
}

function doctorDiagnostic(report: DiagnosticsReport): DiagnosticItem {
  const { failures, warnings, ok } = report.summary;

  if (failures > 0) {
    return {
      key: "doctor",
      label: "Doctor",
      value: plural(failures, "failure"),
      detail: doctorSummaryDetail(report.summary),
      tone: "bad",
    };
  }

  if (warnings > 0) {
    return {
      key: "doctor",
      label: "Doctor",
      value: plural(warnings, "warning"),
      detail: doctorSummaryDetail(report.summary),
      tone: "warn",
    };
  }

  if (ok === 0) {
    return {
      key: "doctor",
      label: "Doctor",
      value: "No checks",
      detail: "Diagnostics have not returned checks yet",
      tone: "info",
    };
  }

  return {
    key: "doctor",
    label: "Doctor",
    value: "Passed",
    detail: doctorSummaryDetail(report.summary),
    tone: "good",
  };
}

function doctorChecksMarkup(report: DiagnosticsReport) {
  const problemChecks = report.checks
    .filter((check) => check.status !== "ok")
    .slice(0, 4);

  if (problemChecks.length === 0) {
    return `
      <div class="doctor-check-list">
        <article class="doctor-check-item ok">
          <span>Doctor checks</span>
          <strong>All checks passed</strong>
          <small>${escapeHtml(doctorSummaryDetail(report.summary))}</small>
        </article>
      </div>
    `;
  }

  return `
    <div class="doctor-check-list">
      ${problemChecks.map(doctorCheckItem).join("")}
    </div>
  `;
}

function doctorCheckItem(check: DiagnosticCheck) {
  return `
    <article class="doctor-check-item ${doctorCheckTone(check)}">
      <span>${escapeHtml(check.name)}</span>
      <strong>${escapeHtml(check.detail)}</strong>
      ${check.hint ? `<small>${escapeHtml(check.hint)}</small>` : ""}
    </article>
  `;
}

function doctorCheckTone(check: DiagnosticCheck) {
  switch (check.status) {
    case "ok":
      return "ok";
    case "warning":
      return "warn";
    case "failure":
      return "bad";
  }
}

function doctorSummaryDetail(summary: DiagnosticsReport["summary"]) {
  return [
    plural(summary.ok, "ok check"),
    plural(summary.warnings, "warning"),
    plural(summary.failures, "failure"),
  ].join(" / ");
}

function summaryLabel(items: DiagnosticItem[]) {
  const bad = items.filter((item) => item.tone === "bad").length;
  const warn = items.filter((item) => item.tone === "warn").length;

  if (bad > 0) {
    return plural(bad, "blocker");
  }
  if (warn > 0) {
    return plural(warn, "attention item");
  }
  return "Ready";
}

function summaryTone(items: DiagnosticItem[]): DiagnosticTone {
  if (items.some((item) => item.tone === "bad")) {
    return "bad";
  }
  if (items.some((item) => item.tone === "warn")) {
    return "warn";
  }
  return "good";
}

function updateActionCount(
  updates: Array<{ action: PackageUpdateAction }>,
  action: PackageUpdateAction,
) {
  return updates.filter((update) => update.action === action).length;
}

function updateDetail(
  installable: number,
  pinned: number,
  external: number,
  missing: number,
) {
  const parts = [
    updateDetailPart(installable, "installable update"),
    updateDetailPart(pinned, "pinned update"),
    updateDetailPart(external, "external update"),
    updateDetailPart(missing, "missing package"),
  ].filter((part) => part.length > 0);
  return parts.length > 0 ? parts.join(" / ") : "No deferred updates";
}

function updateDetailPart(count: number, singular: string) {
  return count > 0 ? plural(count, singular) : "";
}

function plural(count: number, singular: string) {
  return `${count.toLocaleString()} ${singular}${count === 1 ? "" : "s"}`;
}
