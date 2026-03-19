import React from 'react';
import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  createRealtimeSurfaceController,
  useRealtimeSurfaceController,
  type RealtimeRowDelta,
} from '@/hooks/useRealtimeSurfaceController';

type Row = {
  id: string;
  rank: number;
  value: string;
};

function createController() {
  return createRealtimeSurfaceController<Row>({
    getRowId: (row) => row.id,
    compareRows: (left, right) => right.rank - left.rank,
    batchSchedule: (flush) => {
      const id = window.setTimeout(flush, 0);
      return () => window.clearTimeout(id);
    },
  });
}

describe('useRealtimeSurfaceController', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it('keeps the canonical rows array stable for one-row deltas under stable ordering', () => {
    const controller = createController();
    controller.applySnapshot([
      { id: 'alpha', rank: 3, value: 'A' },
      { id: 'beta', rank: 2, value: 'B' },
    ]);

    const first = controller.getSnapshot();
    const firstRows = first.rows;
    const firstAlpha = first.rows[0];

    controller.applyDelta({
      kind: 'upsert',
      row: { id: 'alpha', rank: 3, value: 'A+' },
    });

    const second = controller.getSnapshot();
    expect(second.rows).toBe(firstRows);
    expect(second.rows[0]).toBe(firstAlpha);
    expect(second.rows[0]?.value).toBe('A+');
    expect(second.orderVersion).toBe(first.orderVersion);
    expect(second.dataVersion).toBe(first.dataVersion + 1);
  });

  it('rebuilds ordering when a delta changes the sort position', () => {
    const controller = createController();
    controller.applySnapshot([
      { id: 'alpha', rank: 3, value: 'A' },
      { id: 'beta', rank: 2, value: 'B' },
    ]);

    const first = controller.getSnapshot();

    controller.applyDelta({
      kind: 'upsert',
      row: { id: 'beta', rank: 5, value: 'B+' },
    });

    const second = controller.getSnapshot();
    expect(second.rows).not.toBe(first.rows);
    expect(second.rows.map((row) => row.id)).toEqual(['beta', 'alpha']);
    expect(second.orderVersion).toBe(first.orderVersion + 1);
  });

  it('keeps visible-range updates out of row subscribers', () => {
    const controller = createController();
    controller.applySnapshot([
      { id: 'alpha', rank: 4, value: 'A' },
      { id: 'beta', rank: 3, value: 'B' },
      { id: 'gamma', rank: 2, value: 'C' },
      { id: 'delta', rank: 1, value: 'D' },
    ]);

    const rowsRenders: string[] = [];
    const visibleRenders: string[] = [];

    function RowsProbe() {
      const rows = useRealtimeSurfaceController(controller, (snapshot) => snapshot.rows);
      rowsRenders.push(rows.map((row) => row.id).join(','));
      return null;
    }

    function VisibleProbe() {
      const visibleRows = useRealtimeSurfaceController(
        controller,
        (snapshot) => snapshot.visibleRows.map((row) => row.id).join(','),
      );
      visibleRenders.push(visibleRows);
      return null;
    }

    render(
      React.createElement(
        React.Fragment,
        null,
        React.createElement(RowsProbe),
        React.createElement(VisibleProbe),
      ),
    );

    act(() => {
      controller.setVisibleRange({ start: 0, end: 2 });
    });
    act(() => {
      controller.setVisibleRange({ start: 1, end: 3 });
    });

    expect(rowsRenders).toHaveLength(1);
    expect(visibleRenders).toEqual(['alpha,beta,gamma,delta', 'alpha,beta', 'beta,gamma']);
  });

  it('batches queued deltas until the scheduled flush boundary', () => {
    const controller = createController();
    controller.applySnapshot([
      { id: 'alpha', rank: 3, value: 'A' },
      { id: 'beta', rank: 2, value: 'B' },
    ]);

    const firstVersion = controller.getSnapshot().dataVersion;

    controller.queueDelta({
      kind: 'upsert',
      row: { id: 'alpha', rank: 3, value: 'A+' },
    } satisfies RealtimeRowDelta<Row>);
    controller.queueDelta({
      kind: 'upsert',
      row: { id: 'beta', rank: 2, value: 'B+' },
    } satisfies RealtimeRowDelta<Row>);

    expect(controller.getSnapshot().dataVersion).toBe(firstVersion);

    act(() => {
      vi.runOnlyPendingTimers();
    });

    const snapshot = controller.getSnapshot();
    expect(snapshot.rows[0]?.value).toBe('A+');
    expect(snapshot.rows[1]?.value).toBe('B+');
    expect(snapshot.dataVersion).toBe(firstVersion + 1);
  });
});
