import type {
  AvailableModelPackage,
  CachedModelPackage,
  ModelChainPlan,
  ModelCatalogListResult,
  ModelListResult,
  ModelOperationEvent,
  ModelParameterSummary,
  ModelRunPlan,
  ModelStoreLayout,
} from "./types";
import type {
  LifecycleNotice,
  ModelChainDraftStep,
  OperationControlState,
} from "./view-model";
import { lifecycleNoticeMarkup, operationCancelMarkup } from "./lifecycle-markup";
import { modelActionActive } from "./model-action-lock";
import { modelProgressLabel } from "./operation-events";
import { escapeHtml } from "./view-utils";

const MODEL_ACTION_LOCKED_LABEL = "Model action running";

type ModelPackagesSectionOptions = {
  notice?: LifecycleNotice | null;
  runPlan?: ModelRunPlan | null;
  chainPlan?: ModelChainPlan | null;
  chainSteps?: ModelChainDraftStep[];
  modelStore?: ModelStoreLayout | null;
  modelEvents?: ModelOperationEvent[];
  modelOperation?: OperationControlState | null;
  modelStoreInitializing?: boolean;
  importing?: boolean;
  importingCatalogModelId?: string | null;
  installingModelId?: string | null;
  planningModelId?: string | null;
  planningModelChain?: boolean;
  pullingModelId?: string | null;
  removingModelId?: string | null;
  runningModelId?: string | null;
  modelSearchQuery?: string;
};

export function modelPackagesSection(
  models: ModelListResult,
  modelCatalog: ModelCatalogListResult,
  options: ModelPackagesSectionOptions = {},
) {
  const {
    notice = null,
    runPlan = null,
    chainPlan = null,
    chainSteps = [],
    modelStore = null,
    modelEvents = [],
    modelOperation = null,
    modelStoreInitializing = false,
    importing = false,
    importingCatalogModelId = null,
    installingModelId = null,
    planningModelId = null,
    planningModelChain = false,
    pullingModelId = null,
    removingModelId = null,
    runningModelId = null,
    modelSearchQuery = "",
  } = options;
  const visibleCatalogPackages = modelSearchQuery.trim()
    ? modelCatalog.packages.filter((model) => catalogPackageMatchesQuery(model, modelSearchQuery))
    : modelCatalog.packages;
  const visibleLocalPackages = modelSearchQuery.trim()
    ? models.packages.filter((model) => modelPackageMatchesQuery(model, modelSearchQuery))
    : models.packages;
  const modelActionLocked = modelActionActive({
    modelOperationActive: modelOperation !== null,
    modelStoreInitializing,
    modelImporting: importing,
    importingCatalogModelId,
    installingModelId,
    planningModelId,
    pullingModelId,
    removingModelId,
    runningModelId,
    planningModelChain,
  });
  const cachedWeightCount = visibleLocalPackages.filter((model) => model.weights.cached).length;
  const localManifestCount = visibleCatalogPackages.filter((model) => model.manifest_cached).length;
  const actionLabel = modelImportActionLabel(importing, modelActionLocked);
  const countLabel = modelSearchQuery.trim()
    ? `${visibleLocalPackages.length} local / ${visibleCatalogPackages.length} catalog shown`
    : `${models.packages.length} local / ${modelCatalog.packages.length} catalog listed`;
  return `
    <section class="panel model-panel">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Audio-AI</p>
          <h2>Model packages</h2>
        </div>
        <div class="model-header-actions">
          <input id="model-search" class="model-search-input" type="search" value="${escapeHtml(modelSearchQuery)}" placeholder="Search models" aria-label="Search model packages">
          <span class="status-pill">${countLabel}</span>
          <button class="icon-button model-import-button" data-import-model-manifest type="button" aria-label="${actionLabel}" title="${actionLabel}" ${importing || modelActionLocked ? "disabled" : ""}>
            <i data-lucide="file-plus" aria-hidden="true"></i>
          </button>
        </div>
      </div>
      ${lifecycleNoticeMarkup(notice)}
      ${modelOperationMarkup(modelOperation)}
      ${modelStoreLayoutMarkup(modelStore, modelStoreInitializing, modelActionLocked)}
      ${modelRunPlanMarkup(runPlan)}
      ${modelChainPlanMarkup(chainPlan)}
      ${modelChainDraftMarkup(chainSteps, planningModelChain, modelActionLocked)}
      ${modelTimelineMarkup(modelEvents)}
      <div class="model-subsection">
        <div class="model-subheader">
          <div>
            <p class="eyebrow">Registry catalog</p>
            <h3>Available manifests</h3>
          </div>
          <span class="status-pill">${localManifestCount} local / ${visibleCatalogPackages.length} shown</span>
        </div>
        <div class="model-list catalog-model-list">
          ${
            visibleCatalogPackages.length > 0
              ? visibleCatalogPackages
                  .map((model) =>
                    catalogPackageCard(model, importingCatalogModelId, modelActionLocked),
                  )
                  .join("")
              : `<div class="empty-library">${modelSearchQuery.trim() ? "No registry model packages matched." : "No registry model manifests listed."}</div>`
          }
        </div>
      </div>
      <div class="model-subsection">
        <div class="model-subheader">
          <div>
            <p class="eyebrow">Local store</p>
            <h3>Cached manifests</h3>
          </div>
          <span class="status-pill">${cachedWeightCount} weights cached / ${visibleLocalPackages.length} shown</span>
        </div>
        <div class="model-list">
          ${
            visibleLocalPackages.length > 0
              ? visibleLocalPackages
                  .map((model) =>
                    modelPackageCard(
                      model,
                      installingModelId,
                      planningModelId,
                      pullingModelId,
                      removingModelId,
                      runningModelId,
                      chainSteps,
                      planningModelChain,
                      modelActionLocked,
                    ),
                  )
                  .join("")
              : `<div class="empty-library">${modelSearchQuery.trim() ? "No cached model packages matched." : "No model manifests cached yet."}</div>`
          }
        </div>
      </div>
    </section>
  `;
}

