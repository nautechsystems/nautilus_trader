import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from 'react';
import { clampIndexRange, type IndexRange } from '@/lib/realtime/selectors';

export interface VirtualizedRowItem<TRow> {
  index: number;
  key: string;
  row: TRow;
  start: number;
  size: number;
  end: number;
}

export interface UseVirtualizedRowsOptions<TRow> {
  rows: readonly TRow[];
  getRowId: (row: TRow, index: number) => string;
  estimateSize: number | ((row: TRow, index: number) => number);
  overscan?: number;
  enabled?: boolean;
  scrollRef: RefObject<HTMLElement | null>;
}

export interface UseVirtualizedRowsResult<TRow> {
  visibleRange: IndexRange;
  virtualItems: readonly VirtualizedRowItem<TRow>[];
  totalSize: number;
  paddingTop: number;
  paddingBottom: number;
  measurementCache: ReadonlyMap<string, number>;
  registerMeasurement: (rowId: string, size: number) => void;
  measureElement: (rowId: string) => (node: HTMLElement | null) => void;
  scrollToIndex: (index: number, align?: 'start' | 'center' | 'end') => void;
}

type LayoutSnapshot = {
  rowIds: string[];
  starts: number[];
  sizes: number[];
  scrollTop: number;
};

function findIndexForOffset(starts: readonly number[], sizes: readonly number[], offset: number) {
  if (starts.length === 0) {
    return 0;
  }

  let low = 0;
  let high = starts.length - 1;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    const start = starts[mid] ?? 0;
    const end = start + (sizes[mid] ?? 0);
    if (offset < start) {
      high = mid - 1;
      continue;
    }
    if (offset >= end) {
      low = mid + 1;
      continue;
    }
    return mid;
  }

  return Math.min(starts.length - 1, Math.max(0, low));
}

