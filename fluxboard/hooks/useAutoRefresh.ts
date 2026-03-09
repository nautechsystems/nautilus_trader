// useAutoRefresh hook - Manage auto-refresh toggle with localStorage persistence

import { useState, useCallback, useEffect } from 'react';

/**
 * Hook to manage auto-refresh toggle with localStorage persistence
 *
 * @param storeName - Unique name for this store/component (used as localStorage key)
 * @param defaultValue - Default value if not found in localStorage (default: true)
 * @returns [autoRefresh, setAutoRefresh] tuple
 *
 * @example
 * ```tsx
 * const [auto, setAuto] = useAutoRefresh('alerts', true);
 *
 * usePolling(loadAlerts, 3000, auto);
 *
 * <input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} />
 * ```
 */
export function useAutoRefresh(
  storeName: string,
  defaultValue: boolean = true
): [boolean, (value: boolean) => void] {
  const storageKey = `${storeName}:auto`;

  // Initialize from localStorage, default to true if not found
  const [auto, setAutoState] = useState<boolean>(() => {
    try {
      const stored = localStorage.getItem(storageKey);
      if (stored === null) return defaultValue;
      return stored !== 'false';
    } catch {
      return defaultValue;
    }
  });

  const setAuto = useCallback((value: boolean) => {
    setAutoState(value);
    try {
      localStorage.setItem(storageKey, String(value));
    } catch (e) {
      if (import.meta.env?.DEV) {
        console.warn(`[useAutoRefresh] Failed to save to localStorage:`, e);
      }
    }
  }, [storageKey]);

  return [auto, setAuto];
}
