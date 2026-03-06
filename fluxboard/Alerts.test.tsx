// Alerts component tests

import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import Alerts from './Alerts';
import { useAlertsStore } from './stores';
import * as apiModule from './api';
import { INTERVALS } from './constants';
import type { Alert } from './types';

// Mock API
vi.mock('./api', () => ({
  api: {
    getAlerts: vi.fn(),
    clearAlerts: vi.fn()
  }
}));

// Mock stores - use a factory function to create fresh mocks per test
const createMockStore = () => ({
  rows: [],
  loading: false,
  auto: true,
  dismissedIds: new Set(),
  setRows: vi.fn(),
  setLoading: vi.fn(),
  setAuto: vi.fn(),
  dismissAlert: vi.fn(),
  clearAlerts: vi.fn()
});

let mockStoreState: ReturnType<typeof createMockStore>;

vi.mock('./stores', () => {
  mockStoreState = createMockStore();
  const mockedHook = vi.fn((selector?: any) => {
    if (typeof selector === 'function') {
      return selector(mockStoreState);
    }
    return mockStoreState;
  });
  return { useAlertsStore: mockedHook };
});

// Mock socket
vi.mock('./sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn()
  }
}));

const mockUsePolling = vi.fn();
const mockUseWebSocket = vi.fn();

vi.mock('./hooks/index', () => ({
  usePolling: (fn: any, interval: number, enabled?: boolean) => mockUsePolling(fn, interval, enabled),
  useWebSocket: (...args: any[]) => mockUseWebSocket(...args)
}));