function modelImportActionLabel(importing: boolean, modelActionLocked: boolean) {
  return modelBusyActionLabel(
    importing,
    "Importing model manifest",
    "Import model manifest",
    modelActionLocked,
  );
}

function modelOperationMarkup(operation: OperationControlState | null) {
  if (!operation?.operationId) {
    return "";
  }

  return `
    <div class="model-operation">
      ${operationCancelMarkup("model", operation)}
    </div>
  `;
}

function modelStoreLayoutMarkup(
  modelStore: ModelStoreLayout | null,
  modelStoreInitializing: boolean,
  modelActionLocked: boolean,
) {
  if (!modelStore) {
    return "";
  }

  const paths = modelStorePathDetails(modelStore);
  const initializeLabel = modelStoreInitializing
    ? "Initializing model store"
    : modelReadyActionLabel("Initialize model store", modelActionLocked);

  return `
    <div class="model-subsection model-store-layout" aria-label="Model store layout">
      <div class="model-subheader">
        <div>
          <p class="eyebrow">Store</p>
          <h3>Local layout</h3>
        </div>
        <div class="model-store-actions">
          <span class="status-pill">${paths.length} paths</span>
          <button class="icon-button" data-initialize-model-store type="button" aria-label="${initializeLabel}" title="${initializeLabel}" ${modelStoreInitializing || modelActionLocked ? "disabled" : ""}>
            <i data-lucide="hard-drive" aria-hidden="true"></i>
          </button>
        </div>
      </div>
      <dl class="model-path-grid">
        ${paths.map(([label, path]) => modelPathDetail(label, path)).join("")}
      </dl>
    </div>
  `;
}

function modelStorePathDetails(modelStore: ModelStoreLayout): Array<[string, string]> {
  return [
    ["Root", modelStore.root],
    ["Manifests", modelStore.manifests],
    ["Weights", modelStore.weights],
    ["Runtimes", modelStore.runtimes],
    ["Cache", modelStore.cache],
    ["Logs", modelStore.logs],
    ["Config", modelStore.config],
  ];
}

