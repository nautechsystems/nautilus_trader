import { act, render, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import Alerts from './Alerts';
import type { Alert } from './types';
import { useAlertsStore } from './stores';

const mockGetAlerts = vi.hoisted(() => vi.fn());
const mockUsePolling = vi.hoisted(() => vi.fn());
const mockUseWebSocket = vi.hoisted(() => vi.fn());

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return {
    ...actual,
    api: {
      ...actual.api,
      getAlerts: mockGetAlerts,
      clearAlerts: vi.fn(),
    },
  };
});

vi.mock('./hooks/index', () => ({
  usePolling: (...args: any[]) => mockUsePolling(...args),
  useWebSocket: (...args: any[]) => mockUseWebSocket(...args),
}));

type AlertsStoreState = {
  rows: Alert[];
  loading: boolean;
  auto: boolean;
  dismissedIds: Set<string>;
  setRows: ReturnType<typeof vi.fn>;
  setLoading: ReturnType<typeof vi.fn>;
  setAuto: ReturnType<typeof vi.fn>;
  dismissAlert: ReturnType<typeof vi.fn>;
  clearAlerts: ReturnType<typeof vi.fn>;
};

let storeState: AlertsStoreState;
let wsHandler: ((payload: unknown) => void) | null;

vi.mock('./stores', () => ({
  useAlertsStore: vi.fn((selector?: any) => (typeof selector === 'function' ? selector(storeState) : storeState)),
}));

function createAlert(overrides: Partial<Alert> = {}): Alert {
  return {
    id: 'alert-1',
    level: 'INFO',
    message: 'default message',
    details: {},
    timestamp: 1700000000,
    ...overrides,
  };
}

describe('Alerts wiring', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetAlerts.mockReset();
    mockUsePolling.mockReset();
    mockUseWebSocket.mockReset();
    wsHandler = null;

    storeState = {
      rows: [],
      loading: false,
      auto: true,
      dismissedIds: new Set<string>(),
      setRows: vi.fn((rows: Alert[]) => {
        storeState.rows = rows;
      }),
      setLoading: vi.fn(),
      setAuto: vi.fn(),
      dismissAlert: vi.fn(),
      clearAlerts: vi.fn(),
    };

    (useAlertsStore as any).mockImplementation((selector?: any) =>
      typeof selector === 'function' ? selector(storeState) : storeState,
    );

    mockUsePolling.mockImplementation(() => {
      // Keep polling side effects disabled in these websocket wiring tests.
    });

    mockUseWebSocket.mockImplementation((_event: string, handler: (payload: unknown) => void) => {
      wsHandler = handler;
    });

    mockGetAlerts.mockResolvedValue([]);
  });

  it('uses row_id as fallback identity when websocket alert id is missing', () => {
    render(<Alerts />);

    act(() => {
      wsHandler?.({
        alerts: [
          {
            row_id: 'row-123',
            level: 'WARNING',
            message: 'missing id in payload',
            details: { source: 'redis' },
            timestamp: 1700000001,
          },
        ],
      });
    });

    expect(storeState.setRows).toHaveBeenCalledTimes(1);
    const [[rows]] = storeState.setRows.mock.calls as [Alert[]][];
    expect(rows[0]).toMatchObject({
      id: 'row-123',
      row_id: 'row-123',
      message: 'missing id in payload',
    });
  });

  it('applies an empty websocket snapshot to clear stale alert rows', () => {
    storeState.rows = [createAlert({ id: 'stale-alert' })];

    render(<Alerts />);

    act(() => {
      wsHandler?.({ alerts: [] });
    });

    expect(storeState.setRows).toHaveBeenCalledWith([]);
  });

  it('ignores legacy id-only websocket snapshots', () => {
    storeState.rows = [createAlert({ id: 'keep-alert' })];

    render(<Alerts />);

    act(() => {
      wsHandler?.({ alerts: ['legacy-id-a', 'legacy-id-b'] });
    });

    expect(storeState.setRows).not.toHaveBeenCalled();
  });

  it('refreshes alerts from REST when market_update ships summary metadata only', async () => {
    storeState.auto = false;
    mockGetAlerts.mockResolvedValueOnce([
      createAlert({
        id: 'fresh-alert',
        level: 'CRITICAL',
        message: 'socket summary triggered reload',
        timestamp: 1700000002,
      }),
    ]);

    render(<Alerts />);

    act(() => {
      wsHandler?.({
        alerts: {
          count: 1,
          latest_ts_ms: 1_700_000_002_000,
        },
      });
    });

    await waitFor(() => expect(mockGetAlerts).toHaveBeenCalledTimes(1));
    expect(storeState.setRows).toHaveBeenCalledWith([
      expect.objectContaining({
        id: 'fresh-alert',
        message: 'socket summary triggered reload',
      }),
    ]);
  });

  it('keeps existing alerts when the REST refresh fails transiently', async () => {
    storeState.rows = [createAlert({ id: 'existing-alert', message: 'keep me' })];
    mockGetAlerts.mockRejectedValueOnce(new Error('temporary failure'));
    mockUsePolling.mockImplementation((fn: () => Promise<void>) => {
      void fn();
    });

    render(<Alerts />);

    await waitFor(() => expect(mockGetAlerts).toHaveBeenCalledTimes(1));
    expect(storeState.setRows).not.toHaveBeenCalled();
  });

  it('retries the same summary metadata after a transient REST failure', async () => {
    storeState.auto = false;
    mockGetAlerts
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce([
        createAlert({
          id: 'recovered-alert',
          level: 'WARNING',
          message: 'retried summary reload',
          timestamp: 1700000003,
        }),
      ]);

    render(<Alerts />);

    act(() => {
      wsHandler?.({
        alerts: {
          count: 1,
          latest_ts_ms: 1_700_000_003_000,
        },
      });
    });

    await waitFor(() => expect(mockGetAlerts).toHaveBeenCalledTimes(1));

    act(() => {
      wsHandler?.({
        alerts: {
          count: 1,
          latest_ts_ms: 1_700_000_003_000,
        },
      });
    });

    await waitFor(() => expect(mockGetAlerts).toHaveBeenCalledTimes(2));
    expect(storeState.setRows).toHaveBeenCalledWith([
      expect.objectContaining({
        id: 'recovered-alert',
        message: 'retried summary reload',
      }),
    ]);
  });
});
