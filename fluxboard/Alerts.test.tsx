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
const mockUseStandardWebSocketSubscription = vi.hoisted(() => vi.fn());
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
  useStandardWebSocketSubscription: (...args: unknown[]) => mockUseStandardWebSocketSubscription(...args),
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

function createStandardAlertsSnapshot(
  rows: Alert[] = [createAlert()],
  overrides: Partial<Record<string, unknown>> = {},
): Alert[] & { realtime: Record<string, unknown> } {
  return Object.assign(rows, {
    realtime: {
      contract_version: 2,
      surface: 'alerts',
      profile: 'default',
      surface_query_key: 'alerts|profile=default',
      stream_id: 'alerts-main',
      snapshot_revision: 'alerts-snap-1',
      last_seq: 4,
      capabilities: {
        recovery_mode: 'invalidate_only',
        replay_supported: false,
        transport_mode: 'polling_only',
      },
      ...overrides,
    },
  });
}

describe('Alerts', () => {
  let marketUpdateHandler: ((payload: unknown) => void) | null;
  let standardSubscriptionOptions: Record<string, any> | null;

  beforeEach(() => {
    vi.clearAllMocks();
    window.history.replaceState({}, '', '/alerts');
    mockStoreState = createMockStore();
    alertsStoreRuntime.current = mockStoreState;
    marketUpdateHandler = null;
    standardSubscriptionOptions = null;

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
    mockUseStandardWebSocketSubscription.mockImplementation((options: Record<string, any>) => {
      standardSubscriptionOptions = options;
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

  it('subscribes alerts through the standard realtime client when the standard flag is on', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    const snapshot = createStandardAlertsSnapshot();
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>).mockResolvedValueOnce(snapshot);

    render(<Alerts />);

    await waitFor(() => {
      expect(mockUseStandardWebSocketSubscription).toHaveBeenCalledWith(
        expect.objectContaining({
          enabled: true,
          lineage: expect.objectContaining({
            contract_version: 2,
            surface: 'alerts',
            stream_id: 'alerts-main',
            snapshot_revision: 'alerts-snap-1',
            last_seq: 4,
          }),
        }),
      );
    });

    expect(mockUseWebSocket).not.toHaveBeenCalledWith(
      'market_update',
      expect.any(Function),
      expect.anything(),
    );
  });

  it('shows explicit recovering state while a standard summary refresh is in flight', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    const initialSnapshot = createStandardAlertsSnapshot();

    let resolveRecovery!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(initialSnapshot)
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveRecovery = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
    });

    await waitFor(() => {
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
    });

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 5,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_050_000,
        payload: {
          alerts: {
            count: 2,
            latest_ts_ms: 1_700_000_050_000,
          },
        },
      });
    });

    expect(screen.getByText('RECOVERING')).toBeInTheDocument();

    await act(async () => {
      resolveRecovery(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-2', title: 'Recovered alert', message: 'Recovered alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
    });
    expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(2);
  });

  it('recovers through the standard failure callback when recovery_required is raised', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    const initialSnapshot = createStandardAlertsSnapshot();

    let resolveRecovery!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(initialSnapshot)
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveRecovery = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onFailure).toBeTypeOf('function');
    });

    act(() => {
      standardSubscriptionOptions?.onFailure({
        type: 'recovery_required',
        reason: 'capability_withdrawn',
        requested: {
          contract_version: 2,
          surface: 'alerts',
          profile: 'default',
          surface_query_key: 'alerts|profile=default',
          stream_id: 'alerts-main',
          snapshot_revision: 'alerts-snap-1',
          resume_from_seq: 4,
        },
        event: {
          contract_version: 2,
          surface: 'alerts',
          profile: 'default',
          stream_id: 'alerts-main',
          kind: 'recovery_required',
          seq: 5,
          reason: 'capability_withdrawn',
          snapshot_revision: 'alerts-snap-1',
          server_ts_ms: 1_700_000_050_000,
          payload: {
            alerts: {
              count: 2,
              latest_ts_ms: 1_700_000_050_000,
            },
          },
        },
      });
    });

    await waitFor(() => {
      expect(screen.getByText('SYNCING')).toBeInTheDocument();
    });

    await act(async () => {
      resolveRecovery(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-recovery', title: 'Recovered from recovery_required', message: 'Recovered from recovery_required' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(mockStoreState.rows[0]).toMatchObject({
        id: 'alert-recovery',
        message: 'Recovered from recovery_required',
      });
    });
    expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(2);
  });

  it('stays stale after a failed standard invalidate recovery instead of bouncing back to live', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    mockStoreState.auto = false;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(createStandardAlertsSnapshot())
      .mockRejectedValueOnce(new Error('summary refresh failed'));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
    });

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 5,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_050_000,
        payload: {
          alerts: {
            count: 2,
            latest_ts_ms: 1_700_000_050_000,
          },
        },
      });
    });

    await waitFor(() => {
      expect(screen.getByText('STALE')).toBeInTheDocument();
    });

    await act(async () => {
      await new Promise((resolve) => {
        window.setTimeout(resolve, 1_100);
      });
    });

    expect(screen.getByText('STALE')).toBeInTheDocument();
  });

  it('returns to live after a manual refresh succeeds following a failed authoritative recovery', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    mockStoreState.auto = false;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-initial', title: 'Initial alert', message: 'Initial alert' }),
      ]))
      .mockRejectedValueOnce(new Error('summary refresh failed'))
      .mockResolvedValueOnce(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-manual', title: 'Manual recovery alert', message: 'Manual recovery alert' }),
      ]));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
    });

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 5,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_050_000,
        payload: {
          alerts: {
            count: 2,
            latest_ts_ms: 1_700_000_050_000,
          },
        },
      });
    });

    await waitFor(() => {
      expect(screen.getByText('STALE')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(mockStoreState.rows[0]).toMatchObject({
        id: 'alert-manual',
        message: 'Manual recovery alert',
      });
    });

    await act(async () => {
      await new Promise((resolve) => {
        window.setTimeout(resolve, 1_100);
      });
    });

    expect(screen.getByText('LIVE')).toBeInTheDocument();
  });

  it('keeps polling disabled while the realtime standard surface is healthy', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>).mockResolvedValueOnce(createStandardAlertsSnapshot());

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

  it('fails closed when the standard alerts snapshot lacks realtime lineage metadata', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>).mockResolvedValueOnce([createAlert()]);

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('STALE')).toBeInTheDocument();
    });

    expect(mockUseStandardWebSocketSubscription).toHaveBeenCalledWith(
      expect.objectContaining({
        enabled: false,
        lineage: null,
      }),
    );
    expect(mockUsePolling).toHaveBeenLastCalledWith(
      expect.any(Function),
      INTERVALS.ALERTS_POLL,
      true,
    );
  });

  it('ignores late failure-triggered refresh results after a newer invalidate recovery succeeds', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);

    let resolveFailureRefresh!: (rows: Alert[]) => void;
    let resolveInvalidateRefresh!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-initial', title: 'Initial alert', message: 'Initial alert' }),
      ]))
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveFailureRefresh = resolve;
      }))
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveInvalidateRefresh = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
      expect(standardSubscriptionOptions?.onFailure).toBeTypeOf('function');
    });

    act(() => {
      standardSubscriptionOptions?.onFailure({
        type: 'subscribe_rejected',
        reason: 'test-refresh',
        requested: {
          contract_version: 2,
          surface: 'alerts',
          profile: 'default',
          surface_query_key: 'alerts|profile=default',
          stream_id: 'alerts-main',
          snapshot_revision: 'alerts-snap-1',
          resume_from_seq: 4,
        },
      });
    });

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 5,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_050_000,
        payload: {
          alerts: {
            count: 1,
            latest_ts_ms: 1_700_000_050_000,
          },
        },
      });
    });

    await act(async () => {
      resolveInvalidateRefresh(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-new', title: 'Recovered alert', message: 'Recovered alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(mockStoreState.rows[0]).toMatchObject({
        id: 'alert-new',
        message: 'Recovered alert',
      });
    });

    await act(async () => {
      resolveFailureRefresh(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-stale', title: 'Stale alert', message: 'Stale alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    expect(mockStoreState.rows[0]).toMatchObject({
      id: 'alert-new',
      message: 'Recovered alert',
    });
  });

  it('keeps standard alerts live when an older failure-triggered refresh rejects after newer invalidate recovery succeeds', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);

    let rejectFailureRefresh!: (reason?: unknown) => void;
    let resolveInvalidateRefresh!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-initial', title: 'Initial alert', message: 'Initial alert' }),
      ]))
      .mockReturnValueOnce(new Promise<Alert[]>((_, reject) => {
        rejectFailureRefresh = reject;
      }))
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveInvalidateRefresh = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
      expect(standardSubscriptionOptions?.onFailure).toBeTypeOf('function');
    });

    act(() => {
      standardSubscriptionOptions?.onFailure({
        type: 'subscribe_rejected',
        reason: 'test-refresh',
        requested: {
          contract_version: 2,
          surface: 'alerts',
          profile: 'default',
          surface_query_key: 'alerts|profile=default',
          stream_id: 'alerts-main',
          snapshot_revision: 'alerts-snap-1',
          resume_from_seq: 4,
        },
      });
    });

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 5,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_050_000,
        payload: {
          alerts: {
            count: 1,
            latest_ts_ms: 1_700_000_050_000,
          },
        },
      });
    });

    await act(async () => {
      resolveInvalidateRefresh(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-new', title: 'Recovered alert', message: 'Recovered alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(mockStoreState.rows[0]).toMatchObject({
        id: 'alert-new',
        message: 'Recovered alert',
      });
    });

    await act(async () => {
      rejectFailureRefresh(new Error('stale refresh failed'));
      await Promise.resolve();
    });

    expect(screen.getByText('LIVE')).toBeInTheDocument();
    expect(mockStoreState.rows[0]).toMatchObject({
      id: 'alert-new',
      message: 'Recovered alert',
    });
  });

  it('ignores late manual refresh results after a newer invalidate recovery succeeds', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);

    let resolveManualRefresh!: (rows: Alert[]) => void;
    let resolveInvalidateRefresh!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-initial', title: 'Initial alert', message: 'Initial alert' }),
      ]))
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveManualRefresh = resolve;
      }))
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveInvalidateRefresh = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
    });

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 6,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_060_000,
        payload: {
          alerts: {
            count: 1,
            latest_ts_ms: 1_700_000_060_000,
          },
        },
      });
    });

    await act(async () => {
      resolveInvalidateRefresh(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-newer', title: 'Newer alert', message: 'Newer alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(mockStoreState.rows[0]).toMatchObject({
        id: 'alert-newer',
        message: 'Newer alert',
      });
    });

    await act(async () => {
      resolveManualRefresh(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-stale-manual', title: 'Stale manual alert', message: 'Stale manual alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    expect(mockStoreState.rows[0]).toMatchObject({
      id: 'alert-newer',
      message: 'Newer alert',
    });
  });

  it('keeps standard alerts live when an older manual refresh rejects after newer invalidate recovery succeeds', async () => {
    mockIsRealtimeStandardEnabled.mockReturnValue(true);

    let rejectManualRefresh!: (reason?: unknown) => void;
    let resolveInvalidateRefresh!: (rows: Alert[]) => void;
    (apiModule.api.getAlerts as ReturnType<typeof vi.fn>)
      .mockResolvedValueOnce(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-initial', title: 'Initial alert', message: 'Initial alert' }),
      ]))
      .mockReturnValueOnce(new Promise<Alert[]>((_, reject) => {
        rejectManualRefresh = reject;
      }))
      .mockReturnValueOnce(new Promise<Alert[]>((resolve) => {
        resolveInvalidateRefresh = resolve;
      }));

    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(standardSubscriptionOptions?.onEvent).toBeTypeOf('function');
    });

    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));

    act(() => {
      standardSubscriptionOptions?.onEvent({
        contract_version: 2,
        surface: 'alerts',
        profile: 'default',
        stream_id: 'alerts-main',
        kind: 'invalidate',
        seq: 6,
        snapshot_revision: 'alerts-snap-1',
        server_ts_ms: 1_700_000_060_000,
        payload: {
          alerts: {
            count: 1,
            latest_ts_ms: 1_700_000_060_000,
          },
        },
      });
    });

    await act(async () => {
      resolveInvalidateRefresh(createStandardAlertsSnapshot([
        createAlert({ id: 'alert-newer', title: 'Newer alert', message: 'Newer alert' }),
      ]) as Alert[]);
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByText('LIVE')).toBeInTheDocument();
      expect(mockStoreState.rows[0]).toMatchObject({
        id: 'alert-newer',
        message: 'Newer alert',
      });
    });

    await act(async () => {
      rejectManualRefresh(new Error('stale manual refresh failed'));
      await Promise.resolve();
    });

    expect(screen.getByText('LIVE')).toBeInTheDocument();
    expect(mockStoreState.rows[0]).toMatchObject({
      id: 'alert-newer',
      message: 'Newer alert',
    });
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

  it('hides Clear All on the tokenmm alerts surface', () => {
    window.history.replaceState({}, '', '/tokenmm/alerts');
    mockStoreState.rows = [createAlert({ title: 'Critical path blocked', message: 'Critical path blocked' })];

    expect(document.location.pathname).toBe('/tokenmm/alerts');

    render(<Alerts />);

    expect(screen.queryByRole('button', { name: 'Clear All' })).not.toBeInTheDocument();
  });

  it('renders summary text for contextual alerts', () => {
    mockStoreState.auto = false;
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
