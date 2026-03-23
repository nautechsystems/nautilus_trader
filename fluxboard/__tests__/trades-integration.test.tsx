import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, act, cleanup, screen } from '@testing-library/react';
import Trades from '../Trades';
import { useTradesStore } from '../stores';

const { realtimeFlags } = vi.hoisted(() => ({
  realtimeFlags: {
    trades: false,
  },
}));

const mockTable = vi.fn(({ trades }: any) => (
  <div data-testid="mock-table">{trades?.map((row: any) => row.row_id).join(',')}</div>
));

vi.mock('../components/trades/TradesTable', () => ({
  TradesTable: (props: any) => mockTable(props),
}));

vi.mock('../utils/sound', () => ({
  playTradeClick: vi.fn(),
}));

const { socketHandlers, socketMock, getTrades, getTradesDelta, setNextSubscribeAck } = vi.hoisted(() => {
  const socketHandlers: Record<string, (msg: any) => void> = {};
  const subscribeAckState = { current: null as any };
  const socketMock = {
    on: vi.fn((event: string, handler: (msg: any) => void) => {
      socketHandlers[event] = handler;
    }),
    off: vi.fn((event: string, handler?: (msg: any) => void) => {
      if (!handler || socketHandlers[event] === handler) {
        delete socketHandlers[event];
      }
    }),
    emit: vi.fn((event: string, payload?: any, ack?: (response: any) => void) => {
      if (event === 'subscribe' && typeof ack === 'function') {
        const requested = payload ?? {};
        const response = subscribeAckState.current ?? {
          accepted: true,
          contract_version: requested.contract_version,
          surface: requested.surface,
          profile: requested.profile,
          surface_query_key: requested.surface_query_key,
          stream_id: requested.stream_id,
          snapshot_revision: requested.snapshot_revision,
          accepted_start_seq: requested.resume_from_seq,
          last_seq: requested.resume_from_seq,
          requested_resume_from_seq: requested.resume_from_seq,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        };
        subscribeAckState.current = null;
        ack(response);
      }
      if (event === 'unsubscribe' && typeof ack === 'function') {
        ack({ ok: true, surface: payload?.surface ?? null });
      }
      return true;
    }),
    connected: true,
  };
  return {
    socketHandlers,
    socketMock,
    getTrades: vi.fn(),
    getTradesDelta: vi.fn(),
    setNextSubscribeAck: (ack: any) => {
      subscribeAckState.current = ack;
    },
  };
});

vi.mock('../sockets', () => ({
  socket: socketMock,
  standardSocketClient: {
    subscribe: vi.fn(({
      lineage,
      resumeFromSeq,
      onEvent,
      onFailure,
      onSubscribed,
    }: any) => {
      const request = {
        contract_version: lineage.contract_version,
        surface: lineage.surface,
        profile: lineage.profile,
        surface_query_key: lineage.surface_query_key,
        stream_id: lineage.stream_id,
        snapshot_revision: lineage.snapshot_revision,
        resume_from_seq:
          typeof resumeFromSeq === 'function'
            ? resumeFromSeq()
            : (resumeFromSeq ?? lineage.last_seq),
      };
      const eventHandler = (payload?: any) => {
        if (!payload || typeof payload !== 'object') {
          return;
        }
        if (
          payload.surface !== lineage.surface
          || payload.profile !== lineage.profile
          || payload.stream_id !== lineage.stream_id
          || String(payload.snapshot_revision) !== String(lineage.snapshot_revision)
        ) {
          return;
        }
        if (payload.kind === 'recovery_required') {
          onFailure?.({
            type: 'recovery_required',
            reason: String(payload.reason ?? 'recovery_required'),
            requested: request,
            event: payload,
          });
          return;
        }
        onEvent?.(payload);
      };

      socketHandlers.realtime_event = eventHandler;
      if (socketMock.connected) {
        socketMock.emit('subscribe', request, (ack: any) => {
          if (!ack?.accepted) {
            onFailure?.({
              type: 'subscribe_rejected',
              reason: String(ack?.reason ?? 'subscribe_rejected'),
              requested: request,
              ack,
            });
            return;
          }
          const matchesLineage =
            ack.contract_version === request.contract_version
            && ack.surface === request.surface
            && ack.profile === request.profile
            && ack.surface_query_key === request.surface_query_key
            && ack.stream_id === request.stream_id
            && String(ack.snapshot_revision) === String(request.snapshot_revision);
          if (!matchesLineage) {
            onFailure?.({
              type: 'lineage_mismatch',
              reason: 'ack_lineage_mismatch',
              requested: request,
              ack,
            });
            return;
          }
          if (
            typeof ack.accepted_start_seq === 'number'
            && ack.accepted_start_seq !== request.resume_from_seq
          ) {
            onFailure?.({
              type: 'lineage_mismatch',
              reason: 'accepted_start_seq_mismatch',
              requested: request,
              ack,
            });
            return;
          }
          onSubscribed?.(ack);
        });
      }

      return () => {
        if (socketHandlers.realtime_event === eventHandler) {
          delete socketHandlers.realtime_event;
        }
        socketMock.emit('unsubscribe', { surface: lineage.surface });
      };
    }),
  },
}));

