import type {
  DesktopModelInstallResult,
  DesktopModelRunResult,
  DesktopModelWeightPullResult,
  ModelChainEdgePlan,
  ModelChainIoBinding,
  ModelChainPlan,
  ModelChainPlanRequest,
  ModelChainStepPlan,
  ModelManifestCacheResult,
  ModelParameterSummary,
  ModelRemoveResult,
  ModelRunParamBinding,
  ModelRunPlan,
} from "./types";
import { fallbackSnapshot } from "./fallback-data";

export function fallbackImportModelManifest(): ModelManifestCacheResult {
  const model = fallbackSnapshot.models.packages[0];
  if (!model) {
    throw new Error("Preview model data is unavailable.");
  }

  return {
    model,
    manifest_path: "~/.apm/manifests/demucs/4.0.1.toml",
    replaced: true,
  };
}

export function fallbackImportModelCatalogPackage(
  name: string,
  version: string,
): ModelManifestCacheResult {
  const model = fallbackSnapshot.model_catalog.packages.find(
    (candidate) => candidate.package.name === name && candidate.package.version === version,
  );
  if (!model) {
    throw new Error(`${name}@${version} is not in the preview model catalog.`);
  }

  return {
    model,
    manifest_path: `~/.apm/manifests/${name}/${version}.toml`,
    replaced: model.manifest_cached,
  };
}

export function fallbackPullModelWeights(
  name: string,
  version: string,
): DesktopModelWeightPullResult {
  const model = fallbackSnapshot.models.packages.find(
    (candidate) => candidate.package.name === name && candidate.package.version === version,
  );
  if (!model) {
    return { status: "failed", error: `${name}@${version} is not cached.` };
  }

  return {
    status: "completed",
    result: {
      package_id: model.package.package_id,
      source: model.weights.source,
      resolved_url: "https://example.test/model.safetensors",
      sha256: model.weights.sha256,
      path: `~/.apm/weights/${model.weights.sha256}`,
      bytes: 13,
      status: "pulled",
    },
  };
}

export function fallbackInstallModelPackage(
  name: string,
  version: string,
): DesktopModelInstallResult {
  const model = fallbackSnapshot.models.packages.find(
    (candidate) => candidate.package.name === name && candidate.package.version === version,
  );
  if (!model) {
    return { status: "failed", error: `${name}@${version} is not cached.` };
  }
  const pull = fallbackPullModelWeights(name, version);
  if (pull.status === "failed") {
    return pull;
  }

  return {
    status: "completed",
    result: {
      package_id: pull.result.package_id,
      manifest_path: `~/.apm/manifests/${name}/${version}.toml`,
      runtime_mode: model.package.runtime_mode,
      runtime_entry: model.runtime_entry,
      runtime: {
        adapter: model.package.runtime_mode,
        status: "prepared",
        runtime_mode: model.package.runtime_mode,
        runtime_entry: model.runtime_entry,
        runtime_dir: `~/.apm/runtimes/${model.package.runtime_mode}/${name}/${version}`,
        files: [`~/.apm/runtimes/${model.package.runtime_mode}/${name}/${version}/adapter.toml`],
        message: `${model.package.runtime_mode} runtime metadata prepared.`,
      },
      weights: pull.result,
    },
  };
}

export function fallbackPlanModelRun(
  name: string,
  version: string,
  inputPath: string,
  outputPath: string,
): ModelRunPlan {
  const model = fallbackSnapshot.models.packages.find(
    (candidate) => candidate.package.name === name && candidate.package.version === version,
  );
  if (!model) {
    throw new Error(`${name}@${version} is not cached.`);
  }

  return {
    package_id: model.package.package_id,
    status: "planned",
    runtime_mode: model.package.runtime_mode,
    runtime_entry: model.runtime_entry,
    adapter: model.package.runtime_mode,
    runtime_dir: `~/.apm/runtimes/${model.package.runtime_mode}/${name}/${version}`,
    adapter_manifest_path: `~/.apm/runtimes/${model.package.runtime_mode}/${name}/${version}/adapter.toml`,
    weights_path: `~/.apm/weights/${model.weights.sha256}`,
    input_path: inputPath,
    output_path: outputPath,
    params: fallbackModelRunParams(model.params, {}),
    execution: {
      status: "blocked",
      blocker: "adapter_runner_unavailable",
      message: `${model.package.runtime_mode} execution for ${model.package.package_id} is not implemented yet; this plan is review-only.`,
    },
    message: `Runtime execution is pending; this plan binds prepared adapter metadata for ${model.package.package_id}.`,
  };
}

export function fallbackRunModel(
  name: string,
  version: string,
  inputPath: string,
  outputPath: string,
): DesktopModelRunResult {
  const plan = fallbackPlanModelRun(name, version, inputPath, outputPath);
  return {
    status: "blocked",
    result: {
      package_id: plan.package_id,
      status: "blocked",
      plan,
      message: plan.execution.message,
    },
  };
}

