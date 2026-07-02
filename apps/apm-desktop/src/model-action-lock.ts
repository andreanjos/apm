export type ModelActionLockState = {
  modelOperationActive: boolean;
  modelStoreInitializing: boolean;
  modelImporting: boolean;
  importingCatalogModelId: string | null;
  installingModelId: string | null;
  planningModelId: string | null;
  pullingModelId: string | null;
  removingModelId: string | null;
  runningModelId: string | null;
  planningModelChain: boolean;
};

export type ModelActionViewState = Omit<ModelActionLockState, "modelOperationActive"> & {
  modelOperation: unknown | null;
};

export function modelActionActive(state: ModelActionLockState) {
  return (
    state.modelOperationActive ||
    state.modelStoreInitializing ||
    state.modelImporting ||
    state.importingCatalogModelId !== null ||
    state.installingModelId !== null ||
    state.planningModelId !== null ||
    state.pullingModelId !== null ||
    state.removingModelId !== null ||
    state.runningModelId !== null ||
    state.planningModelChain
  );
}

export function modelActionActiveForView(state: ModelActionViewState) {
  return modelActionActive({
    modelOperationActive: state.modelOperation !== null,
    modelStoreInitializing: state.modelStoreInitializing,
    modelImporting: state.modelImporting,
    importingCatalogModelId: state.importingCatalogModelId,
    installingModelId: state.installingModelId,
    planningModelId: state.planningModelId,
    pullingModelId: state.pullingModelId,
    removingModelId: state.removingModelId,
    runningModelId: state.runningModelId,
    planningModelChain: state.planningModelChain,
  });
}
