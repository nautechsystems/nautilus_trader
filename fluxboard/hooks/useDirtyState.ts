/**
 * useDirtyState - Track dirty params across strategies
 *
 * Manages dirty state for parameter editing with per-strategy, per-param granularity.
 * Compares current values against originals to determine dirty status.
 */

import { useState, useCallback, useMemo } from 'react';

export interface DirtyStateResult {
  /** Map of strategy ID -> Set of dirty param keys */
  dirtyParams: Map<string, Set<string>>;
  /** Total number of strategies with dirty params */
  dirtyCount: number;
  /** Check if a specific param is dirty */
  isDirty: (strategyId: string, paramKey: string) => boolean;
  /** Get all dirty param keys for a strategy */
  getDirtyKeys: (strategyId: string) => Set<string>;
  /** Mark param as dirty or clean based on comparison with original */
  markDirty: (strategyId: string, paramKey: string, currentValue: string, originalValue: string) => void;
  /** Clear all dirty params for a strategy */
  clearDirty: (strategyId: string) => void;
  /** Clear specific dirty param */
  clearParam: (strategyId: string, paramKey: string) => void;
  /** Reset all dirty state */
  resetAll: () => void;
}

/**
 * Hook to manage dirty parameter state.
 *
 * Usage:
 * ```ts
 * const dirty = useDirtyState();
 *
 * // Mark dirty on change
 * dirty.markDirty('strategy1', 'qty', '100', '50'); // dirty
 * dirty.markDirty('strategy1', 'qty', '50', '50');  // clean
 *
 * // Check dirty
 * dirty.isDirty('strategy1', 'qty'); // false
 *
 * // Clear on save
 * dirty.clearDirty('strategy1');
 * ```
 */
export function useDirtyState(): DirtyStateResult {
  const [dirtyParams, setDirtyParams] = useState<Map<string, Set<string>>>(new Map());

  const dirtyCount = useMemo(() => dirtyParams.size, [dirtyParams]);

  const isDirty = useCallback(
    (strategyId: string, paramKey: string): boolean => {
      return dirtyParams.get(strategyId)?.has(paramKey) ?? false;
    },
    [dirtyParams]
  );

  const getDirtyKeys = useCallback(
    (strategyId: string): Set<string> => {
      return dirtyParams.get(strategyId) ?? new Set();
    },
    [dirtyParams]
  );

  const markDirty = useCallback(
    (strategyId: string, paramKey: string, currentValue: string, originalValue: string) => {
      setDirtyParams((prev) => {
        const newMap = new Map(prev);
        const stratDirty = new Set(newMap.get(strategyId) || []);

        if (currentValue !== originalValue) {
          stratDirty.add(paramKey);
        } else {
          stratDirty.delete(paramKey);
        }

        if (stratDirty.size > 0) {
          newMap.set(strategyId, stratDirty);
        } else {
          newMap.delete(strategyId);
        }

        return newMap;
      });
    },
    []
  );

  const clearDirty = useCallback((strategyId: string) => {
    setDirtyParams((prev) => {
      const newMap = new Map(prev);
      newMap.delete(strategyId);
      return newMap;
    });
  }, []);

  const clearParam = useCallback((strategyId: string, paramKey: string) => {
    setDirtyParams((prev) => {
      const newMap = new Map(prev);
      const stratDirty = newMap.get(strategyId);
      if (stratDirty) {
        stratDirty.delete(paramKey);
        if (stratDirty.size === 0) {
          newMap.delete(strategyId);
        } else {
          newMap.set(strategyId, stratDirty);
        }
      }
      return newMap;
    });
  }, []);

  const resetAll = useCallback(() => {
    setDirtyParams(new Map());
  }, []);

  return {
    dirtyParams,
    dirtyCount,
    isDirty,
    getDirtyKeys,
    markDirty,
    clearDirty,
    clearParam,
    resetAll,
  };
}
