/**
 * useTableState - Integrated table state management hook
 *
 * Combines sorting, filtering, and pagination into a single unified hook.
 * Features:
 * - Proper dependency management (filters → sort → paginate pipeline)
 * - Automatic page reset when filters/sort change
 * - Memoized data transformations
 * - Performance-optimized re-render control
 *
 * Data Flow:
 * Raw Data → Filter → Sort → Paginate → Display
 *
 * @example
 * ```tsx
 * const {
 *   displayData,
 *   sorting,
 *   filtering,
 *   pagination,
 *   totalFilteredItems
 * } = useTableState(data, {
 *   initialSort: { column: 'name', direction: 'asc' },
 *   initialPageSize: 25
 * });
 * ```
 */

import { useMemo } from 'react';
import { useSort, type UseSortOptions, type SortDirection } from './useSort';
import { useFilter, type UseFilterOptions, type FilterOperator } from './useFilter';
import { usePagination, type UsePaginationOptions } from './usePagination';

export interface UseTableStateOptions<T, K extends keyof T = keyof T> {
  /** Sort configuration */
  sort?: {
    /** Initial column to sort by */
    defaultColumn?: K;
    /** Initial sort direction */
    defaultDirection?: SortDirection;
    /** Custom comparator function */
    compareFn?: (a: T, b: T, column: K) => number;
  };
  /** Filter configuration */
  filter?: {
    /** Use OR logic instead of AND */
    useOrLogic?: boolean;
    /** Case-sensitive string comparisons */
    caseSensitive?: boolean;
  };
  /** Pagination configuration */
  pagination?: {
    /** Initial page number (1-indexed) */
    initialPage?: number;
    /** Initial page size */
    initialPageSize?: number;
    /** Available page size options */
    pageSizeOptions?: number[];
  };
  /** Enable/disable features */
  features?: {
    sorting?: boolean;
    filtering?: boolean;
    pagination?: boolean;
  };
}

export interface UseTableStateReturn<T, K extends keyof T = keyof T> {
  /** Final data to display (after all transformations) */
  displayData: T[];
  /** Sorting state and controls */
  sorting: {
    sortedData: T[];
    sortColumn: K | null;
    sortDirection: SortDirection;
    handleSort: (column: K) => void;
    resetSort: () => void;
  };
  /** Filtering state and controls */
  filtering: {
    filteredData: T[];
    filters: Array<{ key: string; operator: FilterOperator; value: any; value2?: any }>;
    setFilter: (key: string, operator: FilterOperator, value: any, value2?: any) => void;
    removeFilter: (key: string) => void;
    clearFilters: () => void;
    hasFilters: boolean;
  };
  /** Pagination state and controls */
  pagination: {
    paginatedData: T[];
    page: number;
    pageSize: number;
    totalPages: number;
    totalItems: number;
    startIndex: number;
    endIndex: number;
    goToPage: (page: number) => void;
    nextPage: () => void;
    prevPage: () => void;
    firstPage: () => void;
    lastPage: () => void;
    setPageSize: (size: number) => void;
    canNextPage: boolean;
    canPrevPage: boolean;
  };
  /** Total items after filtering (before pagination) */
  totalFilteredItems: number;
  /** Total items in raw data */
  totalRawItems: number;
}

/**
 * Integrated table state management hook
 *
 * Manages sort, filter, and pagination in a coordinated way.
 * Data flows through: raw → filter → sort → paginate
 *
 * @param data - Raw data array
 * @param options - Configuration options
 * @returns Complete table state and controls
 */
export function useTableState<T extends Record<string, any>, K extends keyof T = keyof T>(
  data: T[],
  options: UseTableStateOptions<T, K> = {}
): UseTableStateReturn<T, K> {
  const {
    sort: sortOptions,
    filter: filterOptions,
    pagination: paginationOptions,
    features = { sorting: true, filtering: true, pagination: true },
  } = options;

  // Step 1: Apply filtering
  const filtering = useFilter<T>(data, filterOptions);
  const filteredData = features.filtering ? filtering.filteredData : data;

  // Step 2: Apply sorting to filtered data
  const sorting = useSort<T, K>(
    filteredData,
    features.sorting ? sortOptions : undefined
  );
  const sortedData = features.sorting ? sorting.sortedData : filteredData;

  // Step 3: Apply pagination to sorted+filtered data
  const pagination = usePagination<T>(
    sortedData,
    features.pagination ? paginationOptions : { initialPageSize: sortedData.length }
  );
  const paginatedData = features.pagination ? pagination.paginatedData : sortedData;

  // Memoize final display data
  const displayData = useMemo(() => paginatedData, [paginatedData]);

  // Calculate totals
  const totalFilteredItems = filteredData.length;
  const totalRawItems = data.length;

  return {
    displayData,
    sorting: {
      sortedData: sorting.sortedData,
      sortColumn: sorting.sortColumn,
      sortDirection: sorting.sortDirection,
      handleSort: sorting.handleSort,
      resetSort: sorting.resetSort,
    },
    filtering: {
      filteredData: filtering.filteredData,
      filters: filtering.filters,
      setFilter: filtering.setFilter,
      removeFilter: filtering.removeFilter,
      clearFilters: filtering.clearFilters,
      hasFilters: filtering.hasFilters,
    },
    pagination: {
      paginatedData: pagination.paginatedData,
      page: pagination.page,
      pageSize: pagination.pageSize,
      totalPages: pagination.totalPages,
      totalItems: pagination.totalItems,
      startIndex: pagination.startIndex,
      endIndex: pagination.endIndex,
      goToPage: pagination.goToPage,
      nextPage: pagination.nextPage,
      prevPage: pagination.prevPage,
      firstPage: pagination.firstPage,
      lastPage: pagination.lastPage,
      setPageSize: pagination.setPageSize,
      canNextPage: pagination.canNextPage,
      canPrevPage: pagination.canPrevPage,
    },
    totalFilteredItems,
    totalRawItems,
  };
}
