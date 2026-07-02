export const expectedContractSchema = "2026-07-01-operation-controls";

export function verifySidecarContract(stdout) {
  let contract;
  try {
    contract = JSON.parse(stdout);
  } catch (error) {
    return [`bundled apm-cli emitted invalid service contract JSON: ${error}`];
  }

  const errors = [];
  expectContractValue(errors, contract, "schema_version", expectedContractSchema);
  expectContractValue(errors, contract, "service_name", "apm local service");
  expectContractValue(errors, contract, "api_version", "v1alpha1");
  expectContractValue(errors, contract?.bind, "host", "127.0.0.1");
  expectContractValue(errors, contract?.bind, "port_env", "APM_SERVE_PORT");

  if (contract?.security?.localhost_only !== true) {
    errors.push("service contract must require localhost-only binding");
  }
  if (!String(contract?.security?.auth ?? "").includes("x-apm-token")) {
    errors.push("service contract auth policy must reference x-apm-token");
  }
  expectPrivilegedInstallPolicy(errors, contract?.security?.privileged_install_policy);
  if (contract?.operation_recovery_policy?.automatic_resume !== "disabled") {
    errors.push("service contract automatic recovery policy must remain disabled");
  }
  if (contract?.operation_recovery_policy?.retry_all_ready_recovery_candidates !== true) {
    errors.push("service contract must expose retry-all-ready recovery support");
  }
  expectOperationControlPolicy(errors, contract?.operation_control_policy);
  expectPendingRuntimeWork(errors, contract);

  const endpoints = arrayField(contract, "endpoints", errors);
  const endpointsById = new Map(endpoints.map((endpoint) => [endpoint.id, endpoint]));
  for (const expected of requiredContractEndpoints()) {
    const endpoint = endpointsById.get(expected.id);
    if (!endpoint) {
      errors.push(`service contract missing endpoint ${expected.id}`);
      continue;
    }
    if (endpoint.method !== expected.method || endpoint.path !== expected.path) {
      errors.push(
        `service contract endpoint ${expected.id} must be ${expected.method} ${expected.path}; got ${endpoint.method ?? "missing"} ${endpoint.path ?? "missing"}`,
      );
    }
  }

  const eventStreams = arrayField(contract, "event_streams", errors);
  const operationEvents = eventStreams.find(
    (stream) => stream.id === "operation.events",
  );
  if (!operationEvents) {
    errors.push("service contract missing operation.events stream");
  } else {
    const events = arrayField(operationEvents, "events", errors);
    for (const eventName of ["ScanEvent", "ModelOperationEvent"]) {
      if (!events.includes(eventName)) {
        errors.push(`operation.events stream must include ${eventName}`);
      }
    }
    const eventNames = arrayField(operationEvents, "event_names", errors);
    for (const eventName of requiredOperationEventNames()) {
      if (!eventNames.includes(eventName)) {
        errors.push(`operation.events stream must include concrete event ${eventName}`);
      }
    }
  }

  return errors.map((error) => `bundled apm-cli contract check failed: ${error}`);
}

function expectPendingRuntimeWork(errors, contract) {
  const pendingWork = arrayField(contract, "pending_runtime_work", errors);
  for (const topic of requiredPendingRuntimeWorkTopics()) {
    if (!pendingWork.some((item) => typeof item === "string" && item.includes(topic))) {
      errors.push(`service contract pending_runtime_work must mention ${topic}`);
    }
  }
}

function requiredPendingRuntimeWorkTopics() {
  return [
    "release-channel artifact acceptance",
    "signed privileged helper",
    "runtime-session checkpoints",
    "native MLX/Core ML",
  ];
}

function expectOperationControlPolicy(errors, policy) {
  expectContractValue(errors, policy, "cancel_endpoint_id", "operation.cancel");
  expectContractValue(errors, policy, "retry_endpoint_id", "operation.retry");
  expectContractValue(
    errors,
    policy,
    "recovery_retry_endpoint_id",
    "operation.recovery.retry",
  );
  expectContractValue(errors, policy, "progress_event_stream_id", "operation.events");
  if (policy?.queued_cancellation !== true) {
    errors.push("service contract must expose queued operation cancellation");
  }
  expectContractValue(
    errors,
    policy,
    "running_cancellation_state",
    "cancel_requested",
  );
  const cancellationKinds = arrayField(policy, "cooperative_cancellation_kinds", errors);
  const progressKinds = arrayField(policy, "progress_event_kinds", errors);
  for (const kind of requiredOperationKinds()) {
    if (!cancellationKinds.includes(kind)) {
      errors.push(`operation control policy must include ${kind} cancellation`);
    }
    if (!progressKinds.includes(kind)) {
      errors.push(`operation control policy must include ${kind} progress events`);
    }
  }
}

