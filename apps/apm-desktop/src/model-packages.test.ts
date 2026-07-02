import { modelPackagesSection } from "./model-packages";
import type {
  AvailableModelPackage,
  CachedModelPackage,
  ModelChainPlan,
  ModelCatalogListResult,
  ModelListResult,
  ModelRunPlan,
  ModelStoreLayout,
} from "./types";

const tests: Array<[string, () => void]> = [];

test("renders cached model packages with escaped metadata", () => {
  const html = modelPackagesSection(
    models({
      packages: [
        {
          package: {
            package_id: "demucs@4.0.1",
            name: "demucs<script>",
            version: "4.0.1",
            description: "Music source separation <stems>",
            publisher: "apm-core",
            runtime_mode: "native-mlx",
            input: "audio",
            output: "stems",
            parameter_count: 1,
            min_memory_gb: 8,
            commercial_license: true,
          },
          runtime_entry: "demucs_mlx.Separator",
          weights: {
            source: "hf:mlx-community/demucs-mlx-fp16",
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            format: "safetensors",
            cached: true,
          },
          params: [
            {
              name: "stems",
              type: "enum",
              values: ["2", "4"],
              default: "4",
            },
          ],
        },
      ],
    }),
    modelCatalog(),
  );

  assertEqual(html.includes("1 local / 0 catalog listed"), true, "model count status");
  assertEqual(html.includes("1 weights cached / 1 shown"), true, "local model count");
  assertEqual(html.includes("demucs&lt;script&gt;"), true, "escaped model name");
  assertEqual(html.includes("Music source separation &lt;stems&gt;"), true, "escaped description");
  assertEqual(html.includes("Weights cached"), true, "cached weight state");
  assertEqual(html.includes('aria-label="Verify weights"'), true, "cached weight verify action");
  assertEqual(html.includes('aria-label="Remove model"'), true, "cached model remove action");
  assertEqual(html.includes('aria-label="Install model"'), true, "cached model install action");
  assertEqual(html.includes('aria-label="Plan model run"'), true, "cached model run plan action");
  assertEqual(
    html.includes('aria-label="Check execution readiness"'),
    true,
    "cached model run check action",
  );
  assertEqual(html.includes('aria-label="Add to model chain"'), true, "cached model chain action");
  assertEqual(html.includes("audio"), true, "input label");
  assertEqual(html.includes("stems:enum"), true, "parameter chip");
});

test("renders registry model catalog with import state", () => {
  const html = modelPackagesSection(
    models(),
    modelCatalog({
      packages: [
        catalogModelPackage({
          source_name: "community<script>",
          manifest_path: "/Registry/models/demucs/4.0.1.toml",
          manifest_cached: false,
        }),
      ],
    }),
  );

  assertEqual(html.includes("0 local / 1 catalog listed"), true, "catalog count");
  assertEqual(html.includes("Available manifests"), true, "catalog heading");
  assertEqual(html.includes("community&lt;script&gt;"), true, "escaped source name");
  assertEqual(html.includes("Registry only"), true, "uncached catalog state");
  assertEqual(
    html.includes('data-import-catalog-model-name="demucs"'),
    true,
    "catalog import model name",
  );
  assertEqual(html.includes('aria-label="Add to local store"'), true, "catalog import action");
});

test("renders registry model catalog import action state", () => {
  const html = modelPackagesSection(
    models(),
    modelCatalog({ packages: [catalogModelPackage({ manifest_cached: true })] }),
    { importingCatalogModelId: "demucs@4.0.1" },
  );

  assertEqual(html.includes('aria-label="Adding model"'), true, "busy catalog import label");
  assertEqual(html.includes("Manifest local"), true, "cached manifest state");
  assertEqual(html.includes("disabled"), true, "busy catalog import disabled");
});

test("renders empty model package state", () => {
  const html = modelPackagesSection(models(), modelCatalog());

  assertEqual(html.includes("0 local / 0 catalog listed"), true, "empty model count");
  assertEqual(
    html.includes("No registry model manifests listed."),
    true,
    "empty catalog copy",
  );
  assertEqual(html.includes("No model manifests cached yet."), true, "empty model copy");
});

