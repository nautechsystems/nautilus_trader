import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { type ColumnDef } from '@tanstack/react-table';
import { TimeAgo } from '@/components/shared/TimeAgo';
import { DataTable, type DataTableDebugMetrics } from '@/components/ui/table/DataTable';
import {
  createRealtimeSurfaceController,
  useRealtimeSurfaceController,
} from '@/hooks/useRealtimeSurfaceController';
import {
  type RealtimeChannelAdapter,
  useRealtimeChannel,
} from '@/hooks/useRealtimeChannel';
import { __resetViewportClockRegistryForTests } from '@/hooks/useViewportClock';

type Row = {
  id: string;
  rank: number;
  name: string;
  updatedAt: number;
  visibleFreshness: boolean;
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

describe('realtime subscription contract', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-03-19T00:00:00.000Z'));
    __resetViewportClockRegistryForTests();
  });

  afterEach(() => {
    __resetViewportClockRegistryForTests();
    vi.useRealTimers();
  });

  it('updates one row without invalidating the full table model and only ticks visible freshness cells', () => {
    const controller = createController();
    const debugMetrics: DataTableDebugMetrics[] = [];
    const tableRenderCount = { current: 0 };
    const connections: Array<Parameters<RealtimeChannelAdapter<Row[], any>['connect']>[0]> = [];
    const adapter: RealtimeChannelAdapter<Row[], any> = {
      connect: vi.fn((handlers) => {
        connections.push(handlers);
        return vi.fn();
      }),
    };

    const columns: ColumnDef<Row>[] = [
      {
        accessorKey: 'name',
        header: 'Name',
      },
      {
        accessorKey: 'updatedAt',
        header: 'Age',
        cell: ({ row }) => {
          const current = row.original;
          return (
            <TimeAgo
              timestamp={current.updatedAt}
              clockKey="panel:contract"
              clockId={`age:${current.id}`}
              isVisible={current.visibleFreshness}
            />
          );
        },
      },
    ];

    function Harness() {
      tableRenderCount.current += 1;
      const { rows, dataVersion } = useRealtimeSurfaceController(controller, (snapshot) => ({
        rows: snapshot.rows,
        dataVersion: snapshot.dataVersion,
      }));

      useRealtimeChannel({
        channelKey: 'trades',
        adapter,
        controller,
        recoveryBaseDelayMs: 1_000,
        recoveryMaxDelayMs: 4_000,
      });

      return (
        <DataTable<Row>
          data={rows as Row[]}
          columns={columns}
          getRowId={(row) => row.id}
          liveDataVersion={dataVersion}
          onDebugMetrics={(metrics) => {
            debugMetrics.push(metrics);
          }}
        />
      );
    }

    render(<Harness />);

    act(() => {
      connections[0]?.onOpen?.();
      connections[0]?.onSnapshot?.([
        {
          id: 'alpha',
          rank: 2,
          name: 'Alpha',
          updatedAt: Date.now() - 1_000,
          visibleFreshness: true,
        },
        {
          id: 'beta',
          rank: 1,
          name: 'Beta',
          updatedAt: Date.now() - 1_000,
          visibleFreshness: false,
        },
      ]);
    });

    expect(screen.getByText('Alpha')).toBeInTheDocument();
    expect(screen.getByText('Beta')).toBeInTheDocument();
    expect(screen.getAllByText('1s')).toHaveLength(2);

    act(() => {
      connections[0]?.onDelta?.({
        kind: 'upsert',
        row: {
          id: 'alpha',
          rank: 2,
          name: 'Alpha+',
          updatedAt: Date.now() - 1_000,
          visibleFreshness: true,
        },
      });
      vi.advanceTimersByTime(0);
    });

    expect(screen.getByText('Alpha+')).toBeInTheDocument();
    expect(debugMetrics.at(-1)?.coreRowModelInvalidated).toBe(false);

    const rendersBeforeTick = tableRenderCount.current;

    act(() => {
      vi.advanceTimersByTime(2_000);
    });

    expect(tableRenderCount.current).toBe(rendersBeforeTick);
    expect(screen.getByText('3s')).toBeInTheDocument();
    expect(screen.getByText('1s')).toBeInTheDocument();
  });
});
