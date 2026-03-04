/**
 * Regression test for hook order stability in SignalTable/LegCell.
 *
 * Before the fix, LegCell returned early when a leg was null, skipping
 * a useMemo hook. When a WebSocket delta later populated the leg, React
 * detected a changed hook count and threw error #310 (rendered fewer hooks
 * than expected). This test simulates that null→object transition to ensure
 * the component renders without a hooks invariant violation.
 */

import { render, screen, act } from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import SignalTable from './SignalTable';
import { useSignalStore } from '../../../stores';
import * as apiModule from '../../../api';
import { socket as mockSocket } from '../../../sockets';

vi.mock('../../../api', () => ({
  api: {
    getSignalStrategies: vi.fn(),
  },
}));

vi.mock('../../../sockets', () => {
  const handlers: Record<string, Array<(payload: any) => void>> = {};
  const socket = {
    connected: false,
    on: vi.fn((event: string, handler: (payload: any) => void) => {
      if (!handlers[event]) handlers[event] = [];
      handlers[event].push(handler);
    }),
    off: vi.fn((event: string, handler: (payload: any) => void) => {
      if (!handlers[event]) return;
      handlers[event] = handlers[event].filter((h) => h !== handler);
    }),
    emit: (event: string, payload?: any) => {
      (handlers[event] || []).forEach((h) => h(payload));
    },
  };
  (socket as any).__handlers = handlers;
  return { socket };
});

describe('SignalTable hook order', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset store to a clean state between tests
    useSignalStore.setState({ rows: [], lastUpdate: undefined });
    // Clear socket handlers
    const handlers = (mockSocket as any).__handlers as Record<string, Array<(payload: any) => void>> | undefined;
    if (handlers) {
      Object.keys(handlers).forEach((key) => delete handlers[key]);
    }
    mockSocket.connected = false;
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('handles leg going from null to object without hook invariant error', async () => {
    const initialStrategy = {
      id: 'toggle_hooks',
      params: { bot_on: '1' } as any,
      legs: {
        A: {
          coin: 'PLUME',
          exchange: 'bybit',
          fv_bid: 1.1,
          fv_ask: 1.2,
          update_time: '2025-12-05 12:00:00',
        },
        B: null,
      },
      balances_ok: true,
    } as any;

    // Initial API snapshot returns leg B as null (no pricing yet)
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [initialStrategy],
      server_time: '2025-12-05 12:00:01',
      server_ts_ms: 1733409601000,
    });

    const renderTable = () => render(<SignalTable />);

    // Render and wait for initial strategy to appear
    await act(async () => {
      renderTable();
    });

    await screen.findByText('toggle_hooks');

    // Simulate WebSocket delta that populates leg B
    const delta = {
      id: 'toggle_hooks',
      legs: {
        B: {
          coin: 'SEI',
          exchange: 'sailor',
          fv_bid: 0.91,
          fv_ask: 0.93,
          update_time: '2025-12-05 12:00:02',
        },
      },
    } as any;

    await act(async () => {
      mockSocket.emit('signal_delta', delta);
    });

    // Should render leg B content without throwing a hooks invariant error
    expect(await screen.findByText(/sailor SEI/i)).toBeInTheDocument();
  });
});