function modelRunPlanMarkup(plan: ModelRunPlan | null) {
  if (!plan) {
    return "";
  }

  return `
    <div class="model-run-plan-summary" aria-label="Model run plan">
      <div class="model-run-plan-heading">
        <div>
          <p class="eyebrow">Run plan</p>
          <strong>${escapeHtml(plan.package_id)}</strong>
        </div>
        <span class="status-pill">${escapeHtml(plan.status)}</span>
      </div>
      <dl class="model-path-grid">
        ${modelPathDetail("Adapter", `${plan.adapter} / ${plan.runtime_entry}`)}
        ${modelPathDetail("Runtime", `${plan.runtime_mode} / ${plan.runtime_dir}`)}
        ${modelPathDetail("Adapter manifest", plan.adapter_manifest_path)}
        ${modelPathDetail("Weights", plan.weights_path)}
        ${modelPathDetail("Input", plan.input_path)}
        ${modelPathDetail("Output", plan.output_path)}
        ${plan.params.length > 0 ? modelPathDetail("Params", modelRunParamSummary(plan.params)) : ""}
        ${modelPathDetail("Execution", modelRunExecutionSummary(plan.execution))}
      </dl>
    </div>
  `;
}

function modelChainPlanMarkup(plan: ModelChainPlan | null) {
  if (!plan) {
    return "";
  }

  return `
    <div class="model-run-plan-summary model-chain-plan-summary" aria-label="Model chain plan">
      <div class="model-run-plan-heading">
        <div>
          <p class="eyebrow">Chain plan</p>
          <strong>${escapeHtml(modelChainPackageSummary(plan))}</strong>
        </div>
        <span class="status-pill">${escapeHtml(plan.status)}</span>
      </div>
      <dl class="model-path-grid">
        ${modelPathDetail("Input", `${plan.input} / ${plan.input_path}`)}
        ${modelPathDetail("Output", `${plan.output} / ${plan.output_path}`)}
        ${modelPathDetail("Steps", modelChainStepPlanSummary(plan))}
        ${plan.edges.length > 0 ? modelPathDetail("Edges", modelChainEdgeSummary(plan)) : ""}
        ${modelPathDetail("Execution", modelChainExecutionSummary(plan.execution))}
      </dl>
    </div>
  `;
}

function modelChainDraftMarkup(
  steps: ModelChainDraftStep[],
  planningModelChain: boolean,
  modelActionLocked: boolean,
) {
  if (steps.length === 0) {
    return "";
  }

  const chainLocked = planningModelChain || modelActionLocked;
  const planLabel = planningModelChain
    ? "Planning model chain"
    : modelReadyActionLabel("Plan model chain", modelActionLocked);
  const clearLabel = modelReadyActionLabel("Clear model chain", modelActionLocked);
  return `
    <div class="model-chain-draft" aria-label="Model chain draft">
      <div class="model-subheader">
        <div>
          <p class="eyebrow">Chain</p>
          <h3>Review order</h3>
        </div>
        <div class="model-chain-actions">
          <span class="status-pill">${steps.length} step${steps.length === 1 ? "" : "s"}</span>
          <button class="icon-button model-chain-plan" data-plan-model-chain type="button" aria-label="${planLabel}" title="${planLabel}" ${chainLocked ? "disabled" : ""}>
            <i data-lucide="git-branch" aria-hidden="true"></i>
          </button>
          <button class="icon-button model-chain-clear" data-clear-model-chain type="button" aria-label="${clearLabel}" title="${clearLabel}" ${chainLocked ? "disabled" : ""}>
            <i data-lucide="x" aria-hidden="true"></i>
          </button>
        </div>
      </div>
      <ol class="model-chain-steps">
        ${steps
          .map(
            (step, index) => `
              <li>
                <span>${index + 1}</span>
                <strong title="${escapeHtml(step.packageId)}">${escapeHtml(step.packageId)}</strong>
                ${removeChainStepButton(step, index, chainLocked, modelActionLocked)}
              </li>
            `,
          )
          .join("")}
      </ol>
    </div>
  `;
}

function removeChainStepButton(
  step: ModelChainDraftStep,
  index: number,
  chainLocked: boolean,
  modelActionLocked: boolean,
) {
  const label = modelActionLocked
    ? MODEL_ACTION_LOCKED_LABEL
    : `Remove ${step.packageId} from chain`;
  return `
    <button class="icon-button model-chain-step-remove" data-remove-model-chain-index="${index}" type="button" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}" ${chainLocked ? "disabled" : ""}>
      <i data-lucide="x" aria-hidden="true"></i>
    </button>
  `;
}