function requiredOperationKinds() {
  return [
    "registry_sync",
    "library_scan",
    "install_url",
    "install_archive",
    "package_update",
    "package_remove",
    "model_weight_pull",
    "model_install",
    "model_run",
  ];
}

export function requiredOperationEventNames() {
  return [
    "scan_started",
    "scan_finished",
    "registry_sync_started",
    "registry_source_sync_started",
    "registry_source_sync_finished",
    "registry_source_sync_failed",
    "registry_sync_finished",
    "install_started",
    "install_format_started",
    "install_download_started",
    "install_download_progress",
    "install_download_finished",
    "install_archive_install_started",
    "install_archive_verified",
    "install_quarantine_removal_started",
    "install_format_placed",
    "install_state_recording_started",
    "install_state_recorded",
    "install_rolled_back",
    "install_finished",
    "install_failed",
    "remove_started",
    "remove_format_removed",
    "remove_format_missing",
    "remove_state_recorded",
    "remove_finished",
    "remove_failed",
    "model_weight_pull_started",
    "model_weight_pull_progress",
    "model_weight_pull_finished",
    "model_weight_pull_failed",
    "model_install_started",
    "model_install_finished",
    "model_install_failed",
    "model_run_started",
    "model_run_completed",
    "model_run_blocked",
    "model_run_failed",
  ];
}

export function requiredContractEndpoints() {
  return [
    endpoint("health", "GET", "/v1/health"),
    endpoint("service.contract", "GET", "/v1/service/contract"),
    endpoint("diagnostics.report", "GET", "/v1/diagnostics"),
    endpoint("catalog.search", "GET", "/v1/packages"),
    endpoint("library.list", "GET", "/v1/library"),
    endpoint("library.updates", "GET", "/v1/library/updates"),
    endpoint("library.scan", "POST", "/v1/library/scan"),
    endpoint("registry.sync", "POST", "/v1/registry/sync"),
    endpoint("install.plan", "POST", "/v1/install/plan"),
    endpoint("install.handoff", "POST", "/v1/install/handoff"),
    endpoint("install.url", "POST", "/v1/install/url"),
    endpoint("install.archive", "POST", "/v1/install/archive"),
    endpoint("package.pin", "POST", "/v1/packages/{slug}/pin"),
    endpoint("package.update", "POST", "/v1/packages/{slug}/update"),
    endpoint("package.remove", "POST", "/v1/packages/{slug}/remove"),
    endpoint("model.list", "GET", "/v1/models"),
    endpoint("model.catalog", "GET", "/v1/models/catalog"),
    endpoint("model.catalog.cache", "POST", "/v1/models/catalog/{name}/{version}/cache"),
    endpoint("model.store", "GET", "/v1/models/store"),
    endpoint("model.store.init", "POST", "/v1/models/store/init"),
    endpoint("model.manifest.validate", "POST", "/v1/models/manifest/validate"),
    endpoint("model.manifest.cache", "POST", "/v1/models/manifest/cache"),
    endpoint("model.weights.pull", "POST", "/v1/models/{name}/{version}/weights/pull"),
    endpoint("model.install", "POST", "/v1/models/{name}/{version}/install"),
    endpoint("model.run.plan", "POST", "/v1/models/{name}/{version}/run/plan"),
    endpoint("model.run", "POST", "/v1/models/{name}/{version}/run"),
    endpoint("model.chain.plan", "POST", "/v1/models/chains/plan"),
    endpoint("model.remove", "DELETE", "/v1/models/{name}/{version}"),
    endpoint("operation.history", "GET", "/v1/operations"),
    endpoint("operation.recovery", "GET", "/v1/operations/recovery"),
    endpoint("operation.recovery.retry", "POST", "/v1/operations/recovery/retry"),
    endpoint("operation.status", "GET", "/v1/operations/{operation_id}"),
    endpoint("operation.cancel", "POST", "/v1/operations/{operation_id}/cancel"),
    endpoint("operation.retry", "POST", "/v1/operations/{operation_id}/retry"),
  ];
}

