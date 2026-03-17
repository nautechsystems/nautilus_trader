import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import SignalTable from '@/components/domain/signal/SignalTable';
import { useSignalStore } from '@/stores';
import * as apiModule from '@/api';
import type { SignalStrategy } from '@/types';

vi.mock('@/hooks/useMobileLayout', () => ({
  useMobileLayout: () => ({
    viewport: 'desktop',
    isMobile: false,
    isMobileViewport: false,
    density: 'desktop',
    isTouch: false,
    width: 1280,
    height: 720,
  }),
  useDensityMode: () => 'desktop',
}));

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(),
  },
}));

vi.mock('@/sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: false,
  },
}));

vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn() };
});

let currentSignalState: any;

function initSignalState(state: any) {
  currentSignalState = {
    rows: [],
    setRows: vi.fn(),
    mergeStrategy: vi.fn(),
    mergeStrategies: vi.fn(),
    ...state,
  };
  (useSignalStore as any).mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState
  );
  (useSignalStore as any).getState = () => currentSignalState;
}

function buildStrategy(referenceInstrumentId: string, route?: string | null): SignalStrategy {
  return {
    id: 'aapl_tradexyz_makerv4',
    params: { bot_on: '1' } as any,
    running: true,
    state: { state: 'running', ts_ms: Date.now(), bot_on: true } as any,
    strategy_family: 'maker_v4',
    meta: {
      class: 'equity_perp_maker_v3',
      strategy_groups: 'equities',
      chain: 'equities',
    } as any,
    maker_v3: {
      quote_snapshot: {
        maker_exchange: 'hyperliquid',
        maker_symbol: 'XYZ:AAPL Perp',
        ref_exchange: 'ibkr',
        ref_symbol: 'AAPL Spot',
        place_bid: 250.1,
        place_ask: 250.2,
        ref_bid: 250.0,
        ref_ask: 250.3,
      },
    } as any,
    legs: {
      A: {
        exchange: 'hyperliquid',
        coin: 'AAPL',
        instrument_id: 'XYZ:AAPL-USD-PERP.HYPERLIQUID',
        decision_bid: 250.1,
        decision_ask: 250.2,
        update_time: '2026-03-16 05:00:00',
      },
      B: {
        exchange: 'ibkr',
        coin: 'AAPL',
        instrument_id: referenceInstrumentId,
        route,
        decision_bid: 250.0,
        decision_ask: 250.3,
        update_time: '2026-03-16 05:00:00',
      },
    },
    balances_ok: true,
  } as any;
}

function renderSignalTable(pathname: string) {
  return render(
    <MemoryRouter initialEntries={[pathname]}>
      <SignalTable />
    </MemoryRouter>
  );
}

describe('Equities signal source badge', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (apiModule.api.getSignalStrategies as any).mockResolvedValue({
      strategies: [],
      server_time: '2026-03-16 05:00:01',
      server_ts_ms: Date.now(),
    });
  });

  it('shows the configured reference route on the equities signal page', async () => {
    initSignalState({ rows: [buildStrategy('AAPL.BLUEOCEAN')] });

    renderSignalTable('/equities/signal');

    await waitFor(() => expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument());

    const row = screen.getByText('aapl_tradexyz_makerv4').closest('tr');
    expect(row).not.toBeNull();
    expect(row as HTMLElement).toHaveTextContent('BLUEOCEAN');
  });

  it('prefers an explicit route field over the instrument id suffix', async () => {
    initSignalState({ rows: [buildStrategy('AAPL.NASDAQ', 'SMART')] });

    renderSignalTable('/equities/signal');

    await waitFor(() => expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument());

    const row = screen.getByText('aapl_tradexyz_makerv4').closest('tr');
    expect(row).not.toBeNull();
    expect(row as HTMLElement).toHaveTextContent('SMART');
    expect(row as HTMLElement).not.toHaveTextContent('NASDAQ');
  });

  it('does not show the source badge outside the equities signal page', async () => {
    initSignalState({ rows: [buildStrategy('AAPL.BLUEOCEAN')] });

    renderSignalTable('/signal');

    await waitFor(() => expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument());

    expect(screen.queryByText('BLUEOCEAN')).not.toBeInTheDocument();
  });
});