describe('Alerts Component', () => {
  beforeEach(() => {
    vi.clearAllMocks();

    // Reset mock store state
    mockStoreState = createMockStore();

    // Update the mock implementation to use current state
    (useAlertsStore as any).mockImplementation((selector?: any) => {
      if (typeof selector === 'function') {
        return selector(mockStoreState);
      }
      return mockStoreState;
    });

    mockUsePolling.mockImplementation((fn: any, _interval: number, enabled: boolean = true) => {
      if (enabled) {
        fn();
      }
    });
    mockUseWebSocket.mockImplementation(() => {});

    (apiModule.api.getAlerts as any).mockResolvedValue([]);
    (apiModule.api.clearAlerts as any).mockResolvedValue({ success: true });
  });

  afterEach(() => {
    vi.useRealTimers();
    mockUsePolling.mockReset();
    mockUseWebSocket.mockReset();
  });

  it('renders empty state when no alerts', async () => {
    render(<Alerts />);

    await waitFor(() => {
      expect(screen.getByText('No alerts')).toBeInTheDocument();
    });
  });

  it('loads alerts on mount', async () => {
    const mockAlerts: Alert[] = [
      {
        id: '1',
        level: 'CRITICAL',
        message: 'Test alert',
        details: { test: 'data' },
        timestamp: Date.now() / 1000
      }
    ];
    (apiModule.api.getAlerts as any).mockResolvedValue(mockAlerts);

    render(<Alerts />);

    await waitFor(() => {
      expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(1);
      expect(mockSetRows).toHaveBeenCalledWith(mockAlerts);
    });
  });

  it('polls alerts every 3 seconds when auto is enabled', async () => {
    (apiModule.api.getAlerts as any).mockResolvedValue([]);

    render(<Alerts />);

    expect(mockUsePolling).toHaveBeenCalledWith(expect.any(Function), INTERVALS.ALERTS_POLL, true);
    expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(1);

    const [pollFn] = mockUsePolling.mock.calls[0];
    await pollFn();
    expect(apiModule.api.getAlerts).toHaveBeenCalledTimes(2);
  });

  it('filters alerts by level', async () => {
    const mockAlerts: Alert[] = [
      { id: '1', level: 'CRITICAL', message: 'Critical alert', details: {}, timestamp: Date.now() / 1000 },
      { id: '2', level: 'WARNING', message: 'Warning alert', details: {}, timestamp: Date.now() / 1000 },
      { id: '3', level: 'INFO', message: 'Info alert', details: {}, timestamp: Date.now() / 1000 }
    ];

    mockStoreState.rows = mockAlerts;

    const { rerender } = render(<Alerts />);

    // All alerts should be visible initially
    expect(screen.getAllByText('Critical alert').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Warning alert').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Info alert').length).toBeGreaterThan(0);

    // Change filter to CRITICAL
    const filterSelect = screen.getByRole('combobox');
    fireEvent.change(filterSelect, { target: { value: 'CRITICAL' } });

    rerender(<Alerts />);

    // Only critical alert should be visible (via count)
    await waitFor(() => {
      expect(screen.getByText(/1 alert/)).toBeInTheDocument();
    });
  });

  it('clears all alerts with confirmation', async () => {
    const mockAlerts: Alert[] = [
      { id: '1', level: 'CRITICAL', message: 'Test alert', details: {}, timestamp: Date.now() / 1000 }
    ];

    mockStoreState.rows = mockAlerts;
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts
    };

    render(<Alerts />);

    // Click clear all button
    const clearButton = screen.getByText('Clear All');
    fireEvent.click(clearButton);

    // Confirm button should appear
    const confirmButton = screen.getByText('Confirm Clear');
    expect(confirmButton).toBeInTheDocument();

    // Click confirm
    fireEvent.click(confirmButton);

    await waitFor(() => {
      expect(apiModule.api.clearAlerts).toHaveBeenCalledTimes(1);
      expect(mockClearAlerts).toHaveBeenCalledTimes(1);
    });
  });

  it('expands alert details when clicked', async () => {
    const mockAlerts: Alert[] = [
      {
        id: '1',
        level: 'CRITICAL',
        message: 'Test alert',
        details: { exchange: 'bybit', symbol: 'BTC/USDT' },
        timestamp: Date.now() / 1000
      }
    ];

    mockStoreState.rows = mockAlerts;
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts
    };

    render(<Alerts />);

    // Click the row to expand (title column)
    const alertTitleCell = screen.getAllByText('Test alert')[0];
    fireEvent.click(alertTitleCell);

    // Details should be visible
    await waitFor(() => {
      expect(screen.getByText(/"exchange": "bybit"/)).toBeInTheDocument();
    });
  });

  it('renders summary text for contextual alerts', async () => {
    const mockAlerts: Alert[] = [
      {
        id: '1',
        level: 'WARNING',
        message: 'Order rejected',
        details: { summary: 'Bybit rejected order due to insufficient balance' },
        timestamp: Date.now() / 1000,
      } as Alert
    ];

    mockStoreState.rows = mockAlerts;
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts
    };

    render(<Alerts />);

    expect(screen.getByText('Bybit rejected order due to insufficient balance')).toBeInTheDocument();
  });

  it('renders alert level badges correctly', async () => {
    const mockAlerts: Alert[] = [
      { id: '1', level: 'CRITICAL', message: 'Critical', details: {}, timestamp: Date.now() / 1000 },
      { id: '2', level: 'WARNING', message: 'Warning', details: {}, timestamp: Date.now() / 1000 },
      { id: '3', level: 'INFO', message: 'Info', details: {}, timestamp: Date.now() / 1000 }
    ];

    mockStoreState.rows = mockAlerts;
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts
    };

    render(<Alerts />);

    expect(screen.getAllByText('CRITICAL')).toHaveLength(1);
    expect(screen.getAllByText('WARNING')).toHaveLength(1);
    expect(screen.getAllByText('INFO')).toHaveLength(1);
  });

  it('sorts by severity rank when header is clicked', async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    const mockAlerts: Alert[] = [
      { id: 'a', level: 'CRITICAL', message: 'C', details: {}, timestamp: nowSec },
      { id: 'b', level: 'INFO', message: 'I', details: {}, timestamp: nowSec },
      { id: 'c', level: 'WARNING', message: 'W', details: {}, timestamp: nowSec }
    ];

    mockStoreState.rows = mockAlerts;
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts
    };

    const { container } = render(<Alerts />);

    // Click Severity header (asc expected: INFO, WARNING, CRITICAL)
    fireEvent.click(screen.getByText('Severity'));

    const rowsAsc = Array.from(container.querySelectorAll('tbody tr'));
    const firstSeverityAsc = rowsAsc[0].querySelectorAll('td')[1]?.textContent?.trim();
    expect(firstSeverityAsc).toBe('INFO');

    // Click again for desc
    fireEvent.click(screen.getByText('Severity'));
    const rowsDesc = Array.from(container.querySelectorAll('tbody tr'));
    const firstSeverityDesc = rowsDesc[0].querySelectorAll('td')[1]?.textContent?.trim();
    expect(firstSeverityDesc).toBe('CRITICAL');
  });

  it('adds ISO timestamp tooltip on Age cell', async () => {
    const tsSec = Math.floor(Date.now() / 1000) - 10;
    const mockAlerts: Alert[] = [
      { id: 't', level: 'INFO', message: 'Tooltip', details: {}, timestamp: tsSec }
    ];

    mockStoreState.rows = mockAlerts;
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts
    };

    const { container } = render(<Alerts />);
    const ageCell = container.querySelector('tbody tr td[title]') as HTMLElement | null;
    expect(ageCell).not.toBeNull();
    const title = ageCell?.getAttribute('title') || '';
    expect(title).toMatch(/\d{4}-\d{2}-\d{2}T/);
  });

  it('uses explorer_url from context when provided', async () => {
    const mockAlerts: Alert[] = [
      {
        id: '1',
        level: 'CRITICAL',
        message: 'Swap failed',
        details: {},
        timestamp: Date.now() / 1000,
        context: { tx_hash: '0xabc', explorer_url: 'https://example.explorer/tx/0xabc' },
      } as any,
    ];

    (useAlertsStore as any).mockReturnValue({
      rows: mockAlerts,
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts,
    });

    render(<Alerts />);

    // Expand row
    const titleCell = screen.getAllByText('Swap failed')[0];
    fireEvent.click(titleCell);

    const link = await screen.findByRole('link', { name: /0x/i });
    expect(link).toHaveAttribute('href', 'https://example.explorer/tx/0xabc');
  });

  it('falls back to chain-aware explorer when only tx_hash is present', async () => {
    const mockAlerts: Alert[] = [
      {
        id: '2',
        level: 'CRITICAL',
        message: 'Swap failed',
        details: { chain: 'plume-testnet' } as any,
        timestamp: Date.now() / 1000,
        context: { tx_hash: '0xabcdef1234567890abcdef1234567890abcdef12' },
      } as any,
    ];

    (useAlertsStore as any).mockReturnValue({
      rows: mockAlerts,
      loading: false,
      auto: true,
      dismissedIds: new Set(),
      setRows: mockStoreState.setRows,
      setLoading: mockStoreState.setLoading,
      setAuto: mockStoreState.setAuto,
      dismissAlert: mockStoreState.dismissAlert,
      clearAlerts: mockStoreState.clearAlerts,
    });

    render(<Alerts />);

    // Expand row
    const titleCell = screen.getAllByText('Swap failed')[0];
    fireEvent.click(titleCell);

    const link = await screen.findByRole('link', { name: /0x/i });
    expect(link.getAttribute('href')).toMatch(/^https:\/\/testnet-explorer\.plumenetwork\.xyz\/tx\//);
  });
  it('handles API errors gracefully', async () => {
    (apiModule.api.getAlerts as any).mockRejectedValue(new Error('API Error'));
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

    render(<Alerts />);

    await waitFor(() => {
      expect(consoleError).toHaveBeenCalledWith(
        '[alerts] Failed to load:',
        expect.any(Error)
      );
      expect(mockStoreState.setRows).toHaveBeenCalledWith([]);
    });

    consoleError.mockRestore();
  });
});