export function fallbackPlanModelChain(
  request: ModelChainPlanRequest,
): ModelChainPlan {
  if (request.input_path.trim().length === 0) {
    throw new Error("model chain input_path must not be empty");
  }
  if (request.output_path.trim().length === 0) {
    throw new Error("model chain output_path must not be empty");
  }
  if (request.steps.length === 0) {
    throw new Error("model chain must include at least one step");
  }

  const steps = request.steps.map((step, index) => {
    const model = fallbackSnapshot.models.packages.find(
      (candidate) =>
        candidate.package.name === step.name && candidate.package.version === step.version,
    );
    if (!model) {
      throw new Error(`${step.name}@${step.version} is not cached.`);
    }

    return {
      step_index: index,
      package_id: model.package.package_id,
      runtime_mode: model.package.runtime_mode,
      runtime_entry: model.runtime_entry,
      adapter: model.package.runtime_mode,
      input: model.package.input,
      output: model.package.output,
      weights_path: `~/.apm/weights/${model.weights.sha256}`,
      runtime_dir: `~/.apm/runtimes/${model.package.runtime_mode}/${step.name}/${step.version}`,
      adapter_manifest_path: `~/.apm/runtimes/${model.package.runtime_mode}/${step.name}/${step.version}/adapter.toml`,
      params: fallbackModelRunParams(model.params, step.params ?? {}),
      execution: {
        status: "blocked",
        blocker: "adapter_runner_unavailable",
        message: `${model.package.runtime_mode} execution for ${model.package.package_id} is not implemented yet; this plan is review-only.`,
      },
    } satisfies ModelChainStepPlan;
  });
  const edges = fallbackChainEdges(steps);

  return {
    status: "planned",
    input_path: request.input_path,
    output_path: request.output_path,
    input: steps[0].input,
    output: steps[steps.length - 1].output,
    steps,
    edges,
    execution: {
      status: "blocked",
      blocker: "chain_runner_unavailable",
      message: `Chain execution for ${steps.length} prepared step${steps.length === 1 ? "" : "s"} is not implemented yet; this plan is review-only.`,
    },
    message: `Runtime chain execution is pending; this plan validates ${steps.length} prepared step${steps.length === 1 ? "" : "s"} and ${edges.length} IO edge${edges.length === 1 ? "" : "s"}.`,
  };
}

export function fallbackRemoveModelPackage(
  name: string,
  version: string,
): ModelRemoveResult {
  const model = fallbackSnapshot.models.packages.find(
    (candidate) => candidate.package.name === name && candidate.package.version === version,
  );
  if (!model) {
    return {
      package_id: `${name}@${version}`,
      manifest_path: `~/.apm/manifests/${name}/${version}.toml`,
      status: "not_cached",
      removed_manifest: false,
      removed_runtime: false,
      removed_weight: false,
      weight_still_referenced: false,
    };
  }

  return {
    package_id: model.package.package_id,
    manifest_path: `~/.apm/manifests/${name}/${version}.toml`,
    runtime_dir: `~/.apm/runtimes/${model.package.runtime_mode}/${name}/${version}`,
    weight_path: `~/.apm/weights/${model.weights.sha256}`,
    status: "removed",
    removed_manifest: true,
    removed_runtime: true,
    removed_weight: true,
    weight_still_referenced: false,
  };
}

function fallbackModelRunParams(
  modelParams: ModelParameterSummary[],
  requestedParams: Record<string, string | number | boolean>,
): ModelRunParamBinding[] {
  const bindings = modelParams.filter(hasModelParameterDefault).map((param) => {
    if (Object.prototype.hasOwnProperty.call(requestedParams, param.name)) {
      return {
        name: param.name,
        value: requestedParams[param.name],
        source: "request" as const,
      };
    }

    return {
      name: param.name,
      value: param.default,
      source: "default" as const,
    };
  });

  for (const paramName of Object.keys(requestedParams)) {
    if (!modelParams.some((param) => param.name === paramName)) {
      throw new Error(`unknown model parameter ${paramName}`);
    }
    if (!bindings.some((binding) => binding.name === paramName)) {
      bindings.push({
        name: paramName,
        value: requestedParams[paramName],
        source: "request",
      });
    }
  }

  return bindings;
}

function fallbackChainEdges(steps: ModelChainStepPlan[]): ModelChainEdgePlan[] {
  const edges: ModelChainEdgePlan[] = [];
  for (let index = 0; index < steps.length - 1; index += 1) {
    const left = steps[index];
    const right = steps[index + 1];
    const binding = fallbackChainIoBinding(left.output, right.input);
    if (binding === null) {
      throw new Error(
        `model chain IO mismatch: step ${left.step_index} ${left.package_id} outputs ${left.output}, but step ${right.step_index} ${right.package_id} requires ${right.input}`,
      );
    }

    edges.push({
      from_step_index: left.step_index,
      to_step_index: right.step_index,
      from_output: left.output,
      to_input: right.input,
      binding,
      message:
        binding === "direct"
          ? `Step ${left.step_index} ${left.output} output feeds step ${right.step_index} ${right.input} input directly.`
          : `Step ${left.step_index} outputs stems; select one audio stem before step ${right.step_index}.`,
    });
  }
  return edges;
}

function fallbackChainIoBinding(
  output: string,
  input: string,
): ModelChainIoBinding | null {
  if (output === input) {
    return "direct";
  }
  if (output === "stems" && input === "audio") {
    return "stem_selection_required";
  }
  return null;
}

function hasModelParameterDefault(
  param: ModelParameterSummary,
): param is ModelParameterSummary & { default: string | number | boolean } {
  return param.default !== null && param.default !== undefined;
}