test("renders model store layout with escaped paths", () => {
  const html = modelPackagesSection(models(), modelCatalog(), {
    modelStore: modelStoreLayout({ weights: "/tmp/.apm/weights <shared>" }),
  });

  assertEqual(html.includes('aria-label="Model store layout"'), true, "store layout");
  assertEqual(html.includes("Local layout"), true, "store heading");
  assertEqual(
    html.includes("data-initialize-model-store"),
    true,
    "store initialize action",
  );
  assertEqual(html.includes("/tmp/.apm/weights &lt;shared&gt;"), true, "escaped weights path");
  assertEqual(html.includes("/tmp/.apm/config.toml"), true, "config path");
});

test("renders model store initialize busy state", () => {
  const html = modelPackagesSection(models(), modelCatalog(), {
    modelStore: modelStoreLayout(),
    modelStoreInitializing: true,
  });

  assertEqual(
    html.includes('aria-label="Initializing model store"'),
    true,
    "store initialize busy label",
  );
  assertEqual(html.includes("disabled"), true, "store initialize disabled");
});

test("filters cached model packages by query", () => {
  const whisper = modelPackage({
    package: {
      ...modelPackage().package,
      package_id: "whisper@1.0.0",
      name: "whisper",
      description: "Speech to text",
      output: "text",
    },
    runtime_entry: "whisper.Model",
  });
  const html = modelPackagesSection(
    models({ packages: [modelPackage(), whisper] }),
    modelCatalog({
      packages: [catalogModelPackage(), catalogModelPackage({ package: whisper.package })],
    }),
    { modelSearchQuery: "stems" },
  );

  assertEqual(html.includes('value="stems"'), true, "search value");
  assertEqual(html.includes("1 local / 1 catalog shown"), true, "filtered count");
  assertEqual(html.includes("demucs@4.0.1"), true, "matching model visible");
  assertEqual(html.includes("whisper@1.0.0"), false, "nonmatching model hidden");
});

test("renders empty model search state", () => {
  const html = modelPackagesSection(
    models({ packages: [modelPackage()] }),
    modelCatalog({ packages: [catalogModelPackage()] }),
    { modelSearchQuery: "no-such-model" },
  );

  assertEqual(html.includes("0 local / 0 catalog shown"), true, "empty filtered count");
  assertEqual(
    html.includes("No registry model packages matched."),
    true,
    "empty catalog search copy",
  );
  assertEqual(
    html.includes("No cached model packages matched."),
    true,
    "empty search copy",
  );
});

test("renders model manifest import action state", () => {
  const html = modelPackagesSection(models(), modelCatalog(), {
    notice: { tone: "success", message: "demucs<script> cached." },
    importing: true,
  });

  assertEqual(html.includes("data-import-model-manifest"), true, "model import action is rendered");
  assertEqual(html.includes('aria-label="Importing model manifest"'), true, "busy model import label");
  assertEqual(html.includes("disabled"), true, "busy model import button disabled");
  assertEqual(html.includes("demucs&lt;script&gt; cached."), true, "escaped model notice");
});

test("renders model operation progress events", () => {
  const html = modelPackagesSection(models(), modelCatalog(), {
    modelEvents: [
      { event: "model_install_started", package_id: "demucs<script>@4.0.1" },
      {
        event: "model_weight_pull_progress",
        package_id: "demucs@4.0.1",
        bytes: 1_048_576,
        total_bytes: 2_097_152,
      },
      {
        event: "model_install_finished",
        package_id: "demucs@4.0.1",
        adapter: "native-mlx",
        runtime_mode: "native-mlx",
        runtime_status: "prepared",
        weights_status: "cached",
      },
    ],
  });

  assertEqual(
    html.includes("Installing demucs&lt;script&gt;@4.0.1"),
    true,
    "escaped model progress start",
  );
  assertEqual(
    html.includes("demucs@4.0.1 weights 1.0 MB of 2.0 MB"),
    true,
    "model weight pull progress",
  );
  assertEqual(
    html.includes("demucs@4.0.1 ready (native-mlx prepared, weights cached)"),
    true,
    "model progress finished",
  );
});

