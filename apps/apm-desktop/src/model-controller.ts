import {
  chooseModelManifestPathCommand,
  chooseModelRunInputPathCommand,
  chooseModelRunOutputPathCommand,
  initializeModelStoreCommand,
  importModelCatalogPackageCommand,
  installModelPackageCommand,
  importModelManifestCommand,
  planModelChainCommand,
  planModelRunCommand,
  pullModelWeightsCommand,
  removeModelPackageCommand,
  runModelCommand,
} from "./commands";
import { modelActionActive } from "./model-action-lock";
import type { ModelChainPlan, ModelRunPlan } from "./types";
import type { LifecycleNotice, ModelChainDraftStep } from "./view-model";

export type ModelControllerState = {
  modelNotice: LifecycleNotice | null;
  modelRunPlan: ModelRunPlan | null;
  modelChainPlan: ModelChainPlan | null;
  modelChainSteps: ModelChainDraftStep[];
  planningModelChain: boolean;
  modelImporting: boolean;
  modelStoreInitializing: boolean;
  importingCatalogModelId: string | null;
  installingModelId: string | null;
  planningModelId: string | null;
  pullingModelId: string | null;
  removingModelId: string | null;
  runningModelId: string | null;
  modelSearchQuery: string;
};

type ModelControllerHost = {
  refreshSnapshotData(): Promise<void>;
  modelOperationActive(): boolean;
  runModelOperation<T>(run: (progressId?: string) => Promise<T>): Promise<T>;
  clearModelEvents(): void;
  formatError(error: unknown): string;
  render(): void;
};

