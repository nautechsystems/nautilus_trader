/**
 * Tests for useFilter hook
 */

import { renderHook, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { useFilter } from './useFilter';

describe('useFilter', () => {
  const testData = [
    { id: 1, name: 'Alice', age: 25, score: 92, tags: ['js', 'react'] },
    { id: 2, name: 'Bob', age: 30, score: 85, tags: ['python', 'django'] },
    { id: 3, name: 'Charlie', age: 35, score: 78, tags: ['js', 'vue'] },
    { id: 4, name: 'David', age: 28, score: 88, tags: ['rust', 'go'] },
    { id: 5, name: 'alice', age: 22, score: 95, tags: ['js', 'angular'] },
  ];

  describe('initial state', () => {
    it('should return unfiltered data when no filters applied', () => {
      const { result } = renderHook(() => useFilter(testData));

      expect(result.current.filteredData).toEqual(testData);
      expect(result.current.filters).toEqual([]);
      expect(result.current.hasFilters).toBe(false);
    });
  });

  describe('equals operator', () => {
    it('should filter by exact match', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'equals', 'Bob');
      });

      expect(result.current.filteredData).toHaveLength(1);
      expect(result.current.filteredData[0].name).toBe('Bob');
    });

    it('should be case-insensitive by default', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'equals', 'alice');
      });

      // Should match both "Alice" and "alice"
      expect(result.current.filteredData).toHaveLength(2);
      expect(result.current.filteredData.map(d => d.name)).toContain('Alice');
      expect(result.current.filteredData.map(d => d.name)).toContain('alice');
    });

    it('should respect case-sensitive option', () => {
      const { result } = renderHook(() => useFilter(testData, { caseSensitive: true }));

      act(() => {
        result.current.setFilter('name', 'equals', 'alice');
      });

      expect(result.current.filteredData).toHaveLength(1);
      expect(result.current.filteredData[0].name).toBe('alice');
    });
  });

  describe('notEquals operator', () => {
    it('should filter by inequality', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'notEquals', 'Bob');
      });

      expect(result.current.filteredData).toHaveLength(4);
      expect(result.current.filteredData.every(d => d.name !== 'Bob')).toBe(true);
    });
  });

  describe('contains operator', () => {
    it('should filter by substring match', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'contains', 'li');
      });

      // Should match "Alice", "alice", "Charlie"
      expect(result.current.filteredData).toHaveLength(3);
    });

    it('should be case-insensitive by default', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'contains', 'ALICE');
      });

      expect(result.current.filteredData).toHaveLength(2);
    });
  });

  describe('startsWith operator', () => {
    it('should filter by prefix match', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'startsWith', 'a');
      });

      // Should match "Alice" and "alice"
      expect(result.current.filteredData).toHaveLength(2);
    });
  });

  describe('endsWith operator', () => {
    it('should filter by suffix match', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'endsWith', 'e');
      });

      // Should match "Alice", "alice", "Charlie"
      expect(result.current.filteredData).toHaveLength(3);
    });
  });

  describe('numeric operators', () => {
    it('should filter by greater than', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'gt', 28);
      });

      expect(result.current.filteredData).toHaveLength(2);
      expect(result.current.filteredData.every(d => d.age > 28)).toBe(true);
    });

    it('should filter by greater than or equal', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'gte', 28);
      });

      expect(result.current.filteredData).toHaveLength(3);
      expect(result.current.filteredData.every(d => d.age >= 28)).toBe(true);
    });

    it('should filter by less than', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('score', 'lt', 85);
      });

      expect(result.current.filteredData).toHaveLength(1);
      expect(result.current.filteredData[0].score).toBe(78);
    });

    it('should filter by less than or equal', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('score', 'lte', 85);
      });

      expect(result.current.filteredData).toHaveLength(2);
    });

    it('should filter by between range', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'between', 25, 30);
      });

      expect(result.current.filteredData).toHaveLength(3);
      expect(result.current.filteredData.every(d => d.age >= 25 && d.age <= 30)).toBe(true);
    });
  });

  describe('in/notIn operators', () => {
    it('should filter by inclusion in array', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'in', ['Alice', 'Bob', 'Unknown']);
      });

      // Case-insensitive: should match "Alice", "alice", "Bob"
      expect(result.current.filteredData).toHaveLength(3);
    });

    it('should filter by exclusion from array', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('name', 'notIn', ['Alice', 'Bob']);
      });

      // Case-insensitive: should exclude "Alice" (matches both "Alice" and "alice") and "Bob"
      // Excluded: id:2 (Alice), id:5 (alice), id:3 (Bob) = 3 items
      // Remaining: id:1 (Charlie), id:4 (David) = 2 items (Eve is missing from data)
      expect(result.current.filteredData).toHaveLength(2);
      expect(result.current.filteredData.map(d => d.name).sort()).toEqual(['Charlie', 'David']);
    });
  });

  describe('multiple filters (AND logic)', () => {
    it('should apply multiple filters with AND logic by default', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'gte', 25);
        result.current.setFilter('score', 'gte', 85);
      });

      // Should match: Bob (30, 85), David (28, 88), Alice (25, 92)
      expect(result.current.filteredData).toHaveLength(3);
      expect(result.current.filteredData.every(d => d.age >= 25 && d.score >= 85)).toBe(true);
    });

    it('should update existing filter when setting same key', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'gte', 25);
      });
      expect(result.current.filteredData).toHaveLength(4);

      act(() => {
        result.current.setFilter('age', 'gte', 30);
      });
      expect(result.current.filteredData).toHaveLength(2);
      expect(result.current.filters).toHaveLength(1);
    });
  });

  describe('multiple filters (OR logic)', () => {
    it('should apply multiple filters with OR logic when configured', () => {
      const { result } = renderHook(() => useFilter(testData, { useOrLogic: true }));

      act(() => {
        result.current.setFilter('name', 'equals', 'Alice');
        result.current.setFilter('age', 'gte', 35);
      });

      // Should match: Alice (age 25 OR name='Alice'), Charlie (age 35), alice (name='Alice')
      expect(result.current.filteredData).toHaveLength(3);
    });
  });

  describe('removeFilter', () => {
    it('should remove a specific filter', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'gte', 25);
        result.current.setFilter('score', 'gte', 85);
      });
      expect(result.current.filters).toHaveLength(2);

      act(() => {
        result.current.removeFilter('age');
      });

      expect(result.current.filters).toHaveLength(1);
      expect(result.current.filters[0].key).toBe('score');
    });

    it('should not error when removing non-existent filter', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.removeFilter('nonexistent');
      });

      expect(result.current.filters).toHaveLength(0);
    });
  });

  describe('clearFilters', () => {
    it('should remove all filters', () => {
      const { result } = renderHook(() => useFilter(testData));

      act(() => {
        result.current.setFilter('age', 'gte', 25);
        result.current.setFilter('score', 'gte', 85);
        result.current.setFilter('name', 'contains', 'a');
      });
      expect(result.current.filters).toHaveLength(3);

      act(() => {
        result.current.clearFilters();
      });

      expect(result.current.filters).toHaveLength(0);
      expect(result.current.filteredData).toEqual(testData);
      expect(result.current.hasFilters).toBe(false);
    });
  });

  describe('edge cases', () => {
    it('should handle empty data array', () => {
      const { result } = renderHook(() => useFilter([]));

      act(() => {
        result.current.setFilter('name', 'equals', 'Alice');
      });

      expect(result.current.filteredData).toEqual([]);
    });

    it('should handle null/undefined values', () => {
      const dataWithNulls = [
        { id: 1, name: 'Alice', value: null as number | null },
        { id: 2, name: 'Bob', value: 10 },
        { id: 3, name: 'Charlie', value: undefined as number | undefined },
      ];

      const { result } = renderHook(() => useFilter(dataWithNulls));

      act(() => {
        result.current.setFilter('value', 'gt', 5);
      });

      expect(result.current.filteredData).toHaveLength(1);
      expect(result.current.filteredData[0].name).toBe('Bob');
    });

    it('should handle notEquals with null values', () => {
      const dataWithNulls = [
        { id: 1, name: 'Alice', value: null as number | null },
        { id: 2, name: 'Bob', value: 10 },
      ];

      const { result } = renderHook(() => useFilter(dataWithNulls));

      act(() => {
        result.current.setFilter('value', 'notEquals', 10);
      });

      // Should include Alice (null != 10)
      expect(result.current.filteredData).toHaveLength(1);
      expect(result.current.filteredData[0].name).toBe('Alice');
    });
  });

  describe('memoization', () => {
    it('should maintain stable function references', () => {
      const { result, rerender } = renderHook(() => useFilter(testData));

      const firstSetFilter = result.current.setFilter;
      const firstRemoveFilter = result.current.removeFilter;
      const firstClearFilters = result.current.clearFilters;

      rerender();

      expect(result.current.setFilter).toBe(firstSetFilter);
      expect(result.current.removeFilter).toBe(firstRemoveFilter);
      expect(result.current.clearFilters).toBe(firstClearFilters);
    });

    it('should only recompute when filters or data change', () => {
      const { result, rerender } = renderHook(
        ({ data }) => useFilter(data),
        { initialProps: { data: testData } }
      );

      act(() => {
        result.current.setFilter('name', 'equals', 'Alice');
      });

      const firstFiltered = result.current.filteredData;

      // Rerender without changes
      rerender({ data: testData });

      // Should maintain reference equality (memoization working)
      expect(result.current.filteredData).toBe(firstFiltered);
    });
  });
});
