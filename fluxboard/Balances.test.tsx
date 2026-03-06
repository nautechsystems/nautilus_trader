import { render, screen, waitFor, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';

import Balances from './Balances';
import { api } from './api';
import { PanelWrapper } from './components/layout/PanelWrapper';
import { useBalancesStore } from './stores';
import { INTERVALS } from './constants';

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./api')>();
  return {
    api: {
      ...actual.api,
      getBalances: vi.fn(),
    },
  };
});

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const mockedApi = vi.mocked(api, true);
const mockUsePolling = vi.fn();
let seenFetchFns: WeakSet<() => unknown>;

vi.mock('./hooks', () => ({
  usePolling: (fn: () => unknown | Promise<unknown>, interval: number, enabled?: boolean) =>
    mockUsePolling(fn, interval, enabled),
}));

const buildPayload = () => ({
  rows: [
    {
      id: 'PLUME_LOGICAL',
      coin: 'PLUME_LOGICAL',
      canonical: 'PLUME',
      is_parent: true,
      stable: false,
      qty_display: '1,500',
      qty_raw: 1500,
      mv_display: '$75.50',
      mv_raw: 75.5,
      mark_display: '0.0503',
      mark_raw: 0.0503,
      time_display: '1h ago',
      time_iso: '2024-01-01T01:00:00Z',
      last_ts: 1704070800000,
      raw: { qty: 1500, mv_usd: 75.5, mark: 0.0503 },
      children: [
        {
          id: 'PLUME_LOGICAL:PLUME:bybit_plume',
          parent_id: 'PLUME_LOGICAL',
          coin: 'PLUME',
          venue: 'bybit',
          wallet: 'bybit-unified',
          address: null,
          label: null,
          contract: 'PLUMEUSDT-LINEAR.BYBIT',
          qty_display: '1,000',
          qty_raw: 1000,
          mv_display: '$50.00',
          mv_raw: 50,
          mark_display: '0.0500',
          mark_raw: 0.05,
          time_display: '1h ago',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
        },
        {
          id: 'PLUME_LOGICAL:WPLUME:wallet_wplume',
          parent_id: 'PLUME_LOGICAL',
          coin: 'WPLUME',
          venue: 'wallet',
          wallet: 'treasury',
          address: '0xabc1234567890000000000000000000000000000',
          label: 'Treasury',
          qty_display: '500',
          qty_raw: 500,
          mv_display: '$25.50',
          mv_raw: 25.5,
          mark_display: '0.0510',
          mark_raw: 0.051,
          time_display: '1h ago',
          time_iso: '2024-01-01T01:00:00Z',
          last_ts: 1704070800000,
        },
      ],
    },
  ],
  total: 1,
  totals: {
    mv_raw: 75.5,
    mv_display: '$75.50',
    net_mv_raw: 75.5,
    net_mv_display: '$75.50',
    long_mv_raw: 75.5,
    long_mv_display: '$75.50',
    short_mv_raw: 0,
    short_mv_display: '$0.00',
    gross_mv_raw: 75.5,
    gross_mv_display: '$75.50',
    stable_mv_raw: 0,
    stable_mv_display: '$0.00',
    non_stable_mv_raw: 75.5,
    non_stable_mv_display: '$75.50'
  },
  generated_at: '2024-01-01T01:05:00Z',
  view: 'parents_only',
  risk_groups: [],
});