vi.mock('../config/featureFlags', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    isRealtimeStandardEnabled: (surface: string) => Boolean((realtimeFlags as Record<string, boolean>)[surface]),
  };
});

vi.mock('../api', async (importOriginal) => {
  const mod = await importOriginal<any>();
  return {
    ...mod,
    api: {
      ...mod.api,
      getTrades,
      getTradesDelta,
    },
    deriveCanonicalNaming: vi.fn(() => ({})),
  };
});

const baseRows = [
  {
    row_id: 'old',
    seq: 1,
    version: 1,
    ts: 1,
    time: '2025-01-01T00:00:01Z',
    coin: 'PLUME/USDT',
    exchange: 'bybit',
    side: 'buy',
    price: 100,
  },
  {
    row_id: 'new',
    seq: 2,
    version: 1,
    ts: 2,
    time: '2025-01-01T00:00:02Z',
    coin: 'PLUME/USDT',
    exchange: 'bybit',
    side: 'sell',
    price: 101,
  },
];

const makeTradeRow = (overrides: Record<string, unknown> = {}) => ({
  row_id: 'trade-row',
  seq: 1,
  version: 1,
  ts: 1,
  time: '2025-01-01T00:00:01Z',
  coin: 'PLUME/USDT',
  exchange: 'bybit',
  side: 'buy',
  price: 100,
  ...overrides,
});

const createDeferred = <T,>() => {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

const flushTradeSocketFrame = async () => {
  await act(async () => {
    await new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => resolve());
    });
  });
};

