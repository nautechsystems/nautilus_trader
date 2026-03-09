export type StrategyParams = Record<string, string>;
export type StrategyParamsMap = Map<string, StrategyParams>;
export type DirtyParamMap = Map<string, Set<string>>;

export type RemoteChangeDiff = {
  remoteUpdated: Set<string>;
  conflictingDirty: Map<string, Set<string>>;
};

const isEqual = (a?: string, b?: string) => (a ?? '') === (b ?? '');

function collectChangedKeys(prevParams: StrategyParams | undefined, nextParams: StrategyParams): string[] {
  const keys = new Set<string>([
    ...Object.keys(prevParams || {}),
    ...Object.keys(nextParams || {}),
  ]);
  const changed: string[] = [];
  keys.forEach((key) => {
    if (!isEqual(prevParams?.[key], nextParams[key])) {
      changed.push(key);
    }
  });
  return changed;
}

export function diffRemoteChanges(
  previousOriginals: StrategyParamsMap,
  nextOriginals: StrategyParamsMap,
  dirtyMap: DirtyParamMap,
): RemoteChangeDiff {
  const remoteUpdated = new Set<string>();
  const conflictingDirty = new Map<string, Set<string>>();

  nextOriginals.forEach((nextParams, strategyId) => {
    const prevParams = previousOriginals.get(strategyId);
    if (!prevParams) {
      remoteUpdated.add(strategyId);
      return;
    }
    const changedKeys = collectChangedKeys(prevParams, nextParams);
    if (changedKeys.length === 0) {
      return;
    }
    const dirtyKeys = dirtyMap.get(strategyId);
    if (!dirtyKeys || dirtyKeys.size === 0) {
      remoteUpdated.add(strategyId);
      return;
    }
    const overlapping = changedKeys.filter((key) => dirtyKeys.has(key));
    if (overlapping.length > 0) {
      conflictingDirty.set(strategyId, new Set(overlapping));
    } else {
      remoteUpdated.add(strategyId);
    }
  });

  return { remoteUpdated, conflictingDirty };
}

export function computeConflictsFromLocal(
  originals: StrategyParamsMap,
  localValues: StrategyParamsMap,
  dirtyMap: DirtyParamMap,
): Map<string, Set<string>> {
  const conflicts = new Map<string, Set<string>>();

  dirtyMap.forEach((dirtyKeys, strategyId) => {
    const local = localValues.get(strategyId);
    const remote = originals.get(strategyId);
    if (!local || !remote) return;
    const conflictKeys = Array.from(dirtyKeys).filter((key) => !isEqual(local[key], remote[key]));
    if (conflictKeys.length > 0) {
      conflicts.set(strategyId, new Set(conflictKeys));
    }
  });

  return conflicts;
}
