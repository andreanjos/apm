import { operationHistorySection } from "./operation-history";
import type { OperationScopeLocks } from "./operation-events";
import type {
  ModelRunResult,
  OperationRecoverySummary,
  OperationStatus,
} from "./types";

const tests: Array<[string, () => void]> = [];

test("only renders retry buttons while request metadata is saved", () => {
  const html = operationHistorySection(
    [
      operationStatus({
        operation_id: "op-consumed",
        state: "failed",
        error: "Retry submitted as op-retry.",
        request: null,
      }),
      operationStatus({
        operation_id: "op-retryable",
        state: "failed",
        request: { kind: "registry_sync" },
      }),
    ],
    recovery(),
    null,
    false,
    retryLocks(),
  );

  assertEqual(
    html.includes('data-retry-operation-id="op-consumed"'),
    false,
    "consumed request metadata retry action",
  );
  assertEqual(
    html.includes("Retry submitted as op-retry."),
    true,
    "consumed request metadata audit message",
  );
  assertEqual(
    html.includes('data-retry-operation-id="op-retryable"'),
    true,
    "saved request metadata retry action",
  );
});

test("escapes operation history audit messages", () => {
  const html = operationHistorySection(
    [
      operationStatus({
        operation_id: "op-error",
        state: "failed",
        error: "failed <script>",
      }),
    ],
    recovery(),
    null,
    false,
    retryLocks(),
  );

  assertEqual(html.includes("failed &lt;script&gt;"), true, "escaped audit message");
  assertEqual(html.includes("failed <script>"), false, "raw audit message");
});

test("renders structured blocked model run results", () => {
  const html = operationHistorySection(
    [
      operationStatus({
        operation_id: "op-model-run",
        kind: "model_run",
        state: "failed",
        error: "fallback error should not be shown",
        result: {
          kind: "model_run",
          result: modelRunResult({
            package_id: "demucs<script>@4.0.1",
            message: "native-mlx execution <blocked>",
          }),
        },
      }),
    ],
    recovery(),
    null,
    false,
    retryLocks(),
  );

  assertEqual(
    html.includes("demucs&lt;script&gt;@4.0.1: native-mlx execution &lt;blocked&gt;"),
    true,
    "structured model run result",
  );
  assertEqual(
    html.includes("fallback error should not be shown"),
    false,
    "structured result takes priority",
  );
});

test("renders structured library scan results", () => {
  const html = operationHistorySection(
    [
      operationStatus({
        operation_id: "op-library-scan",
        kind: "library_scan",
        state: "succeeded",
        error: "fallback scan error should not be shown",
        result: {
          kind: "library_scan",
          result: {
            scanned_count: 8,
            visible_count: 7,
            matched_count: 3,
            tracked_count: 2,
            adopted_count: 1,
            learned_bundle_id_count: 1,
            au_count: 4,
            vst3_count: 4,
            plugins: [],
          },
        },
      }),
    ],
    recovery(),
    null,
    false,
    retryLocks(),
  );

  assertEqual(
    html.includes("Scanned 8; matched 3; adopted 1"),
    true,
    "structured library scan result",
  );
  assertEqual(
    html.includes("fallback scan error should not be shown"),
    false,
    "structured scan result takes priority",
  );
});

test("renders recovery retry action from recovery summary policy", () => {
  const html = operationHistorySection(
    [],
    recovery({ interrupted_count: 1, retryable_count: 1 }),
    null,
    false,
    retryLocks(),
  );

  assertEqual(
    html.includes('data-retry-recovery="true"'),
    true,
    "recovery retry action",
  );
  assertEqual(html.includes("Retry ready"), true, "recovery retry label");
});

test("locks retries while their operation lane is active", () => {
  const html = operationHistorySection(
    [
      operationStatus({
        operation_id: "op-sync",
        kind: "registry_sync",
        state: "failed",
        request: { kind: "registry_sync" },
      }),
      operationStatus({
        operation_id: "op-model",
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
    recovery(),
    null,
    false,
    retryLocks({ sync: true }),
  );

  assertEqual(
    html.includes('data-retry-operation-id="op-sync"'),
    true,
    "sync retry action rendered",
  );
  assertEqual(
    html.includes('aria-label="Sync operation running; retry unavailable"'),
    true,
    "sync retry locked aria label",
  );
  assertEqual(
    html.includes('title="Sync operation running"'),
    true,
    "sync retry locked title",
  );
  assertEqual(html.includes("disabled"), true, "sync retry disabled");
  assertEqual(
    html.includes('aria-label="Retry Model run operation"'),
    true,
    "unlocked model retry label",
  );
  assertEqual(
    html.includes('title="Retry operation"'),
    true,
    "unlocked model retry title",
  );
});

test("labels model retry locks as model actions", () => {
  const html = operationHistorySection(
    [
      operationStatus({
        operation_id: "op-model",
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
    recovery(),
    null,
    false,
    retryLocks({ model: true }),
  );

  assertEqual(
    html.includes('aria-label="Model action running; retry unavailable"'),
    true,
    "model retry locked aria label",
  );
  assertEqual(
    html.includes('title="Model action running"'),
    true,
    "model retry locked title",
  );
  assertEqual(
    html.includes("Model operation running"),
    false,
    "model retry avoids operation wording",
  );
});

test("locks recovery retry while any operation lane is active", () => {
  const html = operationHistorySection(
    [],
    recovery({ interrupted_count: 1, retryable_count: 1 }),
    null,
    false,
    retryLocks({ library: true }),
  );

  assertEqual(
    html.includes('aria-label="Operation operation running; retry unavailable"'),
    false,
    "recovery retry avoids duplicated operation wording",
  );
  assertEqual(
    html.includes('aria-label="Operation running; retry unavailable"'),
    true,
    "recovery retry locked aria label",
  );
  assertEqual(
    html.includes('title="Operation running"'),
    true,
    "recovery retry locked title",
  );
  assertEqual(html.includes("disabled"), true, "recovery retry disabled");
});

runTests();

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

function recovery(
  overrides: Partial<OperationRecoverySummary> = {},
): OperationRecoverySummary {
  return {
    interrupted_count: 0,
    retryable_count: 0,
    candidates: [],
    ...overrides,
  };
}

function retryLocks(overrides: Partial<OperationScopeLocks> = {}): OperationScopeLocks {
  return {
    sync: false,
    lifecycle: false,
    library: false,
    model: false,
    ...overrides,
  };
}

function modelRunResult(
  overrides: Partial<ModelRunResult> = {},
): ModelRunResult {
  const packageId = overrides.package_id ?? "demucs@4.0.1";
  return {
    package_id: packageId,
    status: "blocked",
    message: "native-mlx execution is not implemented yet",
    plan: {
      package_id: packageId,
      status: "planned",
      runtime_mode: "native-mlx",
      runtime_entry: "demucs.Model",
      adapter: "native-mlx",
      runtime_dir: "/tmp/.apm/runtimes/native-mlx/demucs/4.0.1",
      adapter_manifest_path: "/tmp/.apm/runtimes/native-mlx/demucs/4.0.1/adapter.toml",
      weights_path: "/tmp/.apm/weights/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      input_path: "mix.wav",
      output_path: "stems/",
      params: [],
      execution: {
        status: "blocked",
        blocker: "adapter_runner_unavailable",
        message: "native-mlx execution is not implemented yet",
      },
      message: "Runtime execution is pending.",
    },
    ...overrides,
  };
}

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
