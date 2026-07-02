import type { DesktopServiceSession } from "./types";

const previewPrivilegedInstallPolicy = {
  execution: "external_handoff_only",
  handoff_kind: "privileged_installer",
  requires_user_confirmation: true,
  runs_pkg_installers: false,
  design: {
    helper_strategy: "signed_helper_deferred",
    helper: {
      status: "designed",
      label: "apm privileged install helper",
      bundle_identifier: "com.apm.pkg-helper",
      mach_service_name: "com.apm.pkg-helper",
      install_path: "/Library/PrivilegedHelperTools/com.apm.pkg-helper",
      launchd_plist_path: "/Library/LaunchDaemons/com.apm.pkg-helper.plist",
      required_signing_identity: "Developer ID Application",
      requires_authorization: true,
    },
    rollback_strategy: "receipt_backed_uninstall_deferred",
    rollback: {
      status: "designed",
      receipt_store_relative_path: "service/privileged-install-receipts.json",
      receipt_required_before_mutation: true,
      preflight_snapshot_required: true,
      uninstall_requires_receipt: true,
      message:
        "Before helper-run PKG execution can mutate disk, apm must persist a package receipt and preflight snapshot so failed installs and explicit uninstalls have a rollback target.",
    },
    execution_gate:
      "Keep runs_pkg_installers false until a signed helper, explicit consent, package verification, audit trail, and receipt-backed rollback are implemented.",
  },
  prerequisites: [
    {
      id: "helper_or_escalation_design",
      status: "designed",
      message:
        "Use a signed privileged helper as the future PKG execution boundary; keep current builds on external handoff until that helper is implemented and reviewed.",
    },
    {
      id: "explicit_user_consent",
      status: "required",
      message:
        "Require an explicit per-install confirmation before any privileged installer execution.",
    },
    {
      id: "package_verification",
      status: "required",
      message:
        "Verify the downloaded package against registry metadata before privileged execution.",
    },
    {
      id: "audit_trail",
      status: "required",
      message:
        "Record the requested package, source, checksum, and privileged action outcome in operation history.",
    },
    {
      id: "rollback_plan",
      status: "designed",
      message:
        "Record helper-installed package receipts before enabling execution so failed installs and explicit uninstalls have a rollback target.",
    },
  ],
  message:
    "PKG packages are exposed only as explicit external handoffs; apm opens the vendor target after confirmation and does not run installer packages itself.",
} satisfies DesktopServiceSession["privileged_install_policy"];

const previewPendingRuntimeWork = [
  "configure macos-desktop-release signing/notarization secrets, run the manual desktop workflow, and complete release-channel artifact acceptance",
  "implement the signed privileged helper and receipt-backed rollback path before enabling apm-run PKG installers",
  "extend the current model-run cancellation/progress checkpoints into executable native MLX/Core ML and managed Python runtime-session checkpoints",
  "turn blocked model run operations into executable native MLX/Core ML adapters and managed Python runtime sessions",
] satisfies DesktopServiceSession["pending_runtime_work"];

export const previewServiceSession: DesktopServiceSession = {
  status: "preview",
  url: "http://127.0.0.1:4767",
  pid: null,
  api_version: "v1alpha1",
  schema_version: "2026-07-01-operation-controls",
  token_header: "x-apm-token",
  token_file: "",
  token_available: false,
  privileged_install_policy: previewPrivilegedInstallPolicy,
  pending_runtime_work: previewPendingRuntimeWork,
  message: "Browser preview uses local sample data",
};

export function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function startingServiceSession(
  current: DesktopServiceSession,
): DesktopServiceSession {
  return {
    ...current,
    status: isTauriRuntime() ? "unavailable" : "preview",
    message: isTauriRuntime() ? "Starting local service" : previewServiceSession.message,
  };
}

export function unavailableServiceSession(
  current: DesktopServiceSession,
  message: string,
): DesktopServiceSession {
  return {
    status: "unavailable",
    url: current.url || previewServiceSession.url,
    pid: null,
    api_version: current.api_version || previewServiceSession.api_version,
    schema_version: current.schema_version || previewServiceSession.schema_version,
    token_header: current.token_header || previewServiceSession.token_header,
    token_file: current.token_file,
    token_available: false,
    privileged_install_policy:
      current.privileged_install_policy || previewServiceSession.privileged_install_policy,
    pending_runtime_work:
      current.pending_runtime_work || previewServiceSession.pending_runtime_work,
    message,
  };
}

export async function ensureLocalServiceSession(): Promise<DesktopServiceSession> {
  if (!isTauriRuntime()) {
    return previewServiceSession;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DesktopServiceSession>("ensure_local_service");
}
