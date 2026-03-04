/**
 * useBoundedSave - Manage save operations with bounded concurrency
 *
 * Handles saving multiple strategies with a concurrency limit to avoid
 * overwhelming the backend API with simultaneous requests.
 */

import { useState, useCallback, useRef } from 'react';

export interface SaveProgress {
  completed: number;
  failed: number;
  total: number;
}

export interface BoundedSaveResult {
  /** Currently saving strategies */
  saving: Set<string>;
  /** Save progress (null when not saving) */
  progress: SaveProgress | null;
  /** Check if a strategy is currently saving */
  isSaving: (strategyId: string) => boolean;
  /** Execute saves with bounded concurrency */
  executeSaves: <T>(
    items: T[],
    saveFn: (item: T) => Promise<void>,
    options?: { maxConcurrency?: number }
  ) => Promise<{ successful: T[]; failed: Array<{ item: T; error: string }> }>;
  /** Mark strategy as saving */
  markSaving: (strategyId: string) => void;
  /** Mark strategy as done saving */
  markDone: (strategyId: string) => void;
  /** Reset all state */
  reset: () => void;
}

/**
 * Hook to manage bounded concurrent saves.
 *
 * Usage:
 * ```ts
 * const saves = useBoundedSave();
 *
 * // Execute saves with max 5 concurrent
 * const result = await saves.executeSaves(
 *   updates,
 *   async (update) => api.patchStrategyParams(update.strategy_id, update.params),
 *   { maxConcurrency: 5 }
 * );
 *
 * console.log(`${result.successful.length} saved, ${result.failed.length} failed`);
 * ```
 */
export function useBoundedSave(): BoundedSaveResult {
  const [saving, setSaving] = useState<Set<string>>(new Set());
  const [progress, setProgress] = useState<SaveProgress | null>(null);
  const savingRef = useRef(saving);

  // Keep ref in sync
  savingRef.current = saving;

  const isSaving = useCallback(
    (strategyId: string): boolean => {
      return saving.has(strategyId);
    },
    [saving]
  );

  const markSaving = useCallback((strategyId: string) => {
    setSaving((prev) => {
      const next = new Set(prev);
      next.add(strategyId);
      return next;
    });
  }, []);

  const markDone = useCallback((strategyId: string) => {
    setSaving((prev) => {
      const next = new Set(prev);
      next.delete(strategyId);
      return next;
    });
  }, []);

  const reset = useCallback(() => {
    setSaving(new Set());
    setProgress(null);
  }, []);

  const executeSaves = useCallback(
    async <T,>(
      items: T[],
      saveFn: (item: T) => Promise<void>,
      options?: { maxConcurrency?: number }
    ): Promise<{ successful: T[]; failed: Array<{ item: T; error: string }> }> => {
      const maxConcurrency = options?.maxConcurrency ?? 5;
      const successful: T[] = [];
      const failed: Array<{ item: T; error: string }> = [];
      let completed = 0;
      let failedCount = 0;

      setProgress({ completed: 0, failed: 0, total: items.length });

      // Process items with bounded concurrency
      const processBatch = async (batch: T[]): Promise<void> => {
        await Promise.all(
          batch.map(async (item) => {
            try {
              await saveFn(item);
              successful.push(item);
              completed += 1;
            } catch (error) {
              const errorMsg = error instanceof Error ? error.message : String(error);
              failed.push({ item, error: errorMsg });
              failedCount += 1;
            } finally {
              // Update progress
              setProgress({ completed, failed: failedCount, total: items.length });
            }
          })
        );
      };

      // Split items into batches
      for (let i = 0; i < items.length; i += maxConcurrency) {
        const batch = items.slice(i, i + maxConcurrency);
        await processBatch(batch);
      }

      // Clear progress when done
      setProgress(null);

      return { successful, failed };
    },
    []
  );

  return {
    saving,
    progress,
    isSaving,
    executeSaves,
    markSaving,
    markDone,
    reset,
  };
}
