import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import SignalTable from '@/components/domain/signal/SignalTable';
import { api } from '@/api';
import { useSignalStore } from '@/stores';

const socketHandlers = new Map<string, Set<(payload?: any) => void>>();

function emitSocket(event: string, payload?: any) {
  const handlers = socketHandlers.get(event);
  if (!handlers) return;
  for (const handler of handlers) {
    handler(payload);
  }
}

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<any>('react-router-dom');
  return {
    ...actual,
    useLocation: () => ({ pathname: '/tokenmm/signal' }),
  };
});

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(),
  },
}));

vi.mock('@/sockets', () => ({
  socket: {
    connected: false,
    on: vi.fn((event: string, handler: (payload?: any) => void) => {
      const bucket = socketHandlers.get(event) ?? new Set();
      bucket.add(handler);
      socketHandlers.set(event, bucket);
    }),
    off: vi.fn((event: string, handler: (payload?: any) => void) => {
      const bucket = socketHandlers.get(event);
      bucket?.delete(handler);
    }),
  },
}));

describe('SignalTable sticky empty snapshot behavior', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    socketHandlers.clear();
    useSignalStore.setState({ rows: [], lastUpdate: undefined });
  });

  it('keeps existing tokenmm rows during a transient empty poll snapshot', async () => {
    (api.getSignalStrategies as any)
      .mockResolvedValueOnce({
        strategies: [],
        server_time: '2026-03-01 12:00:00',
      })
      .mockResolvedValue({
        strategies: [],
        server_time: '2026-03-01 12:00:02',
      });

    render(<SignalTable />);

    await waitFor(() => {
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);
    });

    const marketStrategy = {
      id: 'tokenmm_strategy_1',
      strategy_family: 'maker_v3',
      params: { bot_on: '1' },
      legs: {
        A: {
          exchange: 'bybit',
          coin: 'PLUME/USDT',
          update_time: '2026-03-01 12:00:01',
        },
        B: {
          exchange: 'rooster',
          coin: 'WPLUME/USDC',
          update_time: '2026-03-01 12:00:01',
        },
      },
      balances_ok: true,
      meta: {
        class: 'maker_v3',
        strategy_groups: 'tokenmm',
        venue_prefix: 'rooster_bybit',
        chain: 'plume',
      },
    };

    act(() => {
      emitSocket('market_update', {
        strategies: [marketStrategy],
        server_time: '2026-03-01 12:00:01',
      });
    });

    await waitFor(() => {
      expect(useSignalStore.getState().rows).toHaveLength(1);
    });

    const refreshButton = screen.getByRole('button', { name: /refresh/i });
    await act(async () => {
      fireEvent.click(refreshButton);
    });

    await waitFor(() => {
      expect((api.getSignalStrategies as any).mock.calls.length).toBe(2);
    });
    expect(useSignalStore.getState().rows).toHaveLength(1);
    expect(useSignalStore.getState().rows[0]?.id).toBe('tokenmm_strategy_1');
  }, 15000);

  it('filters out ungrouped maker_v3 rows on tokenmm websocket updates', async () => {
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies: [],
      server_time: '2026-03-01 12:00:00',
    });

    render(<SignalTable />);

    await waitFor(() => {
      expect(api.getSignalStrategies).toHaveBeenCalledTimes(1);
    });

    const tokenmmStrategy = {
      id: 'tokenmm_strategy_1',
      strategy_family: 'maker_v3',
      params: { bot_on: '1' },
      legs: {},
      balances_ok: true,
      meta: {
        class: 'maker_v3',
        strategy_groups: 'tokenmm',
      },
    };
    const ungroupedMakerV3 = {
      id: 'maker_v3_ungrouped',
      strategy_family: 'maker_v3',
      params: { bot_on: '1' },
      legs: {},
      balances_ok: true,
      meta: {
        class: 'maker_v3',
      },
    };

    act(() => {
      emitSocket('market_update', {
        strategies: [tokenmmStrategy, ungroupedMakerV3],
        server_time: '2026-03-01 12:00:01',
      });
    });

    await waitFor(() => {
      expect(useSignalStore.getState().rows).toHaveLength(1);
    });
    expect(useSignalStore.getState().rows[0]?.id).toBe('tokenmm_strategy_1');
  });
});
