/**
 * usePagination - Generic pagination hook
 *
 * Provides client-side pagination with boundary clamping and page size control.
 * Features:
 * - Automatic page clamping when data changes
 * - Configurable page sizes
 * - Navigation helpers (next, prev, goto)
 * - Total page/item calculations
 * - Memoized pagination for performance
 *
 * @example
 * ```tsx
 * const {
 *   paginatedData,
 *   page,
 *   pageSize,
 *   totalPages,
 *   goToPage,
 *   nextPage,
 *   prevPage,
 *   setPageSize
 * } = usePagination(data, { initialPage: 1, initialPageSize: 25 });
 * ```
 */

import { useState, useMemo, useCallback, useEffect } from 'react';

export interface UsePaginationOptions {
  /** Initial page number (1-indexed) */
  initialPage?: number;
  /** Initial page size */
  initialPageSize?: number;
  /** Available page size options */
  pageSizeOptions?: number[];
}

export interface UsePaginationReturn<T> {
  /** Current page of data */
  paginatedData: T[];
  /** Current page number (1-indexed) */
  page: number;
  /** Current page size */
  pageSize: number;
  /** Total number of pages */
  totalPages: number;
  /** Total number of items */
  totalItems: number;
  /** Start index of current page (0-indexed) */
  startIndex: number;
  /** End index of current page (0-indexed, inclusive) */
  endIndex: number;
  /** Navigate to specific page */
  goToPage: (page: number) => void;
  /** Navigate to next page */
  nextPage: () => void;
  /** Navigate to previous page */
  prevPage: () => void;
  /** Navigate to first page */
  firstPage: () => void;
  /** Navigate to last page */
  lastPage: () => void;
  /** Change page size (resets to page 1) */
  setPageSize: (size: number) => void;
  /** Check if can go to next page */
  canNextPage: boolean;
  /** Check if can go to previous page */
  canPrevPage: boolean;
}

/**
 * Generic pagination hook for data arrays
 *
 * @param data - Array of data to paginate
 * @param options - Configuration options
 * @returns Paginated data and pagination controls
 */
export function usePagination<T>(
  data: T[],
  options: UsePaginationOptions = {}
): UsePaginationReturn<T> {
  const {
    initialPage = 1,
    initialPageSize = 25,
    pageSizeOptions = [10, 25, 50, 100],
  } = options;

  const [page, setPage] = useState(initialPage);
  const [pageSize, setPageSizeState] = useState(initialPageSize);

  const totalItems = data.length;
  const totalPages = Math.max(1, Math.ceil(totalItems / pageSize));

  /**
   * Clamp page to valid range when data or pageSize changes
   */
  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    } else if (page < 1) {
      setPage(1);
    }
  }, [totalPages, page]);

  /**
   * Calculate page boundaries
   */
  const startIndex = useMemo(() => {
    return (page - 1) * pageSize;
  }, [page, pageSize]);

  const endIndex = useMemo(() => {
    return Math.min(startIndex + pageSize - 1, totalItems - 1);
  }, [startIndex, pageSize, totalItems]);

  /**
   * Memoized paginated data slice
   */
  const paginatedData = useMemo(() => {
    if (totalItems === 0) {
      return [];
    }
    return data.slice(startIndex, startIndex + pageSize);
  }, [data, startIndex, pageSize, totalItems]);

  /**
   * Navigation functions
   */
  const goToPage = useCallback(
    (newPage: number) => {
      const clamped = Math.max(1, Math.min(newPage, totalPages));
      setPage(clamped);
    },
    [totalPages]
  );

  const nextPage = useCallback(() => {
    if (page < totalPages) {
      setPage((p) => p + 1);
    }
  }, [page, totalPages]);

  const prevPage = useCallback(() => {
    if (page > 1) {
      setPage((p) => p - 1);
    }
  }, [page]);

  const firstPage = useCallback(() => {
    setPage(1);
  }, []);

  const lastPage = useCallback(() => {
    setPage(totalPages);
  }, [totalPages]);

  const setPageSize = useCallback((size: number) => {
    // Validate page size
    if (size < 1) {
      console.warn(`[usePagination] Invalid page size: ${size}, using 1`);
      size = 1;
    }

    setPageSizeState(size);
    setPage(1); // Reset to first page when changing page size
  }, []);

  const canNextPage = page < totalPages;
  const canPrevPage = page > 1;

  return {
    paginatedData,
    page,
    pageSize,
    totalPages,
    totalItems,
    startIndex,
    endIndex,
    goToPage,
    nextPage,
    prevPage,
    firstPage,
    lastPage,
    setPageSize,
    canNextPage,
    canPrevPage,
  };
}
