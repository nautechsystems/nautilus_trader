/**
 * Tests for usePagination hook
 */

import { renderHook, act } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { usePagination } from './usePagination';

describe('usePagination', () => {
  const testData = Array.from({ length: 100 }, (_, i) => ({
    id: i + 1,
    value: `Item ${i + 1}`,
  }));

  describe('initial state', () => {
    it('should initialize with default page and pageSize', () => {
      const { result } = renderHook(() => usePagination(testData));

      expect(result.current.page).toBe(1);
      expect(result.current.pageSize).toBe(25);
      expect(result.current.totalPages).toBe(4);
      expect(result.current.totalItems).toBe(100);
    });

    it('should initialize with custom page and pageSize', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 2, initialPageSize: 10 })
      );

      expect(result.current.page).toBe(2);
      expect(result.current.pageSize).toBe(10);
      expect(result.current.totalPages).toBe(10);
    });

    it('should return correct first page data', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPageSize: 10 })
      );

      expect(result.current.paginatedData).toHaveLength(10);
      expect(result.current.paginatedData[0].id).toBe(1);
      expect(result.current.paginatedData[9].id).toBe(10);
    });
  });

  describe('pagination calculations', () => {
    it('should calculate totalPages correctly', () => {
      const { result: r1 } = renderHook(() =>
        usePagination(testData, { initialPageSize: 25 })
      );
      expect(r1.current.totalPages).toBe(4); // 100 / 25 = 4

      const { result: r2 } = renderHook(() =>
        usePagination(testData, { initialPageSize: 30 })
      );
      expect(r2.current.totalPages).toBe(4); // ceil(100 / 30) = 4

      const { result: r3 } = renderHook(() =>
        usePagination(testData, { initialPageSize: 100 })
      );
      expect(r3.current.totalPages).toBe(1); // 100 / 100 = 1
    });

    it('should calculate startIndex and endIndex correctly', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 2, initialPageSize: 10 })
      );

      expect(result.current.startIndex).toBe(10); // Page 2 starts at index 10
      expect(result.current.endIndex).toBe(19); // Page 2 ends at index 19
    });

    it('should handle last page with partial data', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 4, initialPageSize: 30 })
      );

      // Page 4: items 91-100 (10 items)
      expect(result.current.paginatedData).toHaveLength(10);
      expect(result.current.paginatedData[0].id).toBe(91);
      expect(result.current.paginatedData[9].id).toBe(100);
      expect(result.current.endIndex).toBe(99);
    });
  });

  describe('goToPage', () => {
    it('should navigate to specific page', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPageSize: 25 })
      );

      act(() => {
        result.current.goToPage(3);
      });

      expect(result.current.page).toBe(3);
      expect(result.current.paginatedData[0].id).toBe(51); // 25*2 + 1
    });

    it('should clamp page to valid range (upper bound)', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPageSize: 25 })
      );

      act(() => {
        result.current.goToPage(100);
      });

      expect(result.current.page).toBe(4); // Max page
    });

    it('should clamp page to valid range (lower bound)', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 3, initialPageSize: 25 })
      );

      act(() => {
        result.current.goToPage(-5);
      });

      expect(result.current.page).toBe(1); // Min page
    });

    it('should handle page 0', () => {
      const { result } = renderHook(() => usePagination(testData));

      act(() => {
        result.current.goToPage(0);
      });

      expect(result.current.page).toBe(1);
    });
  });

  describe('nextPage and prevPage', () => {
    it('should navigate to next page', () => {
      const { result } = renderHook(() => usePagination(testData));

      expect(result.current.page).toBe(1);

      act(() => {
        result.current.nextPage();
      });

      expect(result.current.page).toBe(2);
    });

    it('should not exceed last page', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 4, initialPageSize: 25 })
      );

      expect(result.current.page).toBe(4);

      act(() => {
        result.current.nextPage();
      });

      expect(result.current.page).toBe(4); // Should stay at last page
    });

    it('should navigate to previous page', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 3 })
      );

      act(() => {
        result.current.prevPage();
      });

      expect(result.current.page).toBe(2);
    });

    it('should not go below first page', () => {
      const { result } = renderHook(() => usePagination(testData));

      expect(result.current.page).toBe(1);

      act(() => {
        result.current.prevPage();
      });

      expect(result.current.page).toBe(1); // Should stay at first page
    });
  });

  describe('firstPage and lastPage', () => {
    it('should navigate to first page', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 3 })
      );

      act(() => {
        result.current.firstPage();
      });

      expect(result.current.page).toBe(1);
    });

    it('should navigate to last page', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPageSize: 25 })
      );

      act(() => {
        result.current.lastPage();
      });

      expect(result.current.page).toBe(4);
    });
  });

  describe('setPageSize', () => {
    it('should change page size and reset to page 1', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPage: 3, initialPageSize: 25 })
      );

      expect(result.current.page).toBe(3);
      expect(result.current.pageSize).toBe(25);

      act(() => {
        result.current.setPageSize(10);
      });

      expect(result.current.page).toBe(1);
      expect(result.current.pageSize).toBe(10);
      expect(result.current.totalPages).toBe(10);
    });

    it('should handle invalid page size', () => {
      const consoleSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
      const { result } = renderHook(() => usePagination(testData));

      act(() => {
        result.current.setPageSize(-5);
      });

      expect(result.current.pageSize).toBe(1);
      expect(consoleSpy).toHaveBeenCalled();
      consoleSpy.mockRestore();
    });
  });

  describe('canNextPage and canPrevPage', () => {
    it('should correctly indicate navigation availability', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPageSize: 25 })
      );

      // Page 1
      expect(result.current.canPrevPage).toBe(false);
      expect(result.current.canNextPage).toBe(true);

      // Page 2
      act(() => {
        result.current.nextPage();
      });
      expect(result.current.canPrevPage).toBe(true);
      expect(result.current.canNextPage).toBe(true);

      // Last page (4)
      act(() => {
        result.current.lastPage();
      });
      expect(result.current.canPrevPage).toBe(true);
      expect(result.current.canNextPage).toBe(false);
    });
  });

  describe('edge cases', () => {
    it('should handle empty data array', () => {
      const { result } = renderHook(() => usePagination([]));

      expect(result.current.paginatedData).toEqual([]);
      expect(result.current.totalPages).toBe(1); // At least 1 page
      expect(result.current.totalItems).toBe(0);
      expect(result.current.page).toBe(1);
    });

    it('should handle single item', () => {
      const singleItem = [{ id: 1, value: 'Only' }];
      const { result } = renderHook(() => usePagination(singleItem));

      expect(result.current.paginatedData).toEqual(singleItem);
      expect(result.current.totalPages).toBe(1);
      expect(result.current.canNextPage).toBe(false);
      expect(result.current.canPrevPage).toBe(false);
    });

    it('should handle data.length < pageSize', () => {
      const smallData = testData.slice(0, 5);
      const { result } = renderHook(() =>
        usePagination(smallData, { initialPageSize: 25 })
      );

      expect(result.current.paginatedData).toHaveLength(5);
      expect(result.current.totalPages).toBe(1);
    });

    it('should auto-clamp page when data shrinks', () => {
      const { result, rerender } = renderHook(
        ({ data }) => usePagination(data, { initialPageSize: 10 }),
        { initialProps: { data: testData } }
      );

      // Navigate to page 5
      act(() => {
        result.current.goToPage(5);
      });
      expect(result.current.page).toBe(5);

      // Shrink data to 20 items (2 pages)
      const smallerData = testData.slice(0, 20);
      rerender({ data: smallerData });

      // Should auto-clamp to page 2
      expect(result.current.page).toBe(2);
    });

    it('should handle pageSize larger than data', () => {
      const { result } = renderHook(() =>
        usePagination(testData, { initialPageSize: 200 })
      );

      expect(result.current.paginatedData).toHaveLength(100);
      expect(result.current.totalPages).toBe(1);
      expect(result.current.page).toBe(1);
    });
  });

  describe('memoization', () => {
    it('should maintain stable function references', () => {
      const { result, rerender } = renderHook(() => usePagination(testData));

      const firstGoToPage = result.current.goToPage;
      const firstNextPage = result.current.nextPage;
      const firstPrevPage = result.current.prevPage;
      const firstSetPageSize = result.current.setPageSize;

      rerender();

      expect(result.current.goToPage).toBe(firstGoToPage);
      expect(result.current.nextPage).toBe(firstNextPage);
      expect(result.current.prevPage).toBe(firstPrevPage);
      expect(result.current.setPageSize).toBe(firstSetPageSize);
    });

    it('should only recompute paginatedData when necessary', () => {
      const { result, rerender } = renderHook(
        ({ data }) => usePagination(data, { initialPageSize: 25 }),
        { initialProps: { data: testData } }
      );

      const firstPaginated = result.current.paginatedData;

      // Rerender with same data reference
      rerender({ data: testData });

      // Should maintain reference equality
      expect(result.current.paginatedData).toBe(firstPaginated);
    });
  });

  describe('stress test', () => {
    it('should handle large datasets efficiently', () => {
      const largeData = Array.from({ length: 10000 }, (_, i) => ({ id: i }));
      const { result } = renderHook(() =>
        usePagination(largeData, { initialPageSize: 100 })
      );

      expect(result.current.totalPages).toBe(100);
      expect(result.current.paginatedData).toHaveLength(100);

      // Navigate to middle
      act(() => {
        result.current.goToPage(50);
      });

      expect(result.current.page).toBe(50);
      expect(result.current.paginatedData[0].id).toBe(4900);
    });
  });
});
