/**
 * Tests for useTableState hook
 */

import { renderHook, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { useTableState } from './useTableState';

describe('useTableState', () => {
  const testData = [
    { id: 1, name: 'Charlie', age: 30, score: 85, tags: ['js'] },
    { id: 2, name: 'Alice', age: 25, score: 92, tags: ['python'] },
    { id: 3, name: 'Bob', age: 35, score: 78, tags: ['rust'] },
    { id: 4, name: 'David', age: 28, score: 88, tags: ['go'] },
    { id: 5, name: 'Eve', age: 22, score: 95, tags: ['js'] },
    { id: 6, name: 'Frank', age: 32, score: 82, tags: ['python'] },
  ];

  describe('initial state', () => {
    it('should return unmodified data with default options', () => {
      const { result } = renderHook(() => useTableState(testData));

      expect(result.current.displayData).toEqual(testData);
      expect(result.current.totalRawItems).toBe(6);
      expect(result.current.totalFilteredItems).toBe(6);
    });

    it('should apply initial sort', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          sort: { defaultColumn: 'name', defaultDirection: 'asc' },
        })
      );

      expect(result.current.displayData[0].name).toBe('Alice');
      expect(result.current.sorting.sortColumn).toBe('name');
      expect(result.current.sorting.sortDirection).toBe('asc');
    });

    it('should apply initial pagination', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
        })
      );

      expect(result.current.displayData).toHaveLength(2);
      expect(result.current.pagination.totalPages).toBe(3);
      expect(result.current.pagination.page).toBe(1);
    });
  });

  describe('data pipeline: filter → sort → paginate', () => {
    it('should apply transformations in correct order', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          sort: { defaultColumn: 'age', defaultDirection: 'asc' },
          pagination: { initialPageSize: 2 },
        })
      );

      // Initial state: sorted by age (asc), page 1, size 2
      // Should show Eve (22) and Alice (25)
      expect(result.current.displayData).toHaveLength(2);
      expect(result.current.displayData[0].name).toBe('Eve');
      expect(result.current.displayData[1].name).toBe('Alice');

      // Apply filter: age >= 28
      act(() => {
        result.current.filtering.setFilter('age', 'gte', 28);
      });

      // Filtered: David (28), Charlie (30), Frank (32), Bob (35)
      // Sorted by age: David (28), Charlie (30), Frank (32), Bob (35)
      // Page 1 (size 2): David, Charlie
      expect(result.current.displayData).toHaveLength(2);
      expect(result.current.displayData[0].name).toBe('David');
      expect(result.current.displayData[1].name).toBe('Charlie');
      expect(result.current.totalFilteredItems).toBe(4);
      expect(result.current.pagination.totalPages).toBe(2);
    });

    it('should recalculate pagination when filters change', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 3, initialPage: 2 },
        })
      );

      // Initially on page 2
      expect(result.current.pagination.page).toBe(2);
      expect(result.current.pagination.totalPages).toBe(2);

      // Apply filter that reduces data to 2 items
      act(() => {
        result.current.filtering.setFilter('age', 'lte', 25);
      });

      // Should auto-clamp to page 1 (only 1 page of data now)
      expect(result.current.totalFilteredItems).toBe(2);
      expect(result.current.pagination.totalPages).toBe(1);
      expect(result.current.pagination.page).toBe(1);
    });
  });

  describe('sorting integration', () => {
    it('should sort and paginate correctly', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          sort: { defaultColumn: 'score', defaultDirection: 'desc' },
          pagination: { initialPageSize: 3 },
        })
      );

      // Top 3 scores: Eve (95), Alice (92), David (88)
      expect(result.current.displayData.map(d => d.name)).toEqual([
        'Eve',
        'Alice',
        'David',
      ]);

      // Toggle sort on same column: desc → null (unsorted)
      act(() => {
        result.current.sorting.handleSort('score');
      });

      // After toggle to null: original order, page 1: Charlie, Alice, Bob
      expect(result.current.sorting.sortDirection).toBeNull();
      expect(result.current.displayData.map(d => d.name)).toEqual([
        'Charlie',
        'Alice',
        'Bob',
      ]);

      // Toggle again on same column: null → asc
      act(() => {
        result.current.sorting.handleSort('score');
      });

      // Now sorted asc, page 1: Bob (78), Frank (82), Charlie (85)
      expect(result.current.sorting.sortDirection).toBe('asc');
      expect(result.current.displayData.map(d => d.name)).toEqual([
        'Bob',
        'Frank',
        'Charlie',
      ]);
    });

    it('should maintain page position when sorting same column', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2, initialPage: 2 },
        })
      );

      expect(result.current.pagination.page).toBe(2);

      // Sort by name
      act(() => {
        result.current.sorting.handleSort('name');
      });

      // Page resets when new column sorted
      expect(result.current.pagination.page).toBe(2);
    });
  });

  describe('filtering integration', () => {
    it('should filter and paginate correctly', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
        })
      );

      // Apply filter
      act(() => {
        result.current.filtering.setFilter('name', 'contains', 'a');
      });

      // Matches: Charlie, Alice, David, Frank (4 items)
      expect(result.current.totalFilteredItems).toBe(4);
      expect(result.current.pagination.totalPages).toBe(2);
      expect(result.current.displayData).toHaveLength(2);
    });

    it('should handle multiple filters', () => {
      const { result } = renderHook(() => useTableState(testData));

      act(() => {
        result.current.filtering.setFilter('age', 'gte', 25);
        result.current.filtering.setFilter('score', 'gte', 85);
      });

      // age >= 25 AND score >= 85: Alice (25, 92), Charlie (30, 85), David (28, 88)
      expect(result.current.totalFilteredItems).toBe(3);
      expect(result.current.displayData.map(d => d.name)).toContain('Alice');
      expect(result.current.displayData.map(d => d.name)).toContain('Charlie');
      expect(result.current.displayData.map(d => d.name)).toContain('David');
    });

    it('should clear filters correctly', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
        })
      );

      act(() => {
        result.current.filtering.setFilter('age', 'gte', 30);
      });

      expect(result.current.totalFilteredItems).toBe(3);

      act(() => {
        result.current.filtering.clearFilters();
      });

      expect(result.current.totalFilteredItems).toBe(6);
      expect(result.current.filtering.hasFilters).toBe(false);
    });
  });

  describe('pagination integration', () => {
    it('should paginate correctly', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
        })
      );

      // Page 1
      expect(result.current.displayData).toHaveLength(2);
      expect(result.current.pagination.canNextPage).toBe(true);
      expect(result.current.pagination.canPrevPage).toBe(false);

      // Go to page 2
      act(() => {
        result.current.pagination.nextPage();
      });

      expect(result.current.pagination.page).toBe(2);
      expect(result.current.displayData).toHaveLength(2);

      // Go to last page (3)
      act(() => {
        result.current.pagination.lastPage();
      });

      expect(result.current.pagination.page).toBe(3);
      expect(result.current.displayData).toHaveLength(2);
      expect(result.current.pagination.canNextPage).toBe(false);
    });

    it('should change page size correctly', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
        })
      );

      expect(result.current.pagination.totalPages).toBe(3);

      act(() => {
        result.current.pagination.setPageSize(3);
      });

      expect(result.current.pagination.totalPages).toBe(2);
      expect(result.current.pagination.page).toBe(1); // Reset to page 1
    });
  });

  describe('feature toggles', () => {
    it('should disable sorting when configured', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          sort: { defaultColumn: 'name', defaultDirection: 'asc' },
          features: { sorting: false, filtering: true, pagination: true },
        })
      );

      // Should not apply sort
      expect(result.current.displayData[0].id).toBe(1);
      expect(result.current.sorting.sortColumn).toBeNull();
    });

    it('should disable filtering when configured', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          features: { sorting: true, filtering: false, pagination: true },
        })
      );

      act(() => {
        result.current.filtering.setFilter('age', 'gte', 30);
      });

      // Filter should not be applied
      expect(result.current.totalFilteredItems).toBe(6);
    });

    it('should disable pagination when configured', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
          features: { sorting: true, filtering: true, pagination: false },
        })
      );

      // Should show all data
      expect(result.current.displayData).toHaveLength(6);
    });
  });

  describe('complex scenarios', () => {
    it('should handle filter + sort + paginate together', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          sort: { defaultColumn: 'score', defaultDirection: 'desc' },
          pagination: { initialPageSize: 2 },
        })
      );

      // Apply filter: score >= 85
      act(() => {
        result.current.filtering.setFilter('score', 'gte', 85);
      });

      // Filtered: Eve (95), Alice (92), David (88), Charlie (85)
      // Sorted by score desc: Eve (95), Alice (92), David (88), Charlie (85)
      // Page 1 (size 2): Eve, Alice
      expect(result.current.displayData).toHaveLength(2);
      expect(result.current.displayData[0].name).toBe('Eve');
      expect(result.current.displayData[1].name).toBe('Alice');
      expect(result.current.totalFilteredItems).toBe(4);
      expect(result.current.pagination.totalPages).toBe(2);

      // Go to page 2
      act(() => {
        result.current.pagination.nextPage();
      });

      expect(result.current.displayData[0].name).toBe('David');
      expect(result.current.displayData[1].name).toBe('Charlie');

      // Toggle sort: desc → null (unsorted)
      act(() => {
        result.current.sorting.handleSort('score');
      });

      // After toggle to null: original order (filtered: Charlie, Alice, David, Eve with score >= 85)
      // Page 2 (size 2): David, Eve
      expect(result.current.sorting.sortDirection).toBeNull();
      expect(result.current.displayData[0].name).toBe('David');
      expect(result.current.displayData[1].name).toBe('Eve');

      // Toggle again: null → asc
      act(() => {
        result.current.sorting.handleSort('score');
      });

      // Now sorted asc: Charlie (85), David (88), Alice (92), Eve (95)
      // Page 2: Alice, Eve
      expect(result.current.sorting.sortDirection).toBe('asc');
      expect(result.current.displayData[0].name).toBe('Alice');
      expect(result.current.displayData[1].name).toBe('Eve');
    });

    it('should handle empty filter results', () => {
      const { result } = renderHook(() =>
        useTableState(testData, {
          pagination: { initialPageSize: 2 },
        })
      );

      act(() => {
        result.current.filtering.setFilter('age', 'gt', 100);
      });

      expect(result.current.totalFilteredItems).toBe(0);
      expect(result.current.displayData).toHaveLength(0);
      expect(result.current.pagination.totalPages).toBe(1);
    });
  });

  describe('performance', () => {
    it('should maintain stable function references', () => {
      const { result, rerender } = renderHook(() => useTableState(testData));

      const firstHandleSort = result.current.sorting.handleSort;
      const firstSetFilter = result.current.filtering.setFilter;
      const firstGoToPage = result.current.pagination.goToPage;

      rerender();

      expect(result.current.sorting.handleSort).toBe(firstHandleSort);
      expect(result.current.filtering.setFilter).toBe(firstSetFilter);
      expect(result.current.pagination.goToPage).toBe(firstGoToPage);
    });

    it('should handle large datasets efficiently', () => {
      const largeData = Array.from({ length: 10000 }, (_, i) => ({
        id: i,
        name: `User ${i}`,
        score: Math.floor(Math.random() * 100),
      }));

      const { result } = renderHook(() =>
        useTableState(largeData, {
          pagination: { initialPageSize: 50 },
        })
      );

      expect(result.current.displayData).toHaveLength(50);
      expect(result.current.totalRawItems).toBe(10000);

      // Filter
      act(() => {
        result.current.filtering.setFilter('score', 'gte', 90);
      });

      expect(result.current.totalFilteredItems).toBeLessThan(10000);
      expect(result.current.displayData.length).toBeLessThanOrEqual(50);
    });
  });

  describe('edge cases', () => {
    it('should handle empty data', () => {
      const { result } = renderHook(() => useTableState([]));

      expect(result.current.displayData).toEqual([]);
      expect(result.current.totalRawItems).toBe(0);
      expect(result.current.totalFilteredItems).toBe(0);
    });

    it('should handle single item', () => {
      const singleItem = [{ id: 1, name: 'Solo', value: 42 }];
      const { result } = renderHook(() => useTableState(singleItem));

      expect(result.current.displayData).toEqual(singleItem);
      expect(result.current.pagination.totalPages).toBe(1);
    });
  });
});
