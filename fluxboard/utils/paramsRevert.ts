import type { ValidationErrors } from '../types';

export type StrategyParams = Record<string, string>;

export function revertParamValues(
  strategyIds: string[],
  originalValues: Map<string, StrategyParams>,
  currentValues: Map<string, StrategyParams>
): Map<string, StrategyParams> {
  if (strategyIds.length === 0) {
    return currentValues;
  }
  const next = new Map(currentValues);
  strategyIds.forEach((id) => {
    const original = originalValues.get(id);
    if (!original) return;
    next.set(id, { ...original });
  });
  return next;
}

export function clearDirtyForStrategies(
  strategyIds: string[],
  dirtyMap: Map<string, Set<string>>
): Map<string, Set<string>> {
  if (strategyIds.length === 0) return dirtyMap;
  const next = new Map(dirtyMap);
  strategyIds.forEach((id) => next.delete(id));
  return next;
}

export function clearErrorsForStrategies(
  strategyIds: string[],
  errorMap: Map<string, ValidationErrors>
): Map<string, ValidationErrors> {
  if (strategyIds.length === 0) return errorMap;
  const next = new Map(errorMap);
  strategyIds.forEach((id) => next.delete(id));
  return next;
}
