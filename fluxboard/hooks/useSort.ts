/**
 * useSort - Generic table sorting hook
 *
 * Extracts common sorting logic from multiple table components.
 * Features:
 * - Toggle sort direction: asc → desc → neutral (original order)
 * - Generic type support for any data structure
 * - Memoized sorting for performance
 * - Stable sort key references
 *
 * @example
 * ```tsx
 * const { sortedData, sortColumn, sortDirection, handleSort } = useSort(
 *   data,
 *   'name',
 *   'asc'
 * );
 * ```
 */

import { useState, useMemo, useCallback } from 'react';

export type SortDirection = 'asc' | 'desc' | null;

export interface UseSortOptions<T, K extends keyof T = keyof T> {
  /** Initial sort column */
  defaultColumn?: K;
  /** Initial sort direction */
  defaultDirection?: SortDirection;
  /** Custom comparator function for complex sorting */
  compareFn?: (a: T, b: T, column: K) => number;
}

export interface UseSortReturn<T, K extends keyof T = keyof T> {
  /** Sorted data array */
  sortedData: T[];
  /** Current sort column */
  sortColumn: K | null;
  /** Current sort direction */
  sortDirection: SortDirection;
  /** Function to handle column sort */
  handleSort: (column: K) => void;
  /** Function to reset sort to original order */
  resetSort: () => void;
}

/**
 * Generic sorting hook for table data
 *
 * @param data - Array of data to sort
 * @param options - Configuration options
 * @returns Sorted data and sort control functions
 */
export function useSort<T, K extends keyof T = keyof T>(
  data: T[],
  options: UseSortOptions<T, K> = {}
): UseSortReturn<T, K> {
  const { defaultColumn, defaultDirection = 'asc', compareFn } = options;

  const [sortColumn, setSortColumn] = useState<K | null>(defaultColumn ?? null);
  const [sortDirection, setSortDirection] = useState<SortDirection>(
    defaultColumn ? defaultDirection : null
  );

  /**
   * Handle sort column click
   * Logic: same column toggles asc → desc → null (original order)
   *        different column resets to asc
   */
  const handleSort = useCallback((column: K) => {
    if (sortColumn === column) {
      // Same column: toggle direction
      if (sortDirection === 'asc') {
        setSortDirection('desc');
      } else if (sortDirection === 'desc') {
        setSortDirection(null);
        setSortColumn(null);
      } else {
        setSortDirection('asc');
        setSortColumn(column);
      }
    } else {
      // Different column: reset to ascending
      setSortColumn(column);
      setSortDirection('asc');
    }
  }, [sortColumn, sortDirection]);

  const resetSort = useCallback(() => {
    setSortColumn(null);
    setSortDirection(null);
  }, []);

  /**
   * Memoized sorted data - only recompute when data or sort params change
   */
  const sortedData = useMemo(() => {
    if (!sortColumn || !sortDirection) {
      return data;
    }

    return [...data].sort((a, b) => {
      // Use custom comparator if provided
      if (compareFn) {
        const result = compareFn(a, b, sortColumn);
        return sortDirection === 'asc' ? result : -result;
      }

      // Default comparison logic
      const aVal = a[sortColumn];
      const bVal = b[sortColumn];

      // Handle null/undefined
      if (aVal == null && bVal == null) return 0;
      if (aVal == null) return 1;
      if (bVal == null) return -1;

      // Numeric comparison
      if (typeof aVal === 'number' && typeof bVal === 'number') {
        return sortDirection === 'asc' ? aVal - bVal : bVal - aVal;
      }

      // String comparison (case-insensitive)
      const aStr = String(aVal).toLowerCase();
      const bStr = String(bVal).toLowerCase();
      const comparison = aStr.localeCompare(bStr);
      return sortDirection === 'asc' ? comparison : -comparison;
    });
  }, [data, sortColumn, sortDirection, compareFn]);

  return {
    sortedData,
    sortColumn,
    sortDirection,
    handleSort,
    resetSort,
  };
}
