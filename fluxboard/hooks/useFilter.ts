/**
 * useFilter - Generic data filtering hook
 *
 * Provides flexible filtering capabilities with multiple operators.
 * Features:
 * - Multiple filter conditions (AND logic by default)
 * - Various operators: equals, contains, gt, lt, gte, lte, between
 * - Type-safe filter values
 * - Memoized filtering for performance
 *
 * @example
 * ```tsx
 * const { filteredData, filters, setFilter, clearFilters, removeFilter } = useFilter(data);
 *
 * setFilter('age', 'gte', 18);
 * setFilter('name', 'contains', 'alice');
 * ```
 */

import { useState, useMemo, useCallback } from 'react';

export type FilterOperator =
  | 'equals'
  | 'notEquals'
  | 'contains'
  | 'notContains'
  | 'startsWith'
  | 'endsWith'
  | 'gt'
  | 'gte'
  | 'lt'
  | 'lte'
  | 'between'
  | 'in'
  | 'notIn';

export interface FilterCondition<T = any> {
  key: string;
  operator: FilterOperator;
  value: T;
  /** For 'between' operator: [min, max] */
  value2?: T;
}

export interface UseFilterOptions {
  /** Use OR logic instead of AND for multiple filters */
  useOrLogic?: boolean;
  /** Case-sensitive string comparisons (default: false) */
  caseSensitive?: boolean;
}

export interface UseFilterReturn<T> {
  /** Filtered data array */
  filteredData: T[];
  /** Current filter conditions */
  filters: FilterCondition[];
  /** Add or update a filter */
  setFilter: (key: string, operator: FilterOperator, value: any, value2?: any) => void;
  /** Remove a specific filter by key */
  removeFilter: (key: string) => void;
  /** Clear all filters */
  clearFilters: () => void;
  /** Check if any filters are active */
  hasFilters: boolean;
}

/**
 * Generic filtering hook for data arrays
 *
 * @param data - Array of data to filter
 * @param options - Configuration options
 * @returns Filtered data and filter control functions
 */
export function useFilter<T extends Record<string, any>>(
  data: T[],
  options: UseFilterOptions = {}
): UseFilterReturn<T> {
  const { useOrLogic = false, caseSensitive = false } = options;

  const [filters, setFilters] = useState<FilterCondition[]>([]);

  const setFilter = useCallback(
    (key: string, operator: FilterOperator, value: any, value2?: any) => {
      setFilters((prev) => {
        // Remove existing filter for this key
        const filtered = prev.filter((f) => f.key !== key);
        // Add new filter
        return [...filtered, { key, operator, value, value2 }];
      });
    },
    []
  );

  const removeFilter = useCallback((key: string) => {
    setFilters((prev) => prev.filter((f) => f.key !== key));
  }, []);

  const clearFilters = useCallback(() => {
    setFilters([]);
  }, []);

  /**
   * Apply a single filter condition to a data item
   */
  const matchesFilter = useCallback(
    (item: T, filter: FilterCondition): boolean => {
      const itemValue = item[filter.key];

      // Handle null/undefined
      if (itemValue == null) {
        return filter.operator === 'notEquals' || filter.operator === 'notIn';
      }

      const getValue = (val: any) => {
        if (!caseSensitive && typeof val === 'string') {
          return val.toLowerCase();
        }
        return val;
      };

      const normItemValue = getValue(itemValue);
      const normFilterValue = getValue(filter.value);

      switch (filter.operator) {
        case 'equals':
          return normItemValue === normFilterValue;

        case 'notEquals':
          return normItemValue !== normFilterValue;

        case 'contains':
          return String(normItemValue).includes(String(normFilterValue));

        case 'notContains':
          return !String(normItemValue).includes(String(normFilterValue));

        case 'startsWith':
          return String(normItemValue).startsWith(String(normFilterValue));

        case 'endsWith':
          return String(normItemValue).endsWith(String(normFilterValue));

        case 'gt':
          return Number(itemValue) > Number(filter.value);

        case 'gte':
          return Number(itemValue) >= Number(filter.value);

        case 'lt':
          return Number(itemValue) < Number(filter.value);

        case 'lte':
          return Number(itemValue) <= Number(filter.value);

        case 'between': {
          const num = Number(itemValue);
          const min = Number(filter.value);
          const max = Number(filter.value2);
          return num >= min && num <= max;
        }

        case 'in':
          if (!Array.isArray(filter.value)) return false;
          return filter.value.some((v) => getValue(v) === normItemValue);

        case 'notIn':
          if (!Array.isArray(filter.value)) return true;
          return !filter.value.some((v) => getValue(v) === normItemValue);

        default:
          return true;
      }
    },
    [caseSensitive]
  );

  /**
   * Memoized filtered data - only recompute when data or filters change
   */
  const filteredData = useMemo(() => {
    if (filters.length === 0) {
      return data;
    }

    return data.filter((item) => {
      if (useOrLogic) {
        // OR logic: item matches if ANY filter condition is true
        return filters.some((filter) => matchesFilter(item, filter));
      } else {
        // AND logic: item matches if ALL filter conditions are true
        return filters.every((filter) => matchesFilter(item, filter));
      }
    });
  }, [data, filters, useOrLogic, matchesFilter]);

  return {
    filteredData,
    filters,
    setFilter,
    removeFilter,
    clearFilters,
    hasFilters: filters.length > 0,
  };
}
