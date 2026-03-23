import { useEffect } from 'react';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import Alerts from './Alerts';
import type { Alert } from './types';
import { INTERVALS } from './constants';
import { useAlertsStore } from './stores';
import * as apiModule from './api';

const mockIsRealtimeStandardEnabled = vi.hoisted(() => vi.fn(() => false));
const mockUsePolling = vi.hoisted(() => vi.fn());
const mockUseWebSocket = vi.hoisted(() => vi.fn());
const alertsStoreRuntime = vi.hoisted(() => ({ current: null as MockAlertsStore | null }));

vi.mock('./config/featureFlags', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./config/featureFlags')>();
  return {
    ...actual,
    isRealtimeStandardEnabled: (...args: unknown[]) => mockIsRealtimeStandardEnabled(...args),
  };
});

vi.mock('./api', () => ({
  api: {
    getAlerts: vi.fn(),
    clearAlerts: vi.fn(),
  },
}));

type MockAlertsStore = {
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

function createMockStore(): MockAlertsStore {
  return {
    rows: [],
    loading: false,
    auto: true,
    dismissedIds: new Set<string>(),
    setRows: vi.fn((rows: Alert[]) => {
      mockStoreState.rows = rows;
    }),
    setLoading: vi.fn((loading: boolean) => {
      mockStoreState.loading = loading;
    }),
    setAuto: vi.fn((auto: boolean) => {
      mockStoreState.auto = auto;
    }),
    dismissAlert: vi.fn(),
    clearAlerts: vi.fn(() => {
      mockStoreState.rows = [];
    }),
  };
}

let mockStoreState: MockAlertsStore;

vi.mock('./stores', () => {
  const mockedHook = vi.fn((selector?: (state: MockAlertsStore) => unknown) => (
    typeof selector === 'function'
      ? selector(alertsStoreRuntime.current as MockAlertsStore)
      : alertsStoreRuntime.current
  ));
  return { useAlertsStore: mockedHook };
});

vi.mock('./hooks/index', () => ({
  usePolling: (...args: unknown[]) => mockUsePolling(...args),
  useWebSocket: (...args: unknown[]) => mockUseWebSocket(...args),
}));

function createAlert(overrides: Partial<Alert> = {}): Alert {
  return {
    id: 'alert-1',
    level: 'WARNING',
    severity: 'WARNING',
    title: 'Spread drift',
    message: 'Spread drift',
    details: {},
    timestamp: 1_700_000_000,
    ts: 1_700_000_000,
    ...overrides,
  };
}

describe('Alerts', () => {
  let marketUpdateHandler: ((payload: unknown) => void) | null;

  beforeEach(() => {
    vi.clearAllMocks();
    mockStoreState = createMockStore();
    alertsStoreRuntime.current = mockStoreState;
    marketUpdateHandler = null;

    (useAlertsStore as unknown as ReturnType<typeof vi.fn>).mockImplementation(
      (selector?: (state: MockAlertsStore) => unknown) => (
        typeof selector === 'function' ? selector(mockStoreState) : mockStoreState
      ),
    );

    mockIsRealtimeStandardEnabled.mockReturnValue(false);
    mockUsePolling.mockImplementation((fn: () => void | Promise<unknown>, _interval: number, enabled = true) => {
      useEffect(() => {
        if (enabled) {
          void fn();
        }
      }, [fn, enabled]);
    });
    mockUseWebSocket.mockImplementation((event: string, handler: (payload: unknown) => void) => {
      if (event === 'market_update') {
        marketUpdateHandler = handler;
      }
    });

    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>).mockResolvedValue([]);
    (apiModule.api.clearAlerts as ReturnType<typeof vi.fn>).mockResolvedValue({ success: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('loads alerts on mount and keeps legacy polling enabled when realtime standard is off', async () => {
    render(<Alerts />);

    await waitFor(() => {
      expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(1);
    });
    expect(mockUsePolling).toHaveBeenCalledWith(expect.any(Function), INTERVALS.ALERTS_POLL, true);
  });

  it('routes market updates through the alerts surface-aware websocket options', () => {
    render(<Alerts />);

    expect(mockUseWebSocket).toHaveBeenCalledWith(
      'market_update',
      expect.any(Function),
      expect.objectContaining({ surface: 'alerts' }),
    );
  });

  it('shows explicit recovering state while a standard summary refresh is in flight', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);

    let resolveRecovery!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce([createAlert()])
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveRecovery = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
    });

    act(() => {
      marketUpdateHandler?.({
        alerts: {
          count: 2,
          latest_ts_ms: 1_700_000_050_000,
        },
      });
    });

    expect(screen.getByText('RECOVERING')).toBeInTheDocument();

    await act(async () => {
      resolveRecovery([createAlert({ id: 'alert-2', title: 'Recovered alert', message: 'Recovered alert' })]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
    });
    expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(2);
  });

  it('keeps polling disabled while the realtime standard surface is healthy', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>).mockResolvedValueOnce([createAlert()]);

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
    });

    expect(mockUsePolling).toHaveBeenLastCalledWith(
      expect.any(Function),
      INTERVALS.ALERTS_POLL,
      false,
    );
  });

  it('clears all alerts after confirmation', async () => {
    mockStoreState.rows = [createAlert({ title: 'Critical path blocked', message: 'Critical path blocked' })];

    render(<Alerts />);

    fireEvent.click(screen.getByText('Clear All'));
    fireEvent.click(screen.getByRole('button', { name: 'Clear All' }));

    await waitFor(() => {
      expect(apiModule.api.clearAlerts).toHaveBeenCalledTimes(1);
      expect(mockStoreState.clearAlerts).toHaveBeenCalledTimes(1);
    });
  });

  it('renders summary text for contextual alerts', () => {
    mockStoreState.rows = [
      createAlert({
        title: 'Order rejected',
        message: 'Order rejected',
        details: { summary: 'Bybit rejected order due to insufficient balance' },
      }),
    ];

    render(<Alerts />);

    expect(screen.getByText('Bybit rejected order due to insufficient balance')).toBeInTheDocument();
  });
});
