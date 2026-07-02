import {
  expectedContractSchema,
  requiredContractEndpoints,
  requiredOperationEventNames,
  verifySidecarContract,
} from "./macos-contract-check.mjs";

const tests = [];

test("accepts compatible desktop sidecar contracts", () => {
  assertDeepEqual(verifySidecarContract(JSON.stringify(contract())), [], "compatible contract");
});

test("rejects stale contract schemas", () => {
  const errors = verifySidecarContract(
    JSON.stringify(contract({ schema_version: "2026-07-01-old" })),
  );

  assertIncludes(errors.join("\n"), "schema_version", "schema error");
  assertIncludes(errors.join("\n"), expectedContractSchema, "expected schema");
});

test("rejects missing model chain planning endpoint", () => {
  const fixture = contract();
  fixture.endpoints = fixture.endpoints.filter((endpoint) => endpoint.id !== "model.chain.plan");

  const errors = verifySidecarContract(JSON.stringify(fixture));

  assertIncludes(errors.join("\n"), "model.chain.plan", "missing chain endpoint");
});

test("rejects missing library scan endpoint and stream event", () => {
  const fixture = contract();
  fixture.endpoints = fixture.endpoints.filter((endpoint) => endpoint.id !== "library.scan");
  fixture.event_streams[0].events = fixture.event_streams[0].events.filter(
    (event) => event !== "ScanEvent",
  );
  fixture.event_streams[0].event_names = fixture.event_streams[0].event_names.filter(
    (event) => event !== "model_run_completed",
  );

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "library.scan", "missing scan endpoint");
  assertIncludes(errors, "ScanEvent", "missing scan event");
  assertIncludes(errors, "model_run_completed", "missing concrete event");
});

test("rejects changed endpoint methods and paths", () => {
  const fixture = contract();
  const endpoint = fixture.endpoints.find((candidate) => candidate.id === "model.chain.plan");
  endpoint.path = "/v1/models/chain-plan";

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "POST /v1/models/chains/plan", "expected endpoint shape");
  assertIncludes(errors, "POST /v1/models/chain-plan", "actual endpoint shape");
});

test("rejects weakened operation control policy", () => {
  const fixture = contract();
  fixture.operation_control_policy.cancel_endpoint_id = "operation.stop";
  fixture.operation_control_policy.running_cancellation_state = "running";
  fixture.operation_control_policy.progress_event_kinds =
    fixture.operation_control_policy.progress_event_kinds.filter(
      (kind) => kind !== "model_run",
    );

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "cancel_endpoint_id", "cancel endpoint");
  assertIncludes(errors, "running_cancellation_state", "running cancel state");
  assertIncludes(errors, "model_run progress", "model run progress kind");
});

test("rejects missing pending runtime work coverage", () => {
  const fixture = contract({
    pending_runtime_work: [
      "configure macos-desktop-release signing/notarization secrets",
    ],
  });

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "release-channel artifact acceptance", "release gap topic");
  assertIncludes(errors, "signed privileged helper", "helper gap topic");
  assertIncludes(errors, "runtime-session checkpoints", "checkpoint gap topic");
  assertIncludes(errors, "native MLX/Core ML", "runtime gap topic");
});

test("rejects malformed endpoint, event stream, and pending-work arrays", () => {
  const fixture = contract({
    endpoints: null,
    event_streams: null,
    pending_runtime_work: null,
  });

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "endpoints must be an array", "endpoint array error");
  assertIncludes(errors, "event_streams must be an array", "event stream array error");
  assertIncludes(
    errors,
    "pending_runtime_work must be an array",
    "pending runtime work array error",
  );
});

test("rejects weakened privileged install prerequisite policy", () => {
  const fixture = contract();
  fixture.security.privileged_install_policy.design.helper_strategy = "ad_hoc_escalation";
  fixture.security.privileged_install_policy.prerequisites =
    fixture.security.privileged_install_policy.prerequisites.filter(
      (prerequisite) => prerequisite.id !== "rollback_plan",
    );
  fixture.security.privileged_install_policy.prerequisites.find(
    (prerequisite) => prerequisite.id === "audit_trail",
  ).status = "missing";

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "helper_strategy", "weakened helper design");
  assertIncludes(errors, "rollback_plan", "missing rollback prerequisite");
  assertIncludes(errors, "audit_trail must be required", "weakened audit prerequisite");
});