describe('Trades integration flows', () => {
  beforeEach(() => {
    window.sessionStorage.clear();
    getTrades.mockReset();
    getTradesDelta.mockReset();
    mockTable.mockClear();
    socketMock.on.mockClear();
    socketMock.off.mockClear();
    socketMock.emit.mockClear();
    realtimeFlags.trades = false;
    setNextSubscribeAck(null);
    Object.keys(socketHandlers).forEach((key) => delete socketHandlers[key]);
    useTradesStore.getState().clear();
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      stream_id: 'trades-main',
      snapshot_revision: 17,
      realtime: undefined,
    });
    getTradesDelta.mockResolvedValue({
      rows: [],
      last_seq: 2,
      reset_required: false,
      stream_id: 'trades-main',
      snapshot_revision: 17,
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('loads newest-first snapshot using ts_desc by default', async () => {
    render(<Trades />);

    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    const firstCall = getTrades.mock.calls[0];
    expect(firstCall[0]).toBe(1);
    expect(firstCall[1]).toBe(100);
    expect(firstCall[2]).toMatchObject({ sort: 'ts_desc' });
  });

  it('subscribes trades to realtime_event using backend lineage metadata when the standard flag is on', async () => {
    realtimeFlags.trades = true;
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 0,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    render(<Trades />);

    await waitFor(() => expect(getTrades).toHaveBeenCalled());
    const firstCall = getTrades.mock.calls[0];
    expect(firstCall[0]).toBe(1);
    expect(firstCall[1]).toBe(50);
    expect(firstCall[2]).toMatchObject({
      sort: 'ts_desc',
      contract_version: 2,
    });

    await waitFor(() => {
      expect(socketMock.emit).toHaveBeenCalledWith(
        'subscribe',
        expect.objectContaining({
          contract_version: 2,
          surface: 'trades',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          resume_from_seq: 2,
        }),
        expect.any(Function),
      );
    });

    expect(socketHandlers.realtime_event).toBeTypeOf('function');
    expect(socketHandlers.trade_update).toBeUndefined();
  });

  it('unsubscribes the standard trades stream when the view leaves the canonical live-compatible page shape', async () => {
    realtimeFlags.trades = true;
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 0,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    render(<Trades />);

    await waitFor(() => {
      expect(socketMock.emit).toHaveBeenCalledWith(
        'subscribe',
        expect.objectContaining({ surface: 'trades' }),
        expect.any(Function),
      );
    });

    const pageSizeControl = screen.getByLabelText('Page size');
    act(() => {
      pageSizeControl.dispatchEvent(new Event('focus', { bubbles: true }));
    });
    act(() => {
      (pageSizeControl as HTMLSelectElement).value = '100';
      pageSizeControl.dispatchEvent(new Event('change', { bubbles: true }));
    });

    await waitFor(() => {
      expect(socketMock.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'trades' });
    });
  });

  it('applies live trade_update events to the top of the table', async () => {
    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'live',
        seq: 99,
        version: 1,
        ts: 99,
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
      });
    });

    await flushTradeSocketFrame();

    await waitFor(() => {
      const lastCall = mockTable.mock.calls[mockTable.mock.calls.length - 1];
      expect(lastCall).toBeTruthy();
      const props = lastCall[0];
      expect(props.trades?.[0].row_id).toBe('live');
    });
  });

  it('applies matching realtime_event delta batches to the top of the table when the standard flag is on', async () => {
    realtimeFlags.trades = true;
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 0,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    render(<Trades />);
    await waitFor(() => expect(socketHandlers.realtime_event).toBeTypeOf('function'));

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'other-stream',
        profile: 'default',
        kind: 'delta_batch',
        seq: 1,
        snapshot_revision: 'other-snap',
        server_ts_ms: 1_700_000_000_001,
        payload: {
          trades: [
            {
              op: 'upsert',
              row_id: 'ignored-live',
              seq: 11,
              version: 1,
              ts: 11,
              coin: 'PLUME/USDT',
              exchange: 'bybit',
              side: 'buy',
            },
          ],
        },
      });
    });

    await waitFor(() => {
      const props = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
      expect(props.trades?.[0].row_id).not.toBe('ignored-live');
    });

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'delta_batch',
        seq: 2,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_002,
        payload: {
          trades: [
            {
              op: 'upsert',
              row_id: 'live-standard',
              seq: 12,
              version: 1,
              ts: 12,
              coin: 'PLUME/USDT',
              exchange: 'bybit',
              side: 'buy',
            },
          ],
        },
      });
    });

    await flushTradeSocketFrame();

    await waitFor(() => {
      const props = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
      expect(props.trades?.[0].row_id).toBe('live-standard');
      expect(props.trades?.[0].seq).toBe(12);
    });
  });

  it('fails closed into manual refresh required when standard subscribe is rejected', async () => {
    realtimeFlags.trades = true;
    setNextSubscribeAck({
      accepted: false,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      requested_resume_from_seq: 0,
      reason: 'backend_kill_switch',
    });
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 0,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    render(<Trades />);

    await waitFor(() => {
      expect(screen.getByText('MANUAL REFRESH REQUIRED')).toBeInTheDocument();
    });
  });

  it('fails closed into manual refresh required on mid-session capability withdrawal', async () => {
    realtimeFlags.trades = true;
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 0,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    render(<Trades />);
    await waitFor(() => expect(socketHandlers.realtime_event).toBeTypeOf('function'));

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'recovery_required',
        seq: 3,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_003,
        reason: 'capability_withdrawn',
        payload: {},
      });
    });

    await waitFor(() => {
      expect(screen.getByText('MANUAL REFRESH REQUIRED')).toBeInTheDocument();
    });
  });

  it('keeps manual refresh required sticky across reconnects in standard mode', async () => {
    realtimeFlags.trades = true;
    getTrades.mockResolvedValue({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 2,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    render(<Trades />);
    await waitFor(() => expect(socketHandlers.realtime_event).toBeTypeOf('function'));

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'recovery_required',
        seq: 3,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_003,
        reason: 'capability_withdrawn',
        payload: {},
      });
    });

    await waitFor(() => {
      expect(screen.getByText('MANUAL REFRESH REQUIRED')).toBeInTheDocument();
    });

    getTrades.mockClear();

    act(() => {
      socketHandlers.connect?.();
    });

    expect(getTrades).not.toHaveBeenCalled();
    expect(screen.getByText('MANUAL REFRESH REQUIRED')).toBeInTheDocument();
  });

  it('starts delta recovery when a standard delta batch arrives with a surface seq gap', async () => {
    realtimeFlags.trades = true;
    getTrades.mockResolvedValueOnce({
      rows: baseRows,
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 2,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 2,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });
    getTradesDelta.mockResolvedValueOnce({
      rows: [
        makeTradeRow({
          row_id: 'gap-3',
          seq: 3,
          ts: 3,
          time: '2025-01-01T00:00:03Z',
          side: 'sell',
        }),
        makeTradeRow({
          row_id: 'gap-4',
          seq: 4,
          ts: 4,
          time: '2025-01-01T00:00:04Z',
        }),
        makeTradeRow({
          row_id: 'recovered-standard',
          seq: 5,
          ts: 5,
          time: '2025-01-01T00:00:05Z',
          coin: 'RECOVERED',
        }),
      ],
      last_seq: 5,
      reset_required: false,
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
    });

    render(<Trades />);
    await waitFor(() => expect(socketHandlers.realtime_event).toBeTypeOf('function'));

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'delta_batch',
        seq: 5,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_005,
        payload: {
          trades: [
            makeTradeRow({
              row_id: 'gap-standard',
              seq: 12,
              ts: 12,
              time: '2025-01-01T00:00:12Z',
              coin: 'GAP',
            }),
          ],
        },
      });
    });

    await flushTradeSocketFrame();

    const propsAfterGap = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(propsAfterGap.trades?.map((row: any) => row.row_id)).not.toContain('gap-standard');

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => {
      expect(getTradesDelta).toHaveBeenCalledTimes(1);
    });
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 'snap-1',
      }),
      500,
    );
  });

  it('re-snapshots and re-subscribes when the server withdraws the trades stream with trade_gap recovery', async () => {
    realtimeFlags.trades = true;
    getTrades
      .mockResolvedValueOnce({
        rows: baseRows,
        total: baseRows.length,
        page: 1,
        page_size: 50,
        last_seq: 2,
        realtime: {
          contract_version: 2,
          surface: 'trades',
          profile: 'default',
          surface_query_key: 'trades|profile=default',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          last_seq: 0,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        },
      })
      .mockResolvedValueOnce({
        rows: [{ ...baseRows[0], row_id: 'recovered', seq: 9, ts: 9 }],
        total: baseRows.length,
        page: 1,
        page_size: 50,
        last_seq: 9,
        realtime: {
          contract_version: 2,
          surface: 'trades',
          profile: 'default',
          surface_query_key: 'trades|profile=default',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          last_seq: 9,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        },
      });

    render(<Trades />);
    await waitFor(() => expect(socketHandlers.realtime_event).toBeTypeOf('function'));

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'recovery_required',
        seq: 3,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_003,
        reason: 'trade_gap',
        payload: {},
      });
    });

    await waitFor(() => {
      expect(getTrades).toHaveBeenCalledTimes(2);
    });

    await waitFor(() => {
      const subscribeCalls = socketMock.emit.mock.calls.filter(([event]) => event === 'subscribe');
      expect(subscribeCalls.length).toBeGreaterThanOrEqual(2);
      expect(subscribeCalls[subscribeCalls.length - 1]?.[1]).toMatchObject({
        surface: 'trades',
        resume_from_seq: 9,
      });
    });
  });

  it('recovers with a fresh snapshot instead of failing closed on accepted_start_seq drift', async () => {
    realtimeFlags.trades = true;
    setNextSubscribeAck({
      accepted: true,
      contract_version: 2,
      surface: 'trades',
      profile: 'default',
      surface_query_key: 'trades|profile=default',
      stream_id: 'trades-main',
      snapshot_revision: 'snap-1',
      accepted_start_seq: 9,
      last_seq: 9,
      requested_resume_from_seq: 2,
    });
    getTrades
      .mockResolvedValueOnce({
        rows: baseRows,
        total: baseRows.length,
        page: 1,
        page_size: 50,
        last_seq: 2,
        realtime: {
          contract_version: 2,
          surface: 'trades',
          profile: 'default',
          surface_query_key: 'trades|profile=default',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          last_seq: 2,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        },
      })
      .mockResolvedValueOnce({
        rows: [{ ...baseRows[0], row_id: 'recovered', seq: 9, ts: 9 }],
        total: baseRows.length,
        page: 1,
        page_size: 50,
        last_seq: 9,
        realtime: {
          contract_version: 2,
          surface: 'trades',
          profile: 'default',
          surface_query_key: 'trades|profile=default',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          last_seq: 9,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        },
      });

    render(<Trades />);

    await waitFor(() => {
      expect(getTrades).toHaveBeenCalledTimes(2);
    });

    expect(screen.queryByText('MANUAL REFRESH REQUIRED')).not.toBeInTheDocument();

    await waitFor(() => {
      const subscribeCalls = socketMock.emit.mock.calls.filter(([event]) => event === 'subscribe');
      expect(subscribeCalls.length).toBeGreaterThanOrEqual(2);
      expect(subscribeCalls[subscribeCalls.length - 1]?.[1]).toMatchObject({
        surface: 'trades',
        resume_from_seq: 9,
      });
    });
  });

  it('waits for a fresh canonical snapshot before re-subscribing after leaving the canonical view', async () => {
    realtimeFlags.trades = true;
    const canonicalReentry = createDeferred<any>();
    getTrades
      .mockResolvedValueOnce({
        rows: baseRows,
        total: baseRows.length,
        page: 1,
        page_size: 50,
        last_seq: 2,
        realtime: {
          contract_version: 2,
          surface: 'trades',
          profile: 'default',
          surface_query_key: 'trades|profile=default',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          last_seq: 2,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        },
      })
      .mockResolvedValueOnce({
        rows: baseRows,
        total: baseRows.length,
        page: 1,
        page_size: 100,
        last_seq: 2,
      })
      .mockImplementationOnce(() => canonicalReentry.promise);

    render(<Trades />);

    await waitFor(() => {
      expect(socketMock.emit).toHaveBeenCalledWith(
        'subscribe',
        expect.objectContaining({ surface: 'trades', snapshot_revision: 'snap-1' }),
        expect.any(Function),
      );
    });

    const pageSizeControl = screen.getByLabelText('Page size');
    act(() => {
      (pageSizeControl as HTMLSelectElement).value = '100';
      pageSizeControl.dispatchEvent(new Event('change', { bubbles: true }));
    });

    await waitFor(() => {
      expect(socketMock.emit).toHaveBeenCalledWith('unsubscribe', { surface: 'trades' });
    });

    socketMock.emit.mockClear();

    act(() => {
      (pageSizeControl as HTMLSelectElement).value = '50';
      pageSizeControl.dispatchEvent(new Event('change', { bubbles: true }));
    });

    expect(socketMock.emit).not.toHaveBeenCalledWith(
      'subscribe',
      expect.anything(),
      expect.any(Function),
    );

    canonicalReentry.resolve({
      rows: [{ ...baseRows[0], row_id: 'canonical-return', seq: 12, ts: 12 }],
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 12,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main-2',
        snapshot_revision: 'snap-2',
        last_seq: 12,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    await waitFor(() => {
      expect(socketMock.emit).toHaveBeenCalledWith(
        'subscribe',
        expect.objectContaining({
          surface: 'trades',
          stream_id: 'trades-main-2',
          snapshot_revision: 'snap-2',
          resume_from_seq: 12,
        }),
        expect.any(Function),
      );
    });
  });

  it('keeps manual refresh required when a queued recovery snapshot resolves late', async () => {
    realtimeFlags.trades = true;
    const deferredRecovery = createDeferred<any>();
    getTrades
      .mockResolvedValueOnce({
        rows: baseRows,
        total: baseRows.length,
        page: 1,
        page_size: 50,
        last_seq: 2,
        realtime: {
          contract_version: 2,
          surface: 'trades',
          profile: 'default',
          surface_query_key: 'trades|profile=default',
          stream_id: 'trades-main',
          snapshot_revision: 'snap-1',
          last_seq: 2,
          capabilities: {
            recovery_mode: 'invalidate_only',
            replay_supported: false,
            transport_mode: 'polling_only',
          },
        },
      })
      .mockImplementationOnce(() => deferredRecovery.promise);

    render(<Trades />);
    await waitFor(() => expect(socketHandlers.realtime_event).toBeTypeOf('function'));

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'invalidate',
        seq: 3,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_003,
        reason: 'refresh_required',
        payload: {},
      });
    });

    await waitFor(() => {
      expect(getTrades).toHaveBeenCalledTimes(2);
    });

    act(() => {
      socketHandlers.realtime_event?.({
        contract_version: 2,
        surface: 'trades',
        stream_id: 'trades-main',
        profile: 'default',
        kind: 'recovery_required',
        seq: 4,
        snapshot_revision: 'snap-1',
        server_ts_ms: 1_700_000_000_004,
        reason: 'capability_withdrawn',
        payload: {},
      });
    });

    await waitFor(() => {
      expect(screen.getByText('MANUAL REFRESH REQUIRED')).toBeInTheDocument();
    });

    deferredRecovery.resolve({
      rows: [{ ...baseRows[0], row_id: 'late-recovery', seq: 8, ts: 8 }],
      total: baseRows.length,
      page: 1,
      page_size: 50,
      last_seq: 8,
      realtime: {
        contract_version: 2,
        surface: 'trades',
        profile: 'default',
        surface_query_key: 'trades|profile=default',
        stream_id: 'trades-main',
        snapshot_revision: 'snap-1',
        last_seq: 8,
        capabilities: {
          recovery_mode: 'invalidate_only',
          replay_supported: false,
          transport_mode: 'polling_only',
        },
      },
    });

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByText('MANUAL REFRESH REQUIRED')).toBeInTheDocument();
  });

  it('keeps the legacy trade_update listener when the standard transport flag is off locally', async () => {
    realtimeFlags.trades = false;

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    expect(socketHandlers.trade_update).toBeTypeOf('function');
    expect(socketMock.emit).not.toHaveBeenCalledWith(
      'subscribe',
      expect.anything(),
      expect.any(Function),
    );
  });

  it('does not replay over HTTP while the standard cursor is healthy, then replays with that cursor after recovery starts', async () => {
    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    getTradesDelta.mockClear();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    expect(getTradesDelta).not.toHaveBeenCalled();

    act(() => {
      socketHandlers.disconnect?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );
  });

  it('enters recovery and replays from the last acknowledged seq when a socket seq gap is detected', async () => {
    getTradesDelta.mockResolvedValueOnce({
      rows: [
        makeTradeRow({
          row_id: 'gap-3',
          seq: 3,
          ts: 3,
          time: '2025-01-01T00:00:03Z',
          side: 'sell',
        }),
        makeTradeRow({
          row_id: 'gap-4',
          seq: 4,
          ts: 4,
          time: '2025-01-01T00:00:04Z',
        }),
        makeTradeRow({
          row_id: 'gap-5',
          seq: 5,
          ts: 5,
          time: '2025-01-01T00:00:05Z',
        }),
      ],
      last_seq: 5,
      reset_required: false,
      stream_id: 'trades-main',
      snapshot_revision: 17,
    });

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    const propsAfterGap = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(propsAfterGap.trades.map((row: any) => row.row_id)).not.toContain('gap-5');

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

  });

  it('keeps replaying from the same cursor after a seq gap until the delta response makes forward progress', async () => {
    getTradesDelta
      .mockResolvedValueOnce({
        rows: [],
        last_seq: 2,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      })
      .mockResolvedValueOnce({
        rows: [
          makeTradeRow({
            row_id: 'gap-3',
            seq: 3,
            ts: 3,
            time: '2025-01-01T00:00:03Z',
            side: 'sell',
          }),
          makeTradeRow({
            row_id: 'gap-4',
            seq: 4,
            ts: 4,
            time: '2025-01-01T00:00:04Z',
          }),
          makeTradeRow({
            row_id: 'gap-5',
            seq: 5,
            ts: 5,
            time: '2025-01-01T00:00:05Z',
          }),
        ],
        last_seq: 5,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    const propsAfterGap = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(propsAfterGap.trades.map((row: any) => row.row_id)).not.toContain('gap-5');

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(2));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );
  });

  it('keeps recovery anchored to the last acknowledged seq when in-window socket events arrive during replay', async () => {
    getTradesDelta
      .mockResolvedValueOnce({
        rows: [
          makeTradeRow({
            row_id: 'gap-4',
            seq: 4,
            ts: 4,
            time: '2025-01-01T00:00:04Z',
            side: 'sell',
          }),
          makeTradeRow({
            row_id: 'gap-5',
            seq: 5,
            ts: 5,
            time: '2025-01-01T00:00:05Z',
          }),
        ],
        last_seq: 5,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      })
      .mockResolvedValueOnce({
        rows: [],
        last_seq: 5,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();
    await waitFor(() => expect(screen.getByText('RECOVERING')).toBeTruthy());

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-3',
        seq: 3,
        version: 1,
        ts: 3,
        time: '2025-01-01T00:00:03Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(screen.getByText('RECOVERING')).toBeTruthy();
    expect(screen.queryByText('gap-3')).toBeNull();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(screen.getByText('RECOVERING')).toBeTruthy();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(2));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );
  });

  it('defers legacy in-window socket updates during recovery even without epoch metadata', async () => {
    getTradesDelta
      .mockResolvedValueOnce({
        rows: [
          makeTradeRow({
            row_id: 'gap-4',
            seq: 4,
            ts: 4,
            time: '2025-01-01T00:00:04Z',
            side: 'sell',
          }),
          makeTradeRow({
            row_id: 'gap-5',
            seq: 5,
            ts: 5,
            time: '2025-01-01T00:00:05Z',
          }),
        ],
        last_seq: 5,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      })
      .mockResolvedValueOnce({
        rows: [],
        last_seq: 5,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();
    await waitFor(() => expect(screen.getByText('RECOVERING')).toBeTruthy());

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-3',
        seq: 3,
        version: 1,
        ts: 3,
        time: '2025-01-01T00:00:03Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
      });
    });

    await flushTradeSocketFrame();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(screen.getByText('RECOVERING')).toBeTruthy();
    expect(screen.queryByText('gap-3')).toBeNull();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(screen.getByText('RECOVERING')).toBeTruthy();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(2));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );
  });

  it('does not clear a newer gap target when an older delta response resolves late', async () => {
    const firstDelta = createDeferred<{
      rows: Array<Record<string, unknown>>;
      last_seq: number;
      reset_required: boolean;
      stream_id: string;
      snapshot_revision: number;
    }>();

    getTradesDelta
      .mockImplementationOnce(() => firstDelta.promise)
      .mockResolvedValueOnce({
        rows: [
          makeTradeRow({
            row_id: 'gap-6',
            seq: 6,
            ts: 6,
            time: '2025-01-01T00:00:06Z',
            side: 'sell',
          }),
          makeTradeRow({
            row_id: 'gap-7',
            seq: 7,
            ts: 7,
            time: '2025-01-01T00:00:07Z',
          }),
        ],
        last_seq: 7,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });

    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-7',
        seq: 7,
        version: 1,
        ts: 7,
        time: '2025-01-01T00:00:07Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();
    await waitFor(() => expect(screen.getByText('RECOVERING')).toBeTruthy());

    await act(async () => {
      firstDelta.resolve({
        rows: [],
        last_seq: 5,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(screen.getByText('RECOVERING')).toBeTruthy();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-6',
        seq: 6,
        version: 1,
        ts: 6,
        time: '2025-01-01T00:00:06Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(screen.getByText('RECOVERING')).toBeTruthy();
    expect(screen.queryByText('gap-6')).toBeNull();
    expect(screen.queryByText('gap-7')).toBeNull();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(2));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );
  });

  it('does not let an older delta response overwrite a newer in-order socket cursor', async () => {
    const firstDelta = createDeferred<{
      rows: Array<Record<string, unknown>>;
      last_seq: number;
      reset_required: boolean;
      stream_id: string;
      snapshot_revision: number;
    }>();

    getTradesDelta
      .mockImplementationOnce(() => firstDelta.promise)
      .mockResolvedValueOnce({
        rows: [],
        last_seq: 3,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    act(() => {
      socketHandlers.disconnect?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'live-3',
        seq: 3,
        version: 1,
        ts: 3,
        time: '2025-01-01T00:00:03Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();

    await act(async () => {
      firstDelta.resolve({
        rows: [],
        last_seq: 2,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'gap-5',
        seq: 5,
        version: 1,
        ts: 5,
        time: '2025-01-01T00:00:05Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
    });

    await flushTradeSocketFrame();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta.mock.calls.length).toBeGreaterThanOrEqual(2));
    for (const [cursor, limit] of getTradesDelta.mock.calls.slice(1)) {
      expect(cursor).toEqual(expect.objectContaining({
        sinceSeq: 3,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }));
      expect(limit).toBe(500);
    }
  });

  it('does not let an older delta response restore a stale snapshot epoch after refresh', async () => {
    const firstDelta = createDeferred<{
      rows: Array<Record<string, unknown>>;
      last_seq: number;
      reset_required: boolean;
      stream_id: string;
      snapshot_revision: number;
    }>();

    getTrades.mockReset();
    getTrades
      .mockResolvedValueOnce({
        rows: baseRows,
        total: baseRows.length,
        page: 1,
        page_size: 100,
        last_seq: 2,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      })
      .mockResolvedValueOnce({
        rows: [
          makeTradeRow({
            row_id: 'epoch-10',
            seq: 10,
            ts: 10,
            time: '2025-01-01T00:00:10Z',
          }),
        ],
        total: 1,
        page: 1,
        page_size: 100,
        last_seq: 10,
        stream_id: 'trades-main',
        snapshot_revision: 18,
      })
      .mockResolvedValue({
        rows: [
          makeTradeRow({
            row_id: 'epoch-10',
            seq: 10,
            ts: 10,
            time: '2025-01-01T00:00:10Z',
          }),
        ],
        total: 1,
        page: 1,
        page_size: 100,
        last_seq: 10,
        stream_id: 'trades-main',
        snapshot_revision: 18,
      });
    getTradesDelta
      .mockImplementationOnce(() => firstDelta.promise)
      .mockResolvedValueOnce({
        rows: [],
        last_seq: 10,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 18,
      });

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    act(() => {
      socketHandlers.disconnect?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        sinceSeq: 2,
        streamId: 'trades-main',
        snapshotRevision: 17,
      }),
      500,
    );

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'epoch-10',
        seq: 10,
        version: 1,
        ts: 10,
        time: '2025-01-01T00:00:10Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'trades-main',
        snapshot_revision: 18,
      });
    });

    await flushTradeSocketFrame();
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(2));

    await act(async () => {
      firstDelta.resolve({
        rows: [],
        last_seq: 2,
        reset_required: false,
        stream_id: 'trades-main',
        snapshot_revision: 17,
      });
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(2));
    expect(getTradesDelta).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        sinceSeq: 10,
        streamId: 'trades-main',
        snapshotRevision: 18,
      }),
      500,
    );
  });

  it('keeps the rendered trades array stable for in-place live updates', async () => {
    render(<Trades />);
    await waitFor(() => {
      const latestProps = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
      expect(latestProps?.trades).toHaveLength(2);
    });

    const initialProps = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(initialProps?.trades).toBeTruthy();
    const initialTrades = initialProps.trades;
    const initialOldRow = initialTrades.find((row: any) => row.row_id === 'old');

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'old',
        seq: 1,
        version: 2,
        ts: 1,
        time: '2025-01-01T00:00:01Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        price: 999,
      });
    });

    await flushTradeSocketFrame();

    await waitFor(() => {
      const latestProps = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
      const updatedOldRow = latestProps.trades.find((row: any) => row.row_id === 'old');

      expect(updatedOldRow).toBe(initialOldRow);
      expect(updatedOldRow.price).toBe(999);
    });
  });

  it('keeps zero-seq snapshots on the standard sinceSeq cursor and only replays after recovery begins', async () => {
    getTrades.mockResolvedValueOnce({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    });
    getTradesDelta.mockResolvedValueOnce({
      rows: [],
      last_seq: 0,
      reset_required: false,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    });
    (window.location as any).pathname = '/tokenmm/trades';

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    getTradesDelta.mockClear();

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    expect(getTradesDelta).not.toHaveBeenCalled();

    act(() => {
      socketHandlers.disconnect?.('transport close');
    });

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    const [cursor, limit] = getTradesDelta.mock.calls[0];
    expect(cursor).toMatchObject({
      sinceSeq: 0,
      streamId: 'tokenmm-trades',
      snapshotRevision: 'snap-empty',
    });
    expect(cursor.afterMs).toBeUndefined();
    expect(limit).toBe(500);
  });

  it('treats zero-baseline same-epoch seq jumps as gaps and replays from sinceSeq 0', async () => {
    getTrades.mockResolvedValueOnce({
      rows: [],
      total: 0,
      page: 1,
      page_size: 100,
      last_seq: 0,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    });
    getTradesDelta.mockResolvedValueOnce({
      rows: [
        makeTradeRow({
          row_id: 'gap-1',
          seq: 1,
          ts: 1,
          time: '2025-01-01T00:00:01Z',
        }),
        makeTradeRow({
          row_id: 'gap-2',
          seq: 2,
          ts: 2,
          time: '2025-01-01T00:00:02Z',
          side: 'sell',
        }),
        makeTradeRow({
          row_id: 'gap-3',
          seq: 3,
          ts: 3,
          time: '2025-01-01T00:00:03Z',
        }),
      ],
      last_seq: 3,
      reset_required: false,
      stream_id: 'tokenmm-trades',
      snapshot_revision: 'snap-empty',
    });
    (window.location as any).pathname = '/tokenmm/trades';

    render(<Trades />);
    await waitFor(() => expect(getTrades).toHaveBeenCalledTimes(1));

    act(() => {
      socketHandlers.trade_update?.({
        op: 'upsert',
        row_id: 'gap-3',
        seq: 3,
        version: 1,
        ts: 3,
        time: '2025-01-01T00:00:03Z',
        coin: 'PLUME/USDT',
        exchange: 'bybit',
        side: 'buy',
        stream_id: 'tokenmm-trades',
        snapshot_revision: 'snap-empty',
      });
    });

    const propsAfterGap = mockTable.mock.calls[mockTable.mock.calls.length - 1]?.[0];
    expect(propsAfterGap.trades.map((row: any) => row.row_id)).not.toContain('gap-3');

    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1_200));
    });

    await waitFor(() => expect(getTradesDelta).toHaveBeenCalledTimes(1));
    expect(getTradesDelta).toHaveBeenCalledWith(
      expect.objectContaining({
        sinceSeq: 0,
        streamId: 'tokenmm-trades',
        snapshotRevision: 'snap-empty',
      }),
      500,
    );
  });

  it('normalizes nested FluxAPI trade_update payloads (trade object) into full blotter rows', async () => {
    render(<Trades />);
    await waitFor(() => expect(mockTable).toHaveBeenCalled());

    const handler = socketHandlers['trade_update'];
    expect(handler).toBeDefined();

    act(() => {
      handler?.({
        op: 'upsert',
        row_id: 'live-nested',
        seq: 99, // socket seq (not trade seq)
        version: 1,
        strategy_id: 'bybit_binance_plumeusdt_makerv3',
        server_ts_ms: 1772700209799,
        trade: {
          row_id: 'live-nested',
          version: 1,
          seq: 1772700209804, // trade stream seq
          ts_ms: 1772700209799,
          instrument_id: 'PLUMEUSDT-LINEAR.BYBIT',
          side: '1',
          price: '0.009974',
          qty: '1000',
          client_order_id: 'O-20260305-084321-001-000-932',
          trade_id: 'live-nested',
          strategy_id: 'bybit_binance_plumeusdt_makerv3',
        },
      });
    });

    await flushTradeSocketFrame();

    await waitFor(() => {
      const top = useTradesStore.getState().rows[0];
      expect(top.row_id).toBe('live-nested');
      expect(top.coin).toBe('PLUME');
      expect(top.exchange).toBe('bybit');
      expect(top.side).toBe('buy');
      expect(top.order_id).toBe('O-20260305-084321-001-000-932');
      expect(top.time).toMatch(/T/);
      expect(top.mv).toBeCloseTo(9.974, 6);
    });
  });
});
