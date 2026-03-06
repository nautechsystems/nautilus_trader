/**
 * Tests for useSort hook
 */

import { renderHook, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { useSort } from './useSort';

describe('useSort', () => {
  const testData = [
    { id: 1, name: 'Charlie', age: 30, score: 85 },
    { id: 2, name: 'Alice', age: 25, score: 92 },
    { id: 3, name: 'Bob', age: 35, score: 78 },
    { id: 4, name: 'alice', age: 28, score: null as number | null },
  ];

  describe('initial state', () => {
    it('should return unsorted data when no default column provided', () => {
      const { result } = renderHook(() => useSort(testData));

      expect(result.current.sortedData).toEqual(testData);
      expect(result.current.sortColumn).toBeNull();
      expect(result.current.sortDirection).toBeNull();
    });

    it('should sort by default column in default direction', () => {
      const { result } = renderHook(() =>
        useSort(testData, { defaultColumn: 'name', defaultDirection: 'asc' })
      );

      expect(result.current.sortColumn).toBe('name');
      expect(result.current.sortDirection).toBe('asc');
      expect(result.current.sortedData[0].name).toBe('Alice');
      expect(result.current.sortedData[1].name).toBe('alice');
    });

    it('should use desc as default direction when default column provided', () => {
      const { result } = renderHook(() =>
        useSort(testData, { defaultColumn: 'age', defaultDirection: 'desc' })
      );

      expect(result.current.sortColumn).toBe('age');
      expect(result.current.sortDirection).toBe('desc');
      expect(result.current.sortedData[0].age).toBe(35);
    });
  });

  describe('handleSort', () => {
    it('should sort ascending on first click', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('name');
      });

      expect(result.current.sortColumn).toBe('name');
      expect(result.current.sortDirection).toBe('asc');
      expect(result.current.sortedData[0].name).toBe('Alice');
    });

    it('should toggle asc → desc → null on same column', () => {
      const { result } = renderHook(() => useSort(testData));

      // First click: asc
      act(() => {
        result.current.handleSort('name');
      });
      expect(result.current.sortDirection).toBe('asc');
      expect(result.current.sortedData[0].name).toBe('Alice');

      // Second click: desc
      act(() => {
        result.current.handleSort('name');
      });
      expect(result.current.sortDirection).toBe('desc');
      expect(result.current.sortedData[0].name).toBe('Charlie');

      // Third click: null (original order)
      act(() => {
        result.current.handleSort('name');
      });
      expect(result.current.sortColumn).toBeNull();
      expect(result.current.sortDirection).toBeNull();
      expect(result.current.sortedData).toEqual(testData);
    });

    it('should reset to asc when switching columns', () => {
      const { result } = renderHook(() => useSort(testData));

      // Sort by name desc
      act(() => {
        result.current.handleSort('name');
      });
      act(() => {
        result.current.handleSort('name');
      });
      expect(result.current.sortDirection).toBe('desc');

      // Switch to age - should reset to asc
      act(() => {
        result.current.handleSort('age');
      });
      expect(result.current.sortColumn).toBe('age');
      expect(result.current.sortDirection).toBe('asc');
      expect(result.current.sortedData[0].age).toBe(25);
    });
  });

  describe('numeric sorting', () => {
    it('should sort numbers correctly in ascending order', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('age');
      });

      expect(result.current.sortedData.map(d => d.age)).toEqual([25, 28, 30, 35]);
    });

    it('should sort numbers correctly in descending order', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('age');
      });
      act(() => {
        result.current.handleSort('age');
      });

      expect(result.current.sortedData.map(d => d.age)).toEqual([35, 30, 28, 25]);
    });

    it('should handle null values in numeric sorting', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('score');
      });

      // Null should be last in ascending order
      const scores = result.current.sortedData.map(d => d.score);
      expect(scores[scores.length - 1]).toBeNull();
      expect(scores.slice(0, -1)).toEqual([78, 85, 92]);
    });
  });

  describe('string sorting', () => {
    it('should sort strings case-insensitively', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('name');
      });

      // Both "Alice" and "alice" should be together
      expect(result.current.sortedData[0].name).toBe('Alice');
      expect(result.current.sortedData[1].name).toBe('alice');
    });

    it('should sort strings in descending order', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('name');
      });
      act(() => {
        result.current.handleSort('name');
      });

      expect(result.current.sortedData[0].name).toBe('Charlie');
    });
  });

  describe('custom comparator', () => {
    it('should use custom compareFn when provided', () => {
      const customCompare = (a: typeof testData[0], b: typeof testData[0]) => {
        // Custom logic: sort by name length
        return a.name.length - b.name.length;
      };

      const { result } = renderHook(() =>
        useSort(testData, { compareFn: customCompare })
      );

      act(() => {
        result.current.handleSort('name');
      });

      // Sorted by name length: Bob(3), Alice(5), alice(5), Charlie(7)
      expect(result.current.sortedData[0].name).toBe('Bob');
      expect(result.current.sortedData[3].name).toBe('Charlie');
    });

    it('should respect sortDirection with custom compareFn', () => {
      const customCompare = (a: typeof testData[0], b: typeof testData[0]) => {
        return a.name.length - b.name.length;
      };

      const { result } = renderHook(() =>
        useSort(testData, { compareFn: customCompare })
      );

      // Desc
      act(() => {
        result.current.handleSort('name');
      });
      act(() => {
        result.current.handleSort('name');
      });

      expect(result.current.sortedData[0].name).toBe('Charlie');
    });
  });

  describe('resetSort', () => {
    it('should reset to original order', () => {
      const { result } = renderHook(() => useSort(testData));

      act(() => {
        result.current.handleSort('name');
      });
      expect(result.current.sortColumn).toBe('name');

      act(() => {
        result.current.resetSort();
      });

      expect(result.current.sortColumn).toBeNull();
      expect(result.current.sortDirection).toBeNull();
      expect(result.current.sortedData).toEqual(testData);
    });
  });

  describe('edge cases', () => {
    it('should handle empty array', () => {
      const { result } = renderHook(() => useSort([]));

      expect(result.current.sortedData).toEqual([]);
    });

    it('should handle single item array', () => {
      const singleItem = [{ id: 1, name: 'Solo' }];
      const { result } = renderHook(() => useSort(singleItem));

      act(() => {
        result.current.handleSort('name');
      });

      expect(result.current.sortedData).toEqual(singleItem);
    });

    it('should handle undefined values', () => {
      const dataWithUndefined = [
        { id: 1, name: 'Alice', value: undefined as number | undefined },
        { id: 2, name: 'Bob', value: 10 },
        { id: 3, name: 'Charlie', value: undefined as number | undefined },
      ];

      const { result } = renderHook(() => useSort(dataWithUndefined));

      act(() => {
        result.current.handleSort('value');
      });

      // Undefined should be last
      expect(result.current.sortedData[0].value).toBe(10);
      expect(result.current.sortedData[1].value).toBeUndefined();
      expect(result.current.sortedData[2].value).toBeUndefined();
    });
  });

  describe('memoization', () => {
    it('should not re-sort when data reference changes but content is same', () => {
      const { result, rerender } = renderHook(
        ({ data }) => useSort(data, { defaultColumn: 'name' }),
        { initialProps: { data: testData } }
      );

      const firstSort = result.current.sortedData;

      // Rerender with same content but different reference
      rerender({ data: [...testData] });

      // Should maintain sort
      expect(result.current.sortedData[0].name).toBe(firstSort[0].name);
    });

    it('should maintain stable function references', () => {
      const { result, rerender } = renderHook(() => useSort(testData));

      const firstHandleSort = result.current.handleSort;
      const firstResetSort = result.current.resetSort;

      rerender();

      expect(result.current.handleSort).toBe(firstHandleSort);
      expect(result.current.resetSort).toBe(firstResetSort);
    });
  });
});