test("locks model actions while a model operation is active", () => {
  const html = modelPackagesSection(
    models({ packages: [modelPackage()] }),
    modelCatalog({ packages: [catalogModelPackage()] }),
    {
      modelStore: modelStoreLayout(),
      modelOperation: { operationId: "op-model", canceling: false },
    },
  );

  assertEqual(
    html.includes('data-import-model-manifest type="button" aria-label="Model action running"'),
    true,
    "manifest import locked",
  );
  assertEqual(
    html.includes('data-initialize-model-store type="button" aria-label="Model action running"'),
    true,
    "model store init locked",
  );
  assertEqual(
    html.includes('data-import-catalog-model-name="demucs" data-import-catalog-model-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "catalog import locked",
  );
  assertEqual(
    html.includes('data-install-model-name="demucs" data-install-model-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "model install locked",
  );
  assertEqual(
    html.includes('data-plan-model-run-name="demucs" data-plan-model-run-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "model run planning locked",
  );
  assertEqual(
    html.includes('data-run-model-name="demucs" data-run-model-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "model run check locked",
  );
  assertEqual(
    html.includes('data-pull-model-name="demucs" data-pull-model-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "model weight pull locked",
  );
  assertEqual(
    html.includes('data-remove-model-name="demucs" data-remove-model-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "model remove locked",
  );
});

test("locks model actions while a local model action is active", () => {
  const html = modelPackagesSection(
    models({ packages: [modelPackage()] }),
    modelCatalog({ packages: [catalogModelPackage()] }),
    {
      modelStore: modelStoreLayout(),
      importingCatalogModelId: "demucs@4.0.1",
    },
  );

  assertEqual(
    html.includes('data-import-catalog-model-name="demucs" data-import-catalog-model-version="4.0.1" type="button" aria-label="Adding model"'),
    true,
    "active catalog import keeps busy label",
  );
  assertEqual(
    html.includes('data-import-model-manifest type="button" aria-label="Model action running"'),
    true,
    "manifest import locked by local action",
  );
  assertEqual(
    html.includes('data-initialize-model-store type="button" aria-label="Model action running"'),
    true,
    "model store init locked by local action",
  );
  assertEqual(
    html.includes('data-install-model-name="demucs" data-install-model-version="4.0.1" type="button" aria-label="Model action running"'),
    true,
    "model install locked by local action",
  );
});

test("renders model install action state", () => {
  const model = modelPackage();
  const html = modelPackagesSection(models({ packages: [model] }), modelCatalog(), {
    installingModelId: "demucs@4.0.1",
  });

  assertEqual(html.includes('data-install-model-name="demucs"'), true, "install model name");
  assertEqual(html.includes('aria-label="Installing model"'), true, "install busy label");
  assertEqual(html.includes("disabled"), true, "install button disabled");
});

test("renders model run plan action state", () => {
  const model = modelPackage();
  const html = modelPackagesSection(models({ packages: [model] }), modelCatalog(), {
    planningModelId: "demucs@4.0.1",
  });

  assertEqual(html.includes('data-plan-model-run-name="demucs"'), true, "plan model run name");
  assertEqual(html.includes('aria-label="Planning model run"'), true, "run plan busy label");
  assertEqual(html.includes("disabled"), true, "run plan button disabled");
});

test("renders model run execution check state", () => {
  const model = modelPackage();
  const html = modelPackagesSection(models({ packages: [model] }), modelCatalog(), {
    runningModelId: "demucs@4.0.1",
  });

  assertEqual(html.includes('data-run-model-name="demucs"'), true, "run model name");
  assertEqual(
    html.includes('aria-label="Checking execution readiness"'),
    true,
    "run check busy label",
  );
  assertEqual(html.includes("disabled"), true, "run check button disabled");
});

test("renders model chain draft with escaped steps", () => {
  const html = modelPackagesSection(
    models({ packages: [modelPackage()] }),
    modelCatalog(),
    {
      chainSteps: [
        {
          name: "demucs<script>",
          version: "4.0.1",
          packageId: "demucs<script>@4.0.1",
        },
      ],
      planningModelChain: true,
    },
  );

  assertEqual(html.includes('aria-label="Model chain draft"'), true, "chain draft");
  assertEqual(html.includes("Review order"), true, "chain draft heading");
  assertEqual(html.includes("1 step"), true, "chain step count");
  assertEqual(html.includes("demucs&lt;script&gt;@4.0.1"), true, "escaped chain package");
  assertEqual(html.includes('aria-label="Planning model chain"'), true, "chain busy label");
  assertEqual(html.includes("disabled"), true, "chain plan disabled while busy");
});

test("renders model chain plan summary with escaped paths", () => {
  const html = modelPackagesSection(models(), modelCatalog(), {
    chainPlan: modelChainPlan({
      input_path: "/Users/me/Mix <dry>.wav",
      output_path: "/Users/me/lyrics <out>.txt",
      steps: [
        {
          ...modelChainPlan().steps[0],
          package_id: "demucs<script>@4.0.1",
        },
      ],
    }),
  });

  assertEqual(html.includes('aria-label="Model chain plan"'), true, "chain plan summary");
  assertEqual(html.includes("demucs&lt;script&gt;@4.0.1"), true, "escaped chain package");
  assertEqual(html.includes("audio / /Users/me/Mix &lt;dry&gt;.wav"), true, "escaped input");
  assertEqual(html.includes("stems / /Users/me/lyrics &lt;out&gt;.txt"), true, "escaped output");
  assertEqual(html.includes("1:demucs&lt;script&gt;@4.0.1 audio-&gt;stems"), true, "chain step");
  assertEqual(
    html.includes("blocked: Chain execution for 1 prepared step"),
    true,
    "chain execution readiness",
  );
});

test("renders model run plan summary with escaped paths", () => {
  const html = modelPackagesSection(models(), modelCatalog(), {
    runPlan: modelRunPlan({
      package_id: "demucs<script>@4.0.1",
      input_path: "/Users/me/Mix <dry>.wav",
      output_path: "/Users/me/stems <out>",
      params: [{ name: "stems", value: "4", source: "default" }],
    }),
  });

  assertEqual(html.includes('aria-label="Model run plan"'), true, "run plan summary");
  assertEqual(html.includes("demucs&lt;script&gt;@4.0.1"), true, "escaped package id");
  assertEqual(html.includes("native-mlx / demucs_mlx.Separator"), true, "adapter details");
  assertEqual(html.includes("/Users/me/Mix &lt;dry&gt;.wav"), true, "escaped input path");
  assertEqual(html.includes("/Users/me/stems &lt;out&gt;"), true, "escaped output path");
  assertEqual(html.includes("stems=4"), true, "run plan params");
  assertEqual(
    html.includes("blocked: native-mlx execution"),
    true,
    "run plan execution readiness",
  );
  assertEqual(html.includes("planned"), true, "run plan status");
});

test("renders model weight pull action state", () => {
  const model = modelPackage();
  const html = modelPackagesSection(
    models({
      packages: [{ ...model, weights: { ...model.weights, cached: false } }],
    }),
    modelCatalog(),
    { pullingModelId: "demucs@4.0.1" },
  );

  assertEqual(html.includes("Weights missing"), true, "missing weight state");
  assertEqual(html.includes('data-pull-model-name="demucs"'), true, "pull model name");
  assertEqual(html.includes('aria-label="Pulling weights"'), true, "pull busy label");
  assertEqual(html.includes("disabled"), true, "pull button disabled");
});

test("renders model remove action state", () => {
  const model = modelPackage();
  const html = modelPackagesSection(models({ packages: [model] }), modelCatalog(), {
    removingModelId: "demucs@4.0.1",
  });

  assertEqual(html.includes('data-remove-model-name="demucs"'), true, "remove model name");
  assertEqual(html.includes('aria-label="Removing model"'), true, "remove busy label");
  assertEqual(html.includes("disabled"), true, "remove button disabled");
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

function models(overrides: Partial<ModelListResult> = {}): ModelListResult {
  return {
    packages: [],
    ...overrides,
  };
}

function modelCatalog(
  overrides: Partial<ModelCatalogListResult> = {},
): ModelCatalogListResult {
  return {
    packages: [],
    ...overrides,
  };
}

function catalogModelPackage(
  overrides: Partial<AvailableModelPackage> = {},
): AvailableModelPackage {
  const cached = modelPackage();
  return {
    ...cached,
    source_name: "official",
    manifest_path: "/Registry/models/demucs/4.0.1.toml",
    manifest_cached: true,
    ...overrides,
  };
}

function modelPackage(
  overrides: Partial<CachedModelPackage> = {},
): CachedModelPackage {
  return {
    package: {
      package_id: "demucs@4.0.1",
      name: "demucs",
      version: "4.0.1",
      description: "Music source separation",
      publisher: "apm-core",
      runtime_mode: "native-mlx",
      input: "audio",
      output: "stems",
      parameter_count: 1,
      min_memory_gb: 8,
      commercial_license: true,
    },
    runtime_entry: "demucs_mlx.Separator",
    weights: {
      source: "https://example.test/model.safetensors",
      sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      format: "safetensors",
      cached: true,
    },
    params: [],
    ...overrides,
  };
}

function modelRunPlan(overrides: Partial<ModelRunPlan> = {}): ModelRunPlan {
  const base: ModelRunPlan = {
    package_id: "demucs@4.0.1",
    status: "planned",
    runtime_mode: "native-mlx",
    runtime_entry: "demucs_mlx.Separator",
    adapter: "native-mlx",
    runtime_dir: "/Users/me/Library/Application Support/apm/models/demucs/4.0.1/runtime",
    adapter_manifest_path:
      "/Users/me/Library/Application Support/apm/models/demucs/4.0.1/runtime/adapter.toml",
    weights_path: "/Users/me/Library/Application Support/apm/model-weights/model.safetensors",
    input_path: "/Users/me/mix.wav",
    output_path: "/Users/me/stems",
    params: [],
    execution: {
      status: "blocked",
      blocker: "adapter_runner_unavailable",
      message:
        "native-mlx execution for demucs@4.0.1 is not implemented yet; this plan is review-only.",
    },
    message: "Run plan ready.",
  };
  return {
    ...base,
    ...overrides,
    status: overrides.status ?? base.status,
    params: overrides.params ?? base.params,
  };
}

function modelChainPlan(overrides: Partial<ModelChainPlan> = {}): ModelChainPlan {
  const base: ModelChainPlan = {
    status: "planned",
    input_path: "/Users/me/mix.wav",
    output_path: "/Users/me/stems",
    input: "audio",
    output: "stems",
    steps: [
      {
        step_index: 0,
        package_id: "demucs@4.0.1",
        runtime_mode: "native-mlx",
        runtime_entry: "demucs_mlx.Separator",
        adapter: "native-mlx",
        input: "audio",
        output: "stems",
        weights_path: "/Users/me/Library/Application Support/apm/model-weights/model.safetensors",
        runtime_dir: "/Users/me/Library/Application Support/apm/models/demucs/4.0.1/runtime",
        adapter_manifest_path:
          "/Users/me/Library/Application Support/apm/models/demucs/4.0.1/runtime/adapter.toml",
        params: [],
        execution: {
          status: "blocked",
          blocker: "adapter_runner_unavailable",
          message:
            "native-mlx execution for demucs@4.0.1 is not implemented yet; this plan is review-only.",
        },
      },
    ],
    edges: [],
    execution: {
      status: "blocked",
      blocker: "chain_runner_unavailable",
      message:
        "Chain execution for 1 prepared step is not implemented yet; this plan is review-only.",
    },
    message: "Chain plan ready.",
  };
  return {
    ...base,
    ...overrides,
    status: overrides.status ?? base.status,
    steps: overrides.steps ?? base.steps,
    edges: overrides.edges ?? base.edges,
  };
}

function modelStoreLayout(overrides: Partial<ModelStoreLayout> = {}): ModelStoreLayout {
  return {
    root: "/tmp/.apm",
    manifests: "/tmp/.apm/manifests",
    weights: "/tmp/.apm/weights",
    runtimes: "/tmp/.apm/runtimes",
    cache: "/tmp/.apm/cache",
    logs: "/tmp/.apm/logs",
    config: "/tmp/.apm/config.toml",
    ...overrides,
  };
}

function assertEqual<T>(actual: T, expected: T, message: string) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${formatValue(expected)}, got ${formatValue(actual)}`);
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatValue(value: unknown) {
  return value === null ? "null" : JSON.stringify(value);
}