test("rejects weakened privileged helper and rollback design", () => {
  const fixture = contract();
  fixture.security.privileged_install_policy.design.helper.bundle_identifier = "com.example.helper";
  fixture.security.privileged_install_policy.design.helper.requires_authorization = false;
  fixture.security.privileged_install_policy.design.rollback.receipt_required_before_mutation = false;
  fixture.security.privileged_install_policy.design.rollback.preflight_snapshot_required = false;

  const errors = verifySidecarContract(JSON.stringify(fixture)).join("\n");

  assertIncludes(errors, "bundle_identifier", "helper bundle identifier");
  assertIncludes(errors, "must require authorization", "authorization gate");
  assertIncludes(errors, "receipts before mutation", "receipt mutation gate");
  assertIncludes(errors, "preflight snapshot", "preflight snapshot gate");
});

test("rejects invalid JSON", () => {
  const errors = verifySidecarContract("{not json");

  assertIncludes(errors.join("\n"), "invalid service contract JSON", "invalid JSON error");
});

runTests();

function contract(overrides = {}) {
  return {
    schema_version: expectedContractSchema,
    service_name: "apm local service",
    api_version: "v1alpha1",
    bind: {
      host: "127.0.0.1",
      port_env: "APM_SERVE_PORT",
    },
    security: {
      localhost_only: true,
      auth: "required for non-public endpoints via the x-apm-token header",
      privileged_install_policy: {
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
              "Before helper-run PKG execution can mutate disk, apm must persist receipts.",
          },
          execution_gate: "Keep runs_pkg_installers false until helper support ships.",
        },
        prerequisites: [
          prerequisite("helper_or_escalation_design", "designed"),
          prerequisite("explicit_user_consent", "required"),
          prerequisite("package_verification", "required"),
          prerequisite("audit_trail", "required"),
          prerequisite("rollback_plan", "designed"),
        ],
      },
    },
    operation_recovery_policy: {
      automatic_resume: "disabled",
      retry_all_ready_recovery_candidates: true,
    },
    operation_control_policy: {
      cancel_endpoint_id: "operation.cancel",
      retry_endpoint_id: "operation.retry",
      recovery_retry_endpoint_id: "operation.recovery.retry",
      progress_event_stream_id: "operation.events",
      queued_cancellation: true,
      running_cancellation_state: "cancel_requested",
      cooperative_cancellation_kinds: operationKinds(),
      progress_event_kinds: operationKinds(),
      message: "Operation controls are available.",
    },
    pending_runtime_work: [
      "configure macos-desktop-release signing/notarization secrets, run the manual desktop workflow, and complete release-channel artifact acceptance",
      "implement the signed privileged helper and receipt-backed rollback path before enabling apm-run PKG installers",
      "extend the current model-run cancellation/progress checkpoints into executable native MLX/Core ML and managed Python runtime-session checkpoints",
      "turn blocked model run operations into executable native MLX/Core ML adapters and managed Python runtime sessions",
    ],
    endpoints: requiredContractEndpoints(),
    event_streams: [
      {
        id: "operation.events",
        events: [
          "RegistryEvent",
          "ScanEvent",
          "InstallEvent",
          "RemoveEvent",
          "ModelOperationEvent",
        ],
        event_names: requiredOperationEventNames(),
      },
    ],
    ...overrides,
  };
}

function operationKinds() {
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

function prerequisite(id, status) {
  return {
    id,
    status,
    message: `${id} is ${status}`,
  };
}

function test(name, run) {
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
      console.error(error instanceof Error ? error.message : String(error));
    }
  }
  if (failureCount > 0) {
    throw new Error(`${failureCount} build-tool ${failureCount === 1 ? "test" : "tests"} failed.`);
  }
}

function assertDeepEqual(actual, expected, message) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`${message}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function assertIncludes(value, expected, message) {
  if (!value.includes(expected)) {
    throw new Error(`${message}: expected ${JSON.stringify(value)} to include ${JSON.stringify(expected)}`);
  }
}