function modelRunParamSummary(params: ModelRunPlan["params"]) {
  return params.map((param) => `${param.name}=${String(param.value)}`).join(", ");
}

function modelRunExecutionSummary(execution: ModelRunPlan["execution"]) {
  return `${execution.status}: ${execution.message}`;
}

function modelChainPackageSummary(plan: ModelChainPlan) {
  return plan.steps.map((step) => step.package_id).join(" -> ");
}

function modelChainStepPlanSummary(plan: ModelChainPlan) {
  return plan.steps
    .map((step) => `${step.step_index + 1}:${step.package_id} ${step.input}->${step.output}`)
    .join(", ");
}

function modelChainEdgeSummary(plan: ModelChainPlan) {
  return plan.edges
    .map(
      (edge) =>
        `${edge.from_step_index + 1}->${edge.to_step_index + 1} ${edge.binding}`,
    )
    .join(", ");
}

function modelChainExecutionSummary(execution: ModelChainPlan["execution"]) {
  return `${execution.status}: ${execution.message}`;
}

function modelPathDetail(label: string, value: string) {
  return `
    <div>
      <dt>${escapeHtml(label)}</dt>
      <dd title="${escapeHtml(value)}">${escapeHtml(value)}</dd>
    </div>
  `;
}

function modelTimelineMarkup(events: ModelOperationEvent[]) {
  if (events.length === 0) {
    return "";
  }

  return `
    <div class="event-timeline model-event-timeline" aria-label="Model operation progress">
      ${events.map((event) => `<div class="event-step">${escapeHtml(modelProgressLabel(event))}</div>`).join("")}
    </div>
  `;
}

function catalogPackageCard(
  model: AvailableModelPackage,
  importingCatalogModelId: string | null,
  modelActionLocked: boolean,
) {
  const packageId = model.package.package_id;
  const importing = importingCatalogModelId === packageId;
  return `
    <article class="model-card catalog-model-card">
      <div class="model-card-main">
        <div>
          <strong>${escapeHtml(model.package.name)}</strong>
          <small>${escapeHtml(packageId)} / ${escapeHtml(model.package.publisher)}</small>
        </div>
        <p>${escapeHtml(model.package.description)}</p>
      </div>
      <div class="model-io" aria-label="Model IO">
        <span>${escapeHtml(model.package.input)}</span>
        <i data-lucide="arrow-right" aria-hidden="true"></i>
        <span>${escapeHtml(model.package.output)}</span>
      </div>
      <div class="model-runtime">
        <span>${escapeHtml(model.package.runtime_mode)}</span>
        <small>${escapeHtml(model.runtime_entry)}</small>
      </div>
      <div class="model-weight ${model.weights.cached ? "cached" : "missing"}">
        <span>${model.manifest_cached ? "Manifest local" : "Registry only"}</span>
        <small>${escapeHtml(model.source_name ?? "registry")} / ${escapeHtml(model.weights.format)} / ${escapeHtml(shortDigest(model.weights.sha256))}</small>
        ${importCatalogModelButton(model, importing, modelActionLocked)}
      </div>
      <div class="model-params">
        ${model.params.map(modelParamChip).join("") || `<span class="model-param muted">No params</span>`}
      </div>
    </article>
  `;
}