function endpoint(id, method, path) {
  return { id, method, path };
}

function expectContractValue(errors, object, key, expected) {
  if (object?.[key] !== expected) {
    errors.push(`service contract ${key} must be ${expected}; got ${object?.[key] ?? "missing"}`);
  }
}

function arrayField(object, key, errors) {
  const value = object?.[key];
  if (Array.isArray(value)) {
    return value;
  }
  errors.push(`service contract ${key} must be an array`);
  return [];
}

function expectPrivilegedInstallPolicy(errors, privilegedPolicy) {
  if (privilegedPolicy?.runs_pkg_installers !== false) {
    errors.push("service contract must keep PKG installer execution disabled");
  }
  expectContractValue(
    errors,
    privilegedPolicy?.design,
    "helper_strategy",
    "signed_helper_deferred",
  );
  expectPrivilegedHelperDesign(errors, privilegedPolicy?.design?.helper);
  expectContractValue(
    errors,
    privilegedPolicy?.design,
    "rollback_strategy",
    "receipt_backed_uninstall_deferred",
  );
  expectPrivilegedRollbackDesign(errors, privilegedPolicy?.design?.rollback);
  if (!String(privilegedPolicy?.design?.execution_gate ?? "").includes("runs_pkg_installers false")) {
    errors.push("service contract privileged install design must keep PKG execution gated");
  }

  const privilegedPrerequisites = arrayField(privilegedPolicy, "prerequisites", errors);
  expectPrivilegedPrerequisite(
    errors,
    privilegedPrerequisites,
    "helper_or_escalation_design",
    "designed",
  );
  expectPrivilegedPrerequisite(
    errors,
    privilegedPrerequisites,
    "rollback_plan",
    "designed",
  );
  expectPrivilegedPrerequisite(
    errors,
    privilegedPrerequisites,
    "explicit_user_consent",
    "required",
  );
  expectPrivilegedPrerequisite(
    errors,
    privilegedPrerequisites,
    "package_verification",
    "required",
  );
  expectPrivilegedPrerequisite(
    errors,
    privilegedPrerequisites,
    "audit_trail",
    "required",
  );
}

function expectPrivilegedHelperDesign(errors, helper) {
  expectContractValue(errors, helper, "status", "designed");
  expectContractValue(errors, helper, "bundle_identifier", "com.apm.pkg-helper");
  expectContractValue(errors, helper, "mach_service_name", "com.apm.pkg-helper");
  expectContractValue(
    errors,
    helper,
    "install_path",
    "/Library/PrivilegedHelperTools/com.apm.pkg-helper",
  );
  expectContractValue(
    errors,
    helper,
    "launchd_plist_path",
    "/Library/LaunchDaemons/com.apm.pkg-helper.plist",
  );
  expectContractValue(
    errors,
    helper,
    "required_signing_identity",
    "Developer ID Application",
  );
  if (helper?.requires_authorization !== true) {
    errors.push("service contract privileged helper must require authorization");
  }
}

function expectPrivilegedRollbackDesign(errors, rollback) {
  expectContractValue(errors, rollback, "status", "designed");
  expectContractValue(
    errors,
    rollback,
    "receipt_store_relative_path",
    "service/privileged-install-receipts.json",
  );
  if (rollback?.receipt_required_before_mutation !== true) {
    errors.push("service contract rollback design must require receipts before mutation");
  }
  if (rollback?.preflight_snapshot_required !== true) {
    errors.push("service contract rollback design must require a preflight snapshot");
  }
  if (rollback?.uninstall_requires_receipt !== true) {
    errors.push("service contract rollback design must require receipts for uninstall");
  }
}

function expectPrivilegedPrerequisite(errors, prerequisites, id, status) {
  const prerequisite = prerequisites.find((candidate) => candidate?.id === id);
  if (!prerequisite) {
    errors.push(`service contract privileged install policy missing prerequisite ${id}`);
    return;
  }
  if (prerequisite.status !== status) {
    errors.push(
      `service contract privileged install prerequisite ${id} must be ${status}; got ${prerequisite.status ?? "missing"}`,
    );
  }
  if (typeof prerequisite.message !== "string" || prerequisite.message.trim().length === 0) {
    errors.push(`service contract privileged install prerequisite ${id} must include a message`);
  }
}
