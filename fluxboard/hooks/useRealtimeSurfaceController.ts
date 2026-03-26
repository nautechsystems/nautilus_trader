import { useEffect, useRef, useState } from 'react';
import { createPendingBatcher, type BatchSchedule } from '@/lib/realtime/batching';
import {
  clampIndexRange,
  createVisibleRowsSelector,
  type IndexRange,
} from '@/lib/realtime/selectors';

export type RealtimeRowDelta<TRow> =
  | { kind: 'upsert'; row: TRow }
  | { kind: 'delete'; id: string };

export interface RealtimeSurfaceSnapshot<TRow> {
  rows: readonly TRow[];
  visibleRows: readonly TRow[];
  totalRows: number;
  dataVersion: number;
  orderVersion: number;
  visibleRange: IndexRange;
}

export interface RealtimeSurfaceControllerOptions<TRow> {
  getRowId: (row: TRow) => string;
  compareRows?: (left: TRow, right: TRow) => number;
  initialRows?: readonly TRow[];
  visibleRange?: Partial<IndexRange>;
  batchSchedule?: BatchSchedule;
}

export interface RealtimeSurfaceController<TRow> {
  getSnapshot: () => RealtimeSurfaceSnapshot<TRow>;
  subscribe: (listener: () => void) => () => void;
  applySnapshot: (rows: readonly TRow[]) => RealtimeSurfaceSnapshot<TRow>;
  applyDelta: (delta: RealtimeRowDelta<TRow> | readonly RealtimeRowDelta<TRow>[]) => RealtimeSurfaceSnapshot<TRow>;
  queueDelta: (delta: RealtimeRowDelta<TRow> | readonly RealtimeRowDelta<TRow>[]) => void;
  flush: () => readonly RealtimeRowDelta<TRow>[];
  clearQueuedDeltas: () => void;
  setVisibleRange: (range: Partial<IndexRange>) => RealtimeSurfaceSnapshot<TRow>;
  destroy: () => void;
}

function shallowCloneRow<TRow>(row: TRow): TRow {
  if (!row || typeof row !== 'object') {
    return row;
  }
  if (Array.isArray(row)) {
    return row.slice() as unknown as TRow;
  }
  return { ...(row as Record<string, unknown>) } as TRow;
}

function patchRowInPlace<TRow>(target: TRow, next: TRow): boolean {
  if (!target || typeof target !== 'object' || !next || typeof next !== 'object') {
    return false;
  }

  const targetRecord = target as Record<string, unknown>;
  const nextRecord = next as Record<string, unknown>;
  let changed = false;

  Object.keys(targetRecord).forEach((key) => {
    if (!(key in nextRecord)) {
      delete targetRecord[key];
      changed = true;
    }
  });

  Object.keys(nextRecord).forEach((key) => {
    if (targetRecord[key] !== nextRecord[key]) {
      targetRecord[key] = nextRecord[key];
      changed = true;
    }
  });

  return changed;
}