export function createModelController(host: ModelControllerHost) {
  let modelNotice: LifecycleNotice | null = null;
  let modelRunPlan: ModelRunPlan | null = null;
  let modelChainPlan: ModelChainPlan | null = null;
  let modelChainSteps: ModelChainDraftStep[] = [];
  let planningModelChain = false;
  let modelImporting = false;
  let modelStoreInitializing = false;
  let importingCatalogModelId: string | null = null;
  let installingModelId: string | null = null;
  let planningModelId: string | null = null;
  let pullingModelId: string | null = null;
  let removingModelId: string | null = null;
  let runningModelId: string | null = null;
  let modelSearchQuery = "";

  function state(): ModelControllerState {
    return {
      modelNotice,
      modelRunPlan,
      modelChainPlan,
      modelChainSteps,
      planningModelChain,
      modelImporting,
      modelStoreInitializing,
      importingCatalogModelId,
      installingModelId,
      planningModelId,
      pullingModelId,
      removingModelId,
      runningModelId,
      modelSearchQuery,
    };
  }

  function setModelSearchQuery(query: string) {
    if (modelSearchQuery === query) {
      return;
    }
    modelSearchQuery = query;
    host.render();
  }

  function setModelNotice(notice: LifecycleNotice) {
    modelNotice = notice;
  }

  function modelActionLocked() {
    return modelActionActive({
      modelOperationActive: host.modelOperationActive(),
      modelStoreInitializing,
      modelImporting,
      importingCatalogModelId,
      installingModelId,
      planningModelId,
      pullingModelId,
      removingModelId,
      runningModelId,
      planningModelChain,
    });
  }

  async function initializeModelStore() {
    if (modelActionLocked()) {
      return;
    }

    modelNotice = { tone: "info", message: "Initializing model store" };
    modelStoreInitializing = true;
    host.render();

    try {
      const result = await initializeModelStoreCommand();
      modelNotice = {
        tone: "success",
        message: `Model store ready at ${result.layout.root}.`,
      };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      modelStoreInitializing = false;
      host.render();
    }
  }

  async function importModelManifest() {
    if (modelActionLocked()) {
      return;
    }

    modelNotice = null;
    modelRunPlan = null;
    modelChainPlan = null;
    modelImporting = true;
    host.render();

    try {
      const manifestPath = await chooseModelManifestPathCommand();
      if (!manifestPath) {
        modelNotice = { tone: "info", message: "Manifest import canceled." };
        return;
      }

      const result = await importModelManifestCommand(manifestPath);
      modelNotice = {
        tone: "success",
        message: `${result.model.package.package_id} ${result.replaced ? "updated" : "cached"}.`,
      };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      modelImporting = false;
      host.render();
    }
  }

  async function importModelCatalogPackage(name: string, version: string) {
    if (modelActionLocked()) {
      return;
    }

    const packageId = `${name}@${version}`;
    modelRunPlan = null;
    modelChainPlan = null;
    modelNotice = { tone: "info", message: `Adding ${packageId} to the local model store` };
    importingCatalogModelId = packageId;
    host.render();

    try {
      const result = await importModelCatalogPackageCommand(name, version);
      modelNotice = {
        tone: "success",
        message: `${result.model.package.package_id} ${result.replaced ? "updated" : "cached"}.`,
      };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      importingCatalogModelId = null;
      host.render();
    }
  }

  async function pullModelWeights(name: string, version: string) {
    if (modelActionLocked()) {
      return;
    }

    const packageId = `${name}@${version}`;
    host.clearModelEvents();
    modelRunPlan = null;
    modelChainPlan = null;
    modelNotice = { tone: "info", message: `Pulling weights for ${packageId}` };
    pullingModelId = packageId;
    host.render();

    try {
      const result = await host.runModelOperation((progressId) =>
        pullModelWeightsCommand(name, version, progressId),
      );
      if (result.status === "failed") {
        modelNotice = { tone: "error", message: result.error };
        return;
      }
      modelNotice = {
        tone: "success",
        message: `${result.result.package_id} weights ${result.result.status}.`,
      };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      pullingModelId = null;
      host.render();
    }
  }

  async function installModelPackage(name: string, version: string) {
    if (modelActionLocked()) {
      return;
    }

    const packageId = `${name}@${version}`;
    host.clearModelEvents();
    modelRunPlan = null;
    modelChainPlan = null;
    modelNotice = { tone: "info", message: `Installing ${packageId}` };
    installingModelId = packageId;
    host.render();

    try {
      const result = await host.runModelOperation((progressId) =>
        installModelPackageCommand(name, version, progressId),
      );
      if (result.status === "failed") {
        modelNotice = { tone: "error", message: result.error };
        return;
      }
      modelNotice = {
        tone: "success",
        message: `${result.result.package_id} ready (${result.result.runtime.adapter} ${result.result.runtime.status}).`,
      };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      installingModelId = null;
      host.render();
    }
  }

  async function removeModelPackage(name: string, version: string) {
    if (modelActionLocked()) {
      return;
    }

    const packageId = `${name}@${version}`;
    modelRunPlan = null;
    modelChainPlan = null;
    modelNotice = { tone: "info", message: `Removing ${packageId}` };
    removingModelId = packageId;
    host.render();

    try {
      const result = await removeModelPackageCommand(name, version);
      if (result.status === "not_cached") {
        modelChainSteps = modelChainSteps.filter((step) => step.packageId !== result.package_id);
        modelNotice = { tone: "info", message: `${result.package_id} was not cached.` };
        return;
      }
      modelChainSteps = modelChainSteps.filter((step) => step.packageId !== result.package_id);
      modelNotice = {
        tone: "success",
        message: `${result.package_id} removed${result.removed_weight ? " with weights" : ""}.`,
      };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      removingModelId = null;
      host.render();
    }
  }

  async function planModelRun(name: string, version: string) {
    if (modelActionLocked()) {
      return;
    }

    const packageId = `${name}@${version}`;
    modelRunPlan = null;
    modelChainPlan = null;
    planningModelId = packageId;
    modelNotice = { tone: "info", message: `Planning run for ${packageId}` };
    host.render();

    try {
      const paths = await chooseModelRunPaths("Run planning canceled.");
      if (!paths) {
        return;
      }

      const plan = await planModelRunCommand(
        name,
        version,
        paths.inputPath,
        paths.outputPath,
      );
      modelRunPlan = plan;
      modelNotice = {
        tone: "success",
        message: `${plan.package_id} run planned (${plan.adapter} -> ${plan.output_path}).`,
      };
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      planningModelId = null;
      host.render();
    }
  }

  async function runModel(name: string, version: string) {
    if (modelActionLocked()) {
      return;
    }

    const packageId = `${name}@${version}`;
    host.clearModelEvents();
    modelRunPlan = null;
    modelChainPlan = null;
    runningModelId = packageId;
    modelNotice = { tone: "info", message: `Checking execution readiness for ${packageId}` };
    host.render();

    try {
      const paths = await chooseModelRunPaths("Run check canceled.");
      if (!paths) {
        return;
      }

      const result = await host.runModelOperation((progressId) =>
        runModelCommand(
          name,
          version,
          paths.inputPath,
          paths.outputPath,
          progressId,
        ),
      );
      if (result.status === "blocked") {
        modelRunPlan = result.result.plan;
        modelNotice = {
          tone: "info",
          message: `${result.result.package_id} execution blocked: ${result.result.message}`,
        };
        await host.refreshSnapshotData();
        return;
      }

      if (result.status === "completed") {
        modelRunPlan = result.result.plan;
        modelNotice = {
          tone: "success",
          message: `${result.result.package_id} completed: ${result.result.message}`,
        };
        await host.refreshSnapshotData();
        return;
      }

      modelNotice = { tone: "error", message: result.error };
      await host.refreshSnapshotData();
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      runningModelId = null;
      host.render();
    }
  }

  async function chooseModelRunPaths(cancelMessage: string) {
    const inputPath = await chooseModelRunInputPathCommand();
    if (!inputPath) {
      modelNotice = { tone: "info", message: cancelMessage };
      return null;
    }

    const outputPath = await chooseModelRunOutputPathCommand();
    if (!outputPath) {
      modelNotice = { tone: "info", message: cancelMessage };
      return null;
    }

    return { inputPath, outputPath };
  }

  function addModelChainStep(name: string, version: string, packageId: string) {
    if (modelActionLocked()) {
      return;
    }
    modelRunPlan = null;
    modelChainPlan = null;
    modelChainSteps = [
      ...modelChainSteps,
      { name, version, packageId },
    ];
    modelNotice = null;
    host.render();
  }

  function removeModelChainStep(stepIndex: number) {
    if (modelActionLocked()) {
      return;
    }
    if (stepIndex < 0 || stepIndex >= modelChainSteps.length) {
      return;
    }
    modelChainPlan = null;
    modelChainSteps = modelChainSteps.filter((_, index) => index !== stepIndex);
    host.render();
  }

  function clearModelChain() {
    if (modelActionLocked()) {
      return;
    }
    if (modelChainSteps.length === 0 && modelChainPlan === null) {
      return;
    }
    modelChainSteps = [];
    modelChainPlan = null;
    host.render();
  }

  async function planModelChain() {
    if (modelActionLocked()) {
      return;
    }

    if (modelChainSteps.length === 0) {
      modelNotice = { tone: "info", message: "Choose at least one cached model for the chain." };
      host.render();
      return;
    }

    const requestedSteps = [...modelChainSteps];
    modelRunPlan = null;
    modelChainPlan = null;
    planningModelChain = true;
    modelNotice = {
      tone: "info",
      message: `Planning ${requestedSteps.length} step model chain`,
    };
    host.render();

    try {
      const inputPath = await chooseModelRunInputPathCommand();
      if (!inputPath) {
        modelNotice = { tone: "info", message: "Chain planning canceled." };
        return;
      }

      const outputPath = await chooseModelRunOutputPathCommand();
      if (!outputPath) {
        modelNotice = { tone: "info", message: "Chain planning canceled." };
        return;
      }

      const plan = await planModelChainCommand({
        input_path: inputPath,
        output_path: outputPath,
        steps: requestedSteps.map((step) => ({
          name: step.name,
          version: step.version,
        })),
      });
      modelChainPlan = plan;
      modelNotice = {
        tone: "success",
        message: `${plan.steps.length} step chain planned (${plan.input} -> ${plan.output}).`,
      };
    } catch (error) {
      modelNotice = { tone: "error", message: host.formatError(error) };
    } finally {
      planningModelChain = false;
      host.render();
    }
  }

  return {
    addModelChainStep,
    clearModelChain,
    initializeModelStore,
    importModelCatalogPackage,
    importModelManifest,
    installModelPackage,
    planModelChain,
    planModelRun,
    pullModelWeights,
    removeModelPackage,
    removeModelChainStep,
    runModel,
    setModelNotice,
    setModelSearchQuery,
    state,
  };
}