export function useVirtualizedRows<TRow>({
  rows,
  getRowId,
  estimateSize,
  overscan = 0,
  enabled = true,
  scrollRef,
}: UseVirtualizedRowsOptions<TRow>): UseVirtualizedRowsResult<TRow> {
  const measurementCacheRef = useRef(new Map<string, number>());
  const previousLayoutRef = useRef<LayoutSnapshot | null>(null);
  const [measurementVersion, setMeasurementVersion] = useState(0);
  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);
  const [scrollState, setScrollState] = useState({
    scrollTop: 0,
    viewportHeight: 0,
  });
  const syncScrollState = useCallback((element: HTMLElement | null) => {
    if (!element) {
      return;
    }
    setScrollState((previous) => {
      const nextState = {
        scrollTop: element.scrollTop,
        viewportHeight: element.clientHeight,
      };
      if (
        previous.scrollTop === nextState.scrollTop
        && previous.viewportHeight === nextState.viewportHeight
      ) {
        return previous;
      }
      return nextState;
    });
  }, []);

  useLayoutEffect(() => {
    const nextScrollElement = scrollRef.current;
    if (nextScrollElement !== scrollElement) {
      setScrollElement(nextScrollElement);
    }
    if (!nextScrollElement) {
      return;
    }
    syncScrollState(nextScrollElement);
  }, [scrollElement, scrollRef, syncScrollState]);

  const rowIds = useMemo(
    () => rows.map((row, index) => getRowId(row, index)),
    [rows, getRowId],
  );

  const readEstimateSize = useCallback(
    (row: TRow, index: number) =>
      typeof estimateSize === 'function'
        ? estimateSize(row, index)
        : estimateSize,
    [estimateSize],
  );

  useEffect(() => {
    const validIds = new Set(rowIds);
    let pruned = false;
    measurementCacheRef.current.forEach((_, key) => {
      if (!validIds.has(key)) {
        measurementCacheRef.current.delete(key);
        pruned = true;
      }
    });
    if (pruned) {
      setMeasurementVersion((value) => value + 1);
    }
  }, [rowIds]);

  useEffect(() => {
    if (!scrollElement || !enabled) {
      return undefined;
    }

    const handleScrollOrResize = () => {
      syncScrollState(scrollElement);
    };

    handleScrollOrResize();
    scrollElement.addEventListener('scroll', handleScrollOrResize, { passive: true });
    const resizeObserver = typeof ResizeObserver !== 'undefined'
      ? new ResizeObserver(() => {
          handleScrollOrResize();
        })
      : null;
    resizeObserver?.observe(scrollElement);
    window.addEventListener('resize', handleScrollOrResize);

    return () => {
      scrollElement.removeEventListener('scroll', handleScrollOrResize);
      resizeObserver?.disconnect();
      window.removeEventListener('resize', handleScrollOrResize);
    };
  }, [enabled, scrollElement, syncScrollState]);

  const layout = useMemo(() => {
    const sizes: number[] = [];
    const starts: number[] = [];
    let totalSize = 0;

    rows.forEach((row, index) => {
      const rowId = rowIds[index];
      const measured = rowId ? measurementCacheRef.current.get(rowId) : undefined;
      const size = measured ?? readEstimateSize(row, index);
      starts.push(totalSize);
      sizes.push(size);
      totalSize += size;
    });

    return {
      starts,
      sizes,
      totalSize,
    };
  }, [measurementVersion, readEstimateSize, rowIds, rows]);

  useLayoutEffect(() => {
    const previous = previousLayoutRef.current;
    if (scrollElement && previous && previous.rowIds.length > 0) {
      const anchorIndex = findIndexForOffset(previous.starts, previous.sizes, previous.scrollTop);
      const anchorId = previous.rowIds[anchorIndex];
      if (anchorId) {
        const oldStart = previous.starts[anchorIndex] ?? 0;
        const nextAnchorIndex = rowIds.indexOf(anchorId);
        if (nextAnchorIndex >= 0) {
          const newStart = layout.starts[nextAnchorIndex] ?? 0;
          const delta = newStart - oldStart;
          if (delta !== 0) {
            scrollElement.scrollTop = previous.scrollTop + delta;
            setScrollState({
              scrollTop: scrollElement.scrollTop,
              viewportHeight: scrollElement.clientHeight,
            });
          }
        }
      }
    }

    previousLayoutRef.current = {
      rowIds: rowIds.slice(),
      starts: layout.starts.slice(),
      sizes: layout.sizes.slice(),
      scrollTop: scrollElement?.scrollTop ?? scrollState.scrollTop,
    };
  }, [layout.sizes, layout.starts, rowIds, scrollElement, scrollState.scrollTop]);

  const visibleRange = useMemo(() => {
    if (!enabled || rows.length === 0) {
      return { start: 0, end: 0 };
    }

    const viewportHeight = scrollState.viewportHeight || scrollElement?.clientHeight || 0;
    const startIndex = findIndexForOffset(layout.starts, layout.sizes, scrollState.scrollTop);
    const endOffset = scrollState.scrollTop + viewportHeight;
    let endIndex = startIndex;
    while (endIndex < rows.length) {
      const rowStart = layout.starts[endIndex] ?? 0;
      if (rowStart >= endOffset) {
        break;
      }
      endIndex += 1;
    }

    return clampIndexRange(rows.length, {
      start: startIndex,
      end: Math.max(startIndex + 1, endIndex),
    });
  }, [enabled, layout.sizes, layout.starts, rows.length, scrollElement, scrollState.scrollTop, scrollState.viewportHeight]);

  const virtualRange = useMemo(
    () =>
      clampIndexRange(rows.length, {
        start: Math.max(0, visibleRange.start - overscan),
        end: visibleRange.end + overscan,
      }),
    [overscan, rows.length, visibleRange],
  );

  const virtualItems = useMemo(
    () =>
      rows.slice(virtualRange.start, virtualRange.end).map((row, offset) => {
        const index = virtualRange.start + offset;
        const size = layout.sizes[index] ?? 0;
        const start = layout.starts[index] ?? 0;
        return {
          index,
          key: rowIds[index] ?? String(index),
          row,
          start,
          size,
          end: start + size,
        };
      }),
    [layout.sizes, layout.starts, rowIds, rows, virtualRange],
  );

  const paddingTop = virtualItems[0]?.start ?? 0;
  const paddingBottom = virtualItems.length > 0
    ? Math.max(0, layout.totalSize - (virtualItems[virtualItems.length - 1]?.end ?? 0))
    : 0;

  const registerMeasurement = useCallback((rowId: string, size: number) => {
    if (!rowId || size <= 0) {
      return;
    }
    const previous = measurementCacheRef.current.get(rowId);
    if (previous === size) {
      return;
    }
    measurementCacheRef.current.set(rowId, size);
    setMeasurementVersion((value) => value + 1);
  }, []);

  const measureElement = useCallback(
    (rowId: string) => (node: HTMLElement | null) => {
      if (!node) {
        return;
      }
      const measured = node.getBoundingClientRect().height || node.offsetHeight;
      registerMeasurement(rowId, measured);
    },
    [registerMeasurement],
  );

  const scrollToIndex = useCallback(
    (index: number, align: 'start' | 'center' | 'end' = 'start') => {
      if (!scrollElement || index < 0 || index >= rows.length) {
        return;
      }

      const rowStart = layout.starts[index] ?? 0;
      const rowSize = layout.sizes[index] ?? 0;
      const viewportHeight = scrollElement.clientHeight || 0;
      let nextScrollTop = rowStart;

      if (align === 'center') {
        nextScrollTop = rowStart - Math.max(0, (viewportHeight - rowSize) / 2);
      } else if (align === 'end') {
        nextScrollTop = rowStart - Math.max(0, viewportHeight - rowSize);
      }

      scrollElement.scrollTop = Math.max(0, nextScrollTop);
      setScrollState({
        scrollTop: scrollElement.scrollTop,
        viewportHeight,
      });
    },
    [layout.sizes, layout.starts, rows.length, scrollElement],
  );

  return {
    visibleRange,
    virtualItems,
    totalSize: layout.totalSize,
    paddingTop,
    paddingBottom,
    measurementCache: measurementCacheRef.current,
    registerMeasurement,
    measureElement,
    scrollToIndex,
  };
}
