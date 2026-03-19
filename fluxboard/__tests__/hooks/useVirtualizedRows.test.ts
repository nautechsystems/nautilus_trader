import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { useVirtualizedRows } from '@/hooks/useVirtualizedRows';

type Row = { id: string };

function makeRows(ids: string[]): Row[] {
  return ids.map((id) => ({ id }));
}

function createScrollElement({
  clientHeight,
  scrollTop = 0,
}: {
  clientHeight: number;
  scrollTop?: number;
}) {
  const element = document.createElement('div');
  let currentScrollTop = scrollTop;
  Object.defineProperty(element, 'clientHeight', {
    configurable: true,
    get: () => clientHeight,
  });
  Object.defineProperty(element, 'scrollTop', {
    configurable: true,
    get: () => currentScrollTop,
    set: (value: number) => {
      currentScrollTop = value;
    },
  });
  document.body.appendChild(element);
  return element;
}

describe('useVirtualizedRows', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('computes a stable visible range and virtual items from scroll position', () => {
    const scrollElement = createScrollElement({ clientHeight: 60 });
    const rows = makeRows(['0', '1', '2', '3', '4', '5']);

    const { result } = renderHook(() =>
      useVirtualizedRows({
        rows,
        getRowId: (row) => row.id,
        estimateSize: () => 20,
        overscan: 1,
        scrollRef: { current: scrollElement },
      }),
    );

    expect(result.current.visibleRange).toEqual({ start: 0, end: 3 });
    expect(result.current.virtualItems.map((item) => item.key)).toEqual(['0', '1', '2', '3']);

    act(() => {
      scrollElement.scrollTop = 40;
      scrollElement.dispatchEvent(new Event('scroll'));
    });

    expect(result.current.visibleRange).toEqual({ start: 2, end: 5 });
    expect(result.current.virtualItems.map((item) => item.key)).toEqual(['1', '2', '3', '4', '5']);
  });

  it('preserves the scroll anchor when rows are prepended', () => {
    const scrollElement = createScrollElement({ clientHeight: 60, scrollTop: 40 });
    const initialRows = makeRows(['a', 'b', 'c', 'd', 'e']);

    const { rerender } = renderHook(
      ({ rows }) =>
        useVirtualizedRows({
          rows,
          getRowId: (row) => row.id,
          estimateSize: () => 20,
          overscan: 0,
          scrollRef: { current: scrollElement },
        }),
      {
        initialProps: { rows: initialRows },
      },
    );

    act(() => {
      scrollElement.dispatchEvent(new Event('scroll'));
    });

    rerender({
      rows: makeRows(['x', 'y', 'a', 'b', 'c', 'd', 'e']),
    });

    expect(scrollElement.scrollTop).toBe(80);
  });

  it('keeps measurement cache entries by row key and prunes removed rows', () => {
    const scrollElement = createScrollElement({ clientHeight: 60 });
    const initialRows = makeRows(['a', 'b', 'c']);

    const { result, rerender } = renderHook(
      ({ rows }) =>
        useVirtualizedRows({
          rows,
          getRowId: (row) => row.id,
          estimateSize: () => 20,
          overscan: 0,
          scrollRef: { current: scrollElement },
        }),
      {
        initialProps: { rows: initialRows },
      },
    );

    act(() => {
      result.current.registerMeasurement('b', 80);
    });

    expect(result.current.measurementCache.get('b')).toBe(80);
    expect(result.current.totalSize).toBe(120);

    rerender({
      rows: makeRows(['c', 'b', 'a']),
    });

    expect(result.current.measurementCache.get('b')).toBe(80);
    expect(result.current.totalSize).toBe(120);

    rerender({
      rows: makeRows(['c', 'a']),
    });

    expect(result.current.measurementCache.has('b')).toBe(false);
    expect(result.current.totalSize).toBe(40);
  });
});
