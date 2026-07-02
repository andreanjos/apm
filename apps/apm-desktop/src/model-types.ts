export type ModelListResult = {
  packages: CachedModelPackage[];
};

export type ModelCatalogListResult = {
  packages: AvailableModelPackage[];
};

export type ModelStoreLayout = {
  root: string;
  manifests: string;
  weights: string;
  runtimes: string;
  cache: string;
  logs: string;
  config: string;
};

export type ModelStoreInitResult = {
  layout: ModelStoreLayout;
};

export type ModelManifestCacheResult = {
  model: CachedModelPackage;
  manifest_path: string;
  replaced: boolean;
};

export type ModelRemoveStatus = "removed" | "not_cached";

export type ModelRemoveResult = {
  package_id: string;
  manifest_path: string;
  runtime_dir?: string | null;
  weight_path?: string | null;
  status: ModelRemoveStatus;
  removed_manifest: boolean;
  removed_runtime: boolean;
  removed_weight: boolean;
  weight_still_referenced: boolean;
};

export type ModelWeightPullStatus = "cached" | "pulled";

export type ModelWeightPullResult = {
  package_id: string;
  source: string;
  resolved_url: string;
  sha256: string;
  path: string;
  bytes: number;
  status: ModelWeightPullStatus;
};

export type RuntimeProvisioningStatus = "prepared";

export type ModelRuntimeProvisioning = {
  adapter: string;
  status: RuntimeProvisioningStatus;
  runtime_mode: string;
  runtime_entry: string;
  runtime_dir: string;
  files: string[];
  message: string;
};

export type DesktopModelWeightPullResult =
  | { status: "completed"; result: ModelWeightPullResult }
  | { status: "failed"; error: string };

export type ModelInstallResult = {
  package_id: string;
  manifest_path: string;
  runtime_mode: string;
  runtime_entry: string;
  runtime: ModelRuntimeProvisioning;
  weights: ModelWeightPullResult;
};

export type DesktopModelInstallResult =
  | { status: "completed"; result: ModelInstallResult }
  | { status: "failed"; error: string };

export type DesktopModelRunResult =
  | { status: "completed"; result: ModelRunResult }
  | { status: "blocked"; result: ModelRunResult }
  | { status: "failed"; error: string };

export type ModelRunPlanStatus = "planned";

export type ModelRunParamBinding = {
  name: string;
  value: string | number | boolean;
  source: "default" | "request";
};

export type ModelRunExecutionReadiness =
  | {
      status: "ready";
      message: string;
    }
  | {
      status: "blocked";
      blocker: "adapter_runner_unavailable";
      message: string;
    };

export type ModelRunPlanRequest = {
  input_path: string;
  output_path: string;
  params?: Record<string, string | number | boolean>;
};

export type ModelRunPlan = {
  package_id: string;
  status: ModelRunPlanStatus;
  runtime_mode: string;
  runtime_entry: string;
  adapter: string;
  runtime_dir: string;
  adapter_manifest_path: string;
  weights_path: string;
  input_path: string;
  output_path: string;
  params: ModelRunParamBinding[];
  execution: ModelRunExecutionReadiness;
  message: string;
};

export type ModelRunStatus = "completed" | "blocked";

export type ModelRunResult = {
  package_id: string;
  status: ModelRunStatus;
  plan: ModelRunPlan;
  message: string;
};

export type ModelChainStepRequest = {
  name: string;
  version: string;
  params?: Record<string, string | number | boolean>;
};

export type ModelChainPlanRequest = {
  input_path: string;
  output_path: string;
  steps: ModelChainStepRequest[];
};

export type ModelChainPlanStatus = "planned";

export type ModelChainIoBinding = "direct" | "stem_selection_required";

export type ModelChainExecutionReadiness = {
  status: "blocked";
  blocker: "chain_runner_unavailable";
  message: string;
};

export type ModelChainStepPlan = {
  step_index: number;
  package_id: string;
  runtime_mode: string;
  runtime_entry: string;
  adapter: string;
  input: string;
  output: string;
  weights_path: string;
  runtime_dir: string;
  adapter_manifest_path: string;
  params: ModelRunParamBinding[];
  execution: ModelRunExecutionReadiness;
};

export type ModelChainEdgePlan = {
  from_step_index: number;
  to_step_index: number;
  from_output: string;
  to_input: string;
  binding: ModelChainIoBinding;
  message: string;
};

export type ModelChainPlan = {
  status: ModelChainPlanStatus;
  input_path: string;
  output_path: string;
  input: string;
  output: string;
  steps: ModelChainStepPlan[];
  edges: ModelChainEdgePlan[];
  execution: ModelChainExecutionReadiness;
  message: string;
};

export type CachedModelPackage = {
  package: ModelManifestSummary;
  runtime_entry: string;
  weights: ModelWeightsSummary;
  params: ModelParameterSummary[];
};

export type AvailableModelPackage = CachedModelPackage & {
  source_name?: string | null;
  manifest_path: string;
  manifest_cached: boolean;
};

export type ModelManifestSummary = {
  package_id: string;
  name: string;
  version: string;
  description: string;
  publisher: string;
  runtime_mode: string;
  input: string;
  output: string;
  parameter_count: number;
  min_memory_gb: number;
  commercial_license: boolean;
};

export type ModelWeightsSummary = {
  source: string;
  sha256: string;
  format: string;
  cached: boolean;
};

export type ModelParameterSummary = {
  name: string;
  type: string;
  values?: string[] | null;
  min?: number | null;
  max?: number | null;
  default?: string | number | boolean | null;
};

export type ModelOperationEvent =
  | { event: "model_weight_pull_started"; package_id: string }
  | {
      event: "model_weight_pull_progress";
      package_id: string;
      bytes: number;
      total_bytes?: number | null;
    }
  | {
      event: "model_weight_pull_finished";
      package_id: string;
      status: ModelWeightPullStatus;
      bytes: number;
    }
  | { event: "model_weight_pull_failed"; package_id: string; error: string }
  | { event: "model_install_started"; package_id: string }
  | {
      event: "model_install_finished";
      package_id: string;
      adapter: string;
      runtime_mode: string;
      runtime_status: RuntimeProvisioningStatus;
      weights_status: ModelWeightPullStatus;
    }
  | { event: "model_install_failed"; package_id: string; error: string }
  | { event: "model_run_started"; package_id: string }
  | {
      event: "model_run_completed";
      package_id: string;
      output_path: string;
      message: string;
    }
  | {
      event: "model_run_blocked";
      package_id: string;
      blocker: "adapter_runner_unavailable";
      message: string;
    }
  | { event: "model_run_failed"; package_id: string; error: string };