function modelPackageCard(
  model: CachedModelPackage,
  installingModelId: string | null,
  planningModelId: string | null,
  pullingModelId: string | null,
  removingModelId: string | null,
  runningModelId: string | null,
  chainSteps: ModelChainDraftStep[],
  planningModelChain: boolean,
  modelActionLocked: boolean,
) {
  const packageId = model.package.package_id;
  const installing = installingModelId === packageId;
  const planning = planningModelId === packageId;
  const pulling = pullingModelId === packageId;
  const removing = removingModelId === packageId;
  const running = runningModelId === packageId;
  const chainCount = chainSteps.filter((step) => step.packageId === packageId).length;
  return `
    <article class="model-card">
      <div class="model-card-main">
        <div>
          <strong>${escapeHtml(model.package.name)}</strong>
          <small>${escapeHtml(model.package.package_id)} / ${escapeHtml(model.package.publisher)}</small>
        </div>
        <p>${escapeHtml(model.package.description)}</p>
      </div>
      <div class="model-io" aria-label="Model IO">
        <span>${escapeHtml(model.package.input)}</span>
        <i data-lucide="arrow-right" aria-hidden="true"></i>
        <span>${escapeHtml(model.package.output)}</span>
      </div>
      <div class="model-runtime">
        <span>${escapeHtml(model.package.runtime_mode)}</span>
        <small>${escapeHtml(model.runtime_entry)}</small>
      </div>
      <div class="model-weight ${model.weights.cached ? "cached" : "missing"}">
        <span>${model.weights.cached ? "Weights cached" : "Weights missing"}</span>
        <small>${escapeHtml(model.weights.format)} / ${escapeHtml(shortDigest(model.weights.sha256))}</small>
        ${installModelButton(model, installing, modelActionLocked)}
        ${planRunButton(model, planning, modelActionLocked)}
        ${runModelButton(model, running, modelActionLocked)}
        ${addToChainButton(model, chainCount, planningModelChain, modelActionLocked)}
        ${pullWeightsButton(model, pulling, modelActionLocked)}
        ${removeModelButton(model, removing, modelActionLocked)}
      </div>
      <div class="model-params">
        ${model.params.map(modelParamChip).join("") || `<span class="model-param muted">No params</span>`}
      </div>
    </article>
  `;
}

function importCatalogModelButton(
  model: AvailableModelPackage,
  importing: boolean,
  modelActionLocked: boolean,
) {
  const label = importing
    ? "Adding model"
    : modelReadyActionLabel(
        model.manifest_cached ? "Update manifest" : "Add to local store",
        modelActionLocked,
      );
  return `
    <button class="icon-button model-catalog-import" data-import-catalog-model-name="${escapeHtml(model.package.name)}" data-import-catalog-model-version="${escapeHtml(model.package.version)}" type="button" aria-label="${label}" title="${label}" ${importing || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="file-plus" aria-hidden="true"></i>
    </button>
  `;
}

function installModelButton(
  model: CachedModelPackage,
  installing: boolean,
  modelActionLocked: boolean,
) {
  const label = installing
    ? "Installing model"
    : modelReadyActionLabel("Install model", modelActionLocked);
  return `
    <button class="icon-button model-install" data-install-model-name="${escapeHtml(model.package.name)}" data-install-model-version="${escapeHtml(model.package.version)}" type="button" aria-label="${label}" title="${label}" ${installing || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="package-check" aria-hidden="true"></i>
    </button>
  `;
}

function planRunButton(
  model: CachedModelPackage,
  planning: boolean,
  modelActionLocked: boolean,
) {
  const label = planning
    ? "Planning model run"
    : modelReadyActionLabel("Plan model run", modelActionLocked);
  return `
    <button class="icon-button model-run-plan" data-plan-model-run-name="${escapeHtml(model.package.name)}" data-plan-model-run-version="${escapeHtml(model.package.version)}" type="button" aria-label="${label}" title="${label}" ${planning || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="play" aria-hidden="true"></i>
    </button>
  `;
}

function runModelButton(
  model: CachedModelPackage,
  running: boolean,
  modelActionLocked: boolean,
) {
  const label = running
    ? "Checking execution readiness"
    : modelReadyActionLabel("Check execution readiness", modelActionLocked);
  return `
    <button class="icon-button model-run-check" data-run-model-name="${escapeHtml(model.package.name)}" data-run-model-version="${escapeHtml(model.package.version)}" type="button" aria-label="${label}" title="${label}" ${running || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="terminal" aria-hidden="true"></i>
    </button>
  `;
}

function addToChainButton(
  model: CachedModelPackage,
  chainCount: number,
  planningModelChain: boolean,
  modelActionLocked: boolean,
) {
  const label = planningModelChain
    ? "Planning model chain"
    : modelReadyActionLabel(
        chainCount > 0
          ? `Add another ${model.package.package_id} step to chain`
          : "Add to model chain",
        modelActionLocked,
      );
  return `
    <button class="icon-button model-chain-add" data-add-model-chain-name="${escapeHtml(model.package.name)}" data-add-model-chain-version="${escapeHtml(model.package.version)}" data-add-model-chain-package-id="${escapeHtml(model.package.package_id)}" type="button" aria-label="${escapeHtml(label)}" title="${escapeHtml(label)}" ${planningModelChain || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="list-plus" aria-hidden="true"></i>
    </button>
  `;
}