export function createRealtimeSurfaceController<TRow>({
  getRowId,
  compareRows,
  initialRows = [],
  visibleRange,
  batchSchedule,
}: RealtimeSurfaceControllerOptions<TRow>): RealtimeSurfaceController<TRow> {
  const listeners = new Set<() => void>();
  const selectVisibleRows = createVisibleRowsSelector<TRow>();
  let byId = new Map<string, TRow>();
  let order: string[] = [];
  let rows: readonly TRow[] = [];
  let dataVersion = 0;
  let orderVersion = 0;
  let visibleVersion = 0;
  let currentVisibleRange: IndexRange = visibleRange
    ? clampIndexRange(0, visibleRange)
    : { start: 0, end: Number.MAX_SAFE_INTEGER };

  const emit = () => {
    listeners.forEach((listener) => listener());
  };

  const buildRows = (nextOrder: readonly string[]) => nextOrder.map((id) => byId.get(id)).filter(Boolean) as TRow[];

  const updateSnapshot = ({
    emitChange = true,
    orderChanged = false,
    dataChanged = false,
    visibleChanged = false,
  }: {
    emitChange?: boolean;
    orderChanged?: boolean;
    dataChanged?: boolean;
    visibleChanged?: boolean;
  }) => {
    currentVisibleRange = clampIndexRange(rows.length, currentVisibleRange);
    if (dataChanged) {
      dataVersion += 1;
    }
    if (orderChanged) {
      orderVersion += 1;
    }
    if (visibleChanged) {
      visibleVersion += 1;
    }
    if (emitChange && (dataChanged || orderChanged)) {
      emit();
    }
    return controller.getSnapshot();
  };

  const controller: RealtimeSurfaceController<TRow> = {
    getSnapshot() {
      const resolvedVisibleRange = clampIndexRange(rows.length, currentVisibleRange);
      return {
        rows,
        visibleRows: selectVisibleRows(rows, resolvedVisibleRange, visibleVersion),
        totalRows: rows.length,
        dataVersion,
        orderVersion,
        visibleRange: resolvedVisibleRange,
      };
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    applySnapshot(nextRows) {
      byId = new Map(nextRows.map((row) => [getRowId(row), shallowCloneRow(row)]));
      order = nextRows.map((row) => getRowId(row));
      if (compareRows) {
        order.sort((leftId, rightId) => compareRows(byId.get(leftId) as TRow, byId.get(rightId) as TRow));
      }
      rows = buildRows(order);
      return updateSnapshot({ orderChanged: true, dataChanged: true, visibleChanged: true });
    },
    applyDelta(deltaInput) {
      const deltas = Array.isArray(deltaInput) ? deltaInput : [deltaInput];
      if (deltas.length === 0) {
        return controller.getSnapshot();
      }

      let changed = false;
      let orderChanged = false;
      let visibleChanged = false;
      const resolvedVisibleRange = clampIndexRange(rows.length, currentVisibleRange);

      for (const delta of deltas) {
        if (delta.kind === 'delete') {
          const deletedIndex = order.indexOf(delta.id);
          if (!byId.has(delta.id)) {
            continue;
          }
          byId.delete(delta.id);
          order = order.filter((rowId) => rowId !== delta.id);
          rows = buildRows(order);
          changed = true;
          orderChanged = true;
          if (deletedIndex >= resolvedVisibleRange.start && deletedIndex < resolvedVisibleRange.end) {
            visibleChanged = true;
          }
          continue;
        }

        const nextId = getRowId(delta.row);
        const existing = byId.get(nextId);

        if (!existing) {
          byId.set(nextId, shallowCloneRow(delta.row));
          order.push(nextId);
          if (compareRows) {
            order.sort((leftId, rightId) => compareRows(byId.get(leftId) as TRow, byId.get(rightId) as TRow));
          }
          rows = buildRows(order);
          changed = true;
          orderChanged = true;
          visibleChanged = true;
          continue;
        }

        const rowIndex = order.indexOf(nextId);
        const nextRow = shallowCloneRow(delta.row);
        const rowChanged = patchRowInPlace(existing, nextRow);
        if (!rowChanged) {
          continue;
        }

        changed = true;
        if (rowIndex >= resolvedVisibleRange.start && rowIndex < resolvedVisibleRange.end) {
          visibleChanged = true;
        }

        if (compareRows && rowIndex >= 0) {
          const previousId = order[rowIndex - 1];
          const nextIdInOrder = order[rowIndex + 1];
          const previousRow = previousId ? byId.get(previousId) : undefined;
          const nextRowInOrder = nextIdInOrder ? byId.get(nextIdInOrder) : undefined;
          const violatesPrevious = previousRow ? compareRows(previousRow, existing) > 0 : false;
          const violatesNext = nextRowInOrder ? compareRows(existing, nextRowInOrder) > 0 : false;
          if (violatesPrevious || violatesNext) {
            order.sort((leftId, rightId) => compareRows(byId.get(leftId) as TRow, byId.get(rightId) as TRow));
            rows = buildRows(order);
            orderChanged = true;
            visibleChanged = true;
          }
        }
      }

      if (!changed) {
        return controller.getSnapshot();
      }

      if (!orderChanged && rows.length === 0) {
        rows = buildRows(order);
      }

      return updateSnapshot({ orderChanged, dataChanged: true, visibleChanged: visibleChanged || orderChanged });
    },
    queueDelta(delta) {
      batcher.enqueue(delta);
    },
    flush() {
      return batcher.flush();
    },
    clearQueuedDeltas() {
      batcher.cancel();
    },
    setVisibleRange(range) {
      const nextRange = clampIndexRange(rows.length, range);
      if (
        nextRange.start === currentVisibleRange.start
        && nextRange.end === currentVisibleRange.end
      ) {
        return controller.getSnapshot();
      }
      currentVisibleRange = nextRange;
      emit();
      return controller.getSnapshot();
    },
    destroy() {
      batcher.cancel();
      listeners.clear();
    },
  };

  const batcher = createPendingBatcher<RealtimeRowDelta<TRow>>({
    schedule: batchSchedule,
    onFlush: (items) => {
      controller.applyDelta(items);
    },
  });

  if (initialRows.length > 0) {
    controller.applySnapshot(initialRows);
  }

  return controller;
}

export function useRealtimeSurfaceController<TRow, TSelected = RealtimeSurfaceSnapshot<TRow>>(
  controller: RealtimeSurfaceController<TRow>,
  selector: (snapshot: RealtimeSurfaceSnapshot<TRow>) => TSelected = ((snapshot) => snapshot as unknown as TSelected),
  isEqual: (left: TSelected, right: TSelected) => boolean = Object.is,
) {
  const selectorRef = useRef(selector);
  const isEqualRef = useRef(isEqual);
  selectorRef.current = selector;
  isEqualRef.current = isEqual;

  const [selected, setSelected] = useState(() => selector(controller.getSnapshot()));

  useEffect(() => {
    setSelected(selectorRef.current(controller.getSnapshot()));
    return controller.subscribe(() => {
      const nextSelected = selectorRef.current(controller.getSnapshot());
      setSelected((previous) => (isEqualRef.current(previous, nextSelected) ? previous : nextSelected));
    });
  }, [controller]);

  return selected;
}