describe('Balances component', () => {
  beforeEach(() => {
    seenFetchFns = new WeakSet();
    localStorage.clear();
    useBalancesStore.setState({
      rows: [],
      totals: null,
      totalCount: 0,
      generatedAt: undefined,
      loading: false,
      lastUpdate: undefined,
    });
    mockedApi.getBalances.mockResolvedValue(buildPayload());
    mockUsePolling.mockImplementation((fn, _interval, enabled = true) => {
      if (!enabled) return;
      if (seenFetchFns.has(fn)) {
        return;
      }
      seenFetchFns.add(fn);
      Promise.resolve().then(() => {
        void fn();
      });
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
    mockUsePolling.mockReset();
  });

  it('polls balances at the configured interval', async () => {
    render(<Balances />);
    await waitFor(() => {
      expect(mockUsePolling).toHaveBeenCalledWith(expect.any(Function), INTERVALS.BALANCES_POLL, undefined);
      expect(mockedApi.getBalances).toHaveBeenCalledTimes(1);
    });

    const [pollFn] = mockUsePolling.mock.calls[0];
    await act(async () => {
      await pollFn();
    });

    await waitFor(() => {
      expect(mockedApi.getBalances).toHaveBeenCalledTimes(2);
    });
  });

  it('renders parent rows with totals', async () => {
    render(<Balances />);
    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(screen.getByText('Net Equity (Σ MV): $75.50')).toBeInTheDocument();
  });

  it('prefers authoritative totals.mv_display when net_mv_display is missing', async () => {
    const payload = buildPayload();
    mockedApi.getBalances.mockResolvedValueOnce({
      ...payload,
      totals: {
        mv_raw: 999.12,
        mv_display: '$999.12',
      },
    } as any);

    render(<Balances />);
    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(screen.getByText('Net Equity (Σ MV): $999.12')).toBeInTheDocument();
  });

  it('defaults to expanded rows and allows collapsing from the header control', async () => {
    const user = userEvent.setup();
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    const expandAllButton = await screen.findByRole('button', { name: /collapse all/i });

    // Parents with children should be expanded by default
    expect(await screen.findByText('0xabc1…0000')).toBeInTheDocument();

    await user.click(expandAllButton);

    // Button should switch to "Expand all" and rows collapse when clicked
    expect(expandAllButton).toHaveTextContent(/expand all/i);
    await waitFor(() => {
      expect(screen.queryByText('0xabc1…0000')).not.toBeInTheDocument();
    });

    await user.click(expandAllButton);
    expect(await screen.findByText('0xabc1…0000')).toBeInTheDocument();
  });

  it('renders full product contract inline for exchange balances', async () => {
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    expect(
      screen.getAllByText((_, element) =>
        String(element?.textContent ?? '').includes('bybit') &&
        String(element?.textContent ?? '').includes('bybit-unified') &&
        String(element?.textContent ?? '').includes('PLUMEUSDT-LINEAR.BYBIT')
      ).length
    ).toBeGreaterThan(0);
    expect(screen.queryByText('PLUMEU...YBIT')).not.toBeInTheDocument();
  });

  it('surfaces header actions when embedded in dashboard wrapper', async () => {
    render(
      <PanelWrapper title="Balances">
        <Balances showHeader={false} />
      </PanelWrapper>
    );

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await screen.findByText('Balances');

    await screen.findByRole('button', { name: /collapse all/i });
    await screen.findByRole('button', { name: /filters/i });
  });

  it('switches to child view when logical parents are disabled', async () => {
    const user = userEvent.setup();
    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: /Filters/i }));
    const switches = screen.getAllByRole('switch');
    // Toggle "Show logical parents only" (second switch)
    await user.click(switches[1]);

    expect(await screen.findByText(/wallet \(treasury\)/i)).toBeInTheDocument();
  });

  it('renders wallet addresses with copy-to-clipboard support for child rows', async () => {
    const user = userEvent.setup();
    localStorage.setItem(
      'balances:filters:v1',
      JSON.stringify({
        hideZero: true,
        logicalOnly: false,
        stableOnly: false,
        sortBy: 'mv',
      }),
    );

    const clipboardMock = {
      writeText: vi.fn(() => Promise.resolve()),
    } as unknown as Clipboard;
    const originalClipboard = navigator.clipboard;
    Object.defineProperty(window.navigator, 'clipboard', {
      configurable: true,
      value: clipboardMock,
    });

    render(<Balances />);
    const addressButton = await screen.findByRole('button', { name: /copy wallet address/i });
    expect(addressButton).toHaveTextContent('0xabc1…0000');

    await user.click(addressButton);

    await waitFor(() => {
      expect(clipboardMock.writeText).toHaveBeenCalledWith('0xabc1234567890000000000000000000000000000');
    });

    if (originalClipboard) {
      Object.defineProperty(window.navigator, 'clipboard', {
        configurable: true,
        value: originalClipboard,
      });
    } else {
      // @ts-expect-error clean up mocked clipboard
      delete navigator.clipboard;
    }
  });

  it('sorts rows when clicking sortable headers', async () => {
    const user = userEvent.setup();
    const payload = {
      rows: [
        {
          id: 'BBB_LOGICAL',
          coin: 'BBB_LOGICAL',
          canonical: 'BBB',
          is_parent: true,
          stable: false,
          qty_display: '2,000',
          qty_raw: 2000,
          mv_display: '$20.00',
          mv_raw: 20,
          mark_display: '10.0',
          mark_raw: 10,
          time_display: 'just now',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
          raw: { qty: 2000, mv_usd: 20, mark: 10 },
          children: [],
        },
        {
          id: 'AAA_LOGICAL',
          coin: 'AAA_LOGICAL',
          canonical: 'AAA',
          is_parent: true,
          stable: false,
          qty_display: '1,000',
          qty_raw: 1000,
          mv_display: '$10.00',
          mv_raw: 10,
          mark_display: '9.0',
          mark_raw: 9,
          time_display: 'just now',
          time_iso: '2024-01-01T00:00:00Z',
          last_ts: 1704067200000,
          raw: { qty: 1000, mv_usd: 10, mark: 9 },
          children: [],
        },
      ],
      total: 2,
      totals: { mv_raw: 30, mv_display: '$30.00' },
      generated_at: '2024-01-01T00:00:00Z',
      view: 'parents_only',
    };
    mockedApi.getBalances.mockResolvedValue(payload);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('BBB')).toBeInTheDocument();
    });

    const coinCells = screen.getAllByRole('cell', { name: /AAA|BBB/ });
    expect(coinCells[0]).toHaveTextContent('BBB');
    expect(coinCells[1]).toHaveTextContent('AAA');

    await user.click(screen.getByRole('button', { name: /sort by coin/i }));

    const resorted = screen.getAllByRole('cell', { name: /AAA|BBB/ });
    expect(resorted[0]).toHaveTextContent('AAA');
    expect(resorted[1]).toHaveTextContent('BBB');
  });

  it('renders Risk view using risk_groups data', async () => {
    const user = userEvent.setup();
    const payload = {
      ...buildPayload(),
      risk_groups: [
        {
          risk_key: 'PLUME',
          label: 'PLUME',
          net_qty: 1500,
          net_mv: 75.5,
          long_mv: 75.5,
          short_mv: 0,
          gross_mv: 75.5,
          abs_net_mv: 75.5,
          hedge_ratio: 0,
          sources: ['bybit', 'wallet'],
        },
      ],
    };
    mockedApi.getBalances.mockResolvedValue(payload);

    render(<Balances />);

    await waitFor(() => {
      expect(screen.getByText('PLUME')).toBeInTheDocument();
    });

    await user.click(screen.getByRole('button', { name: 'Risk' }));

    // Risk row label + sources badges should be visible
    const plumeLabels = await screen.findAllByText('PLUME');
    expect(plumeLabels.length).toBeGreaterThan(0);
    expect(await screen.findByText('bybit')).toBeInTheDocument();
    expect(await screen.findByText('wallet')).toBeInTheDocument();
  });
});