function removeModelButton(
  model: CachedModelPackage,
  removing: boolean,
  modelActionLocked: boolean,
) {
  const label = removing
    ? "Removing model"
    : modelReadyActionLabel("Remove model", modelActionLocked);
  return `
    <button class="icon-button model-remove" data-remove-model-name="${escapeHtml(model.package.name)}" data-remove-model-version="${escapeHtml(model.package.version)}" type="button" aria-label="${label}" title="${label}" ${removing || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="trash-2" aria-hidden="true"></i>
    </button>
  `;
}

function pullWeightsButton(
  model: CachedModelPackage,
  pulling: boolean,
  modelActionLocked: boolean,
) {
  const label = modelWeightActionLabel(model.weights.cached, pulling, modelActionLocked);
  return `
    <button class="icon-button model-weight-pull" data-pull-model-name="${escapeHtml(model.package.name)}" data-pull-model-version="${escapeHtml(model.package.version)}" type="button" aria-label="${label}" title="${label}" ${pulling || modelActionLocked ? "disabled" : ""}>
      <i data-lucide="download" aria-hidden="true"></i>
    </button>
  `;
}

function modelWeightActionLabel(
  cached: boolean,
  pulling: boolean,
  modelActionLocked: boolean,
) {
  if (pulling) {
    return cached ? "Checking weights" : "Pulling weights";
  }
  return modelReadyActionLabel(
    cached ? "Verify weights" : "Pull weights",
    modelActionLocked,
  );
}

function modelBusyActionLabel(
  busy: boolean,
  busyLabel: string,
  readyLabel: string,
  modelActionLocked: boolean,
) {
  if (busy) {
    return busyLabel;
  }
  return modelReadyActionLabel(readyLabel, modelActionLocked);
}

function modelReadyActionLabel(readyLabel: string, modelActionLocked: boolean) {
  return modelActionLocked ? MODEL_ACTION_LOCKED_LABEL : readyLabel;
}

function modelParamChip(param: ModelParameterSummary) {
  return `
    <span class="model-param" title="${escapeHtml(modelParamTitle(param))}">
      ${escapeHtml(param.name)}:${escapeHtml(param.type)}
    </span>
  `;
}

function modelParamTitle(param: ModelParameterSummary) {
  const parts = [`${param.name} (${param.type})`];
  if (param.values?.length) {
    parts.push(`values: ${param.values.join(", ")}`);
  }
  if (param.min !== null && param.min !== undefined) {
    parts.push(`min: ${param.min}`);
  }
  if (param.max !== null && param.max !== undefined) {
    parts.push(`max: ${param.max}`);
  }
  if (param.default !== null && param.default !== undefined) {
    parts.push(`default: ${param.default}`);
  }
  return parts.join(" / ");
}

function shortDigest(digest: string) {
  return digest.length > 12 ? `${digest.slice(0, 12)}...` : digest;
}

function modelPackageMatchesQuery(model: CachedModelPackage, query: string) {
  return fieldsMatchQuery(modelSearchFields(model), query);
}

function catalogPackageMatchesQuery(model: AvailableModelPackage, query: string) {
  return fieldsMatchQuery(
    [
      model.source_name ?? "",
      model.manifest_path,
      model.manifest_cached ? "local cached imported" : "registry available",
      ...modelSearchFields(model),
    ],
    query,
  );
}

function fieldsMatchQuery(fields: string[], query: string) {
  const terms = query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  if (terms.length === 0) {
    return true;
  }
  const haystack = fields.join("\n").toLowerCase();
  return terms.every((term) => haystack.includes(term));
}

function modelSearchFields(model: CachedModelPackage) {
  return [
    model.package.package_id,
    model.package.name,
    model.package.version,
    model.package.description,
    model.package.publisher,
    model.package.runtime_mode,
    model.package.input,
    model.package.output,
    model.runtime_entry,
    model.weights.source,
    model.weights.sha256,
    model.weights.format,
    ...model.params.flatMap((param) => [
      param.name,
      param.type,
      ...(param.values ?? []),
      param.default === null || param.default === undefined ? "" : String(param.default),
    ]),
  ];
}
