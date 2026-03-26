import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactElement, ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { StrictMode, Suspense, forwardRef, startTransition, useImperativeHandle, useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { colors } from '@/lib/tokens';

const dataTablePropsHistory: Array<{ data: unknown; liveDataVersion: unknown }> = [];
let renderActualDataTable = true;

vi.mock('@/components/ui/table/DataTable', async () => {
  const actual = await vi.importActual<any>('@/components/ui/table/DataTable');
  return {
    ...actual,
    DataTable: (props: any) => {
      dataTablePropsHistory.push({
        data: props.data,
        liveDataVersion: props.liveDataVersion,
      });
      if (!renderActualDataTable) {
        return null;
      }
      const ActualDataTable = actual.DataTable;
      return <ActualDataTable {...props} />;
    },
  };
});

import { api } from '@/api';
import SignalTable from '@/components/domain/signal/SignalTable';
import MakerV4SignalTable, { buildLegTooltip } from '@/components/domain/signal/MakerV4SignalTable';
import { useSignalStore } from '@/stores';
import type { SignalStrategy } from '@/types';

vi.mock('@/api', () => ({
  api: {
    getSignalStrategies: vi.fn(() => Promise.resolve({ strategies: [], server_time: '2024-01-01 12:00:00' })),
  },
}));

vi.mock('@/sockets', () => ({
  socket: {
    on: vi.fn(),
    off: vi.fn(),
    connected: false,
  },
}));

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

vi.mock('@/stores', async () => {
  const actual = await vi.importActual<any>('@/stores');
  return { ...actual, useSignalStore: vi.fn() };
});

vi.mock('@/components/ui/popover/Popover', () => ({
  default: ({ children }: any) => <div>{children}</div>,
  Popover: ({ children }: any) => <div>{children}</div>,
  PopoverTrigger: ({ children }: any) => <div>{children}</div>,
  PopoverContent: ({ children }: any) => <div>{children}</div>,
}));

let currentSignalState: any;

function initSignalState(state: any) {
  currentSignalState = state;
  const mockedUseSignalStore = useSignalStore as any;
  mockedUseSignalStore.getState = () => currentSignalState;
  mockedUseSignalStore.mockImplementation((selector?: any) =>
    selector ? selector(currentSignalState) : currentSignalState,
  );
}

function buildMakerV4Strategy(): SignalStrategy {
  return {
    id: 'aapl_tradexyz_makerv4',
    strategy_family: 'maker_v4',
    running: true,
    tradeable: false,
    blocked: false,
    balances_ok: true,
    params: { bot_on: '0', qty: '1' },
    meta: {
      chain: 'equities',
      strategy_groups: 'equities',
      strategy_family: 'maker_v4',
      strategy_version: 'v4',
      class: 'maker_v4',
      param_set: 'makerv4',
      base_asset: 'AAPL',
      quote_asset: 'USD',
    },
    maker_role_map: {
      maker_leg: 'maker',
      hedge_leg: 'hedge-blueocean',
      ref_leg: 'hedge',
    },
    legs_order: ['maker', 'hedge', 'hedge-blueocean'],
    legs: {
      maker: {
        exchange: 'hyperliquid',
        instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
        update_ts_ms: 1_700_000_000_000,
      },
      hedge: {
        exchange: 'ibkr',
        instrument_id: 'AAPL.NASDAQ',
        update_ts_ms: 1_700_000_000_500,
      },
      'hedge-blueocean': {
        exchange: 'ibkr',
        instrument_id: 'AAPL.BLUEOCEAN',
        update_ts_ms: 1_700_000_000_550,
      },
    },
    maker_v4: {
      operator: {
        execution_mode: 'take_take',
        behavior: 'take_take',
        hedge_policy: {
          route: 'SMART',
          time_in_force: 'DAY',
          outside_rth: true,
          include_overnight: true,
          cancel_after_ms: 5000,
        },
        fee_assumptions: {
          ibkr_fee_plan: 'tiered',
          ibkr_fee_min_usd: 0.35,
          maker_taker_fee_bps: 4.5,
          maker_maker_fee_bps: 0.25,
          assumed_hedge_fee_bps: 1.0,
        },
      },
      quote_snapshot: {
        ts_ms: 1_700_000_000_500,
        mid_spread_bps: 2.0,
        arb_bid_spread_bps: 14.0,
        arb_ask_spread_bps: -11.0,
        effective_spread_bps: 6.5,
        quoted_spread_bps: 8.0,
        expected_maker_fee_bps: 0.25,
        assumed_hedge_fee_bps: 1.0,
        hedge_ready: true,
        hedge_route: 'BLUEOCEAN',
        hedge_disabled_reason: null,
        fee_snapshot_age_s: 9,
        ibkr_quote_age_ms: 3_000,
        hedge_latency_ms: 45,
        hedge_slippage_bps_vs_mid: 1.5,
        maker_leg: {
          venue: 'HYPERLIQUID',
          instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
          age_ms: 1_000,
        },
        hedge_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.BLUEOCEAN',
          route: 'BLUEOCEAN',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
          age_ms: 2_000,
        },
        ref_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.NASDAQ',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
          age_ms: 3_000,
        },
      },
    },
    state: {
      ts_ms: 1_700_000_000_500,
      state: 'hedge_paused',
    },
  };
}

function buildTrackedMakerV4Strategy(id: string, accessCounts: Map<string, number>, overrides: Partial<SignalStrategy> = {}): SignalStrategy {
  const strategy = buildMakerV4Strategy();
  strategy.id = id;
  const makerV4 = overrides.maker_v4 ?? strategy.maker_v4;
  Object.assign(strategy, overrides);
  Object.defineProperty(strategy, 'maker_v4', {
    configurable: true,
    enumerable: true,
    get() {
      accessCounts.set(id, (accessCounts.get(id) ?? 0) + 1);
      return makerV4;
    },
  });
  return strategy;
}

function createSuspendGate() {
  let released = false;
  let resolvePromise: (() => void) | null = null;
  const promise = new Promise<void>((resolve) => {
    resolvePromise = () => {
      released = true;
      resolve();
    };
  });

  function Gate() {
    if (!released) {
      throw promise;
    }
    return null;
  }

  return {
    Gate,
    release() {
      resolvePromise?.();
    },
  };
}

function getVisibleMakerV4StrategyOrder(): string[] {
  const table = screen.getByRole('table');
  return Array.from(table.querySelectorAll('tbody tr'))
    .map((row) => row.querySelector('td')?.textContent?.trim() ?? '');
}

type TransitionHarnessHandle = {
  update(next: { liveDataVersion?: number; blocker?: ReactElement | null }): void;
};

const TransitionHarness = forwardRef<TransitionHarnessHandle, { rows: SignalStrategy[] }>(
  function TransitionHarness({ rows }, ref) {
    const [liveDataVersion, setLiveDataVersion] = useState(1);
    const [blocker, setBlocker] = useState<ReactNode>(null);

    useImperativeHandle(ref, () => ({
      update(next) {
        startTransition(() => {
          if (next.liveDataVersion !== undefined) {
            setLiveDataVersion(next.liveDataVersion);
          }
          setBlocker(next.blocker ?? null);
        });
      },
    }), []);

    return (
      <Suspense fallback={null}>
        <MakerV4SignalTable
          rows={rows}
          liveDataVersion={liveDataVersion}
        />
        {blocker}
      </Suspense>
    );
  },
);

async function flushAsyncRender() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('MakerV4SignalTable', () => {
  afterEach(() => {
    renderActualDataTable = true;
    dataTablePropsHistory.length = 0;
    vi.useRealTimers();
  });

  it('renders a dedicated maker v4 signal table with both venue legs, route, and strategy-published spreads', () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();
    expect(screen.getByText('Mode')).toBeInTheDocument();
    expect(screen.getByText('Mid Spread')).toBeInTheDocument();
    expect(screen.getByText('Arb Spread')).toBeInTheDocument();
    expect(screen.getByText('aapl_tradexyz_makerv4')).toBeInTheDocument();
    expect(screen.getByText(/Hyperliquid/i)).toBeInTheDocument();
    expect(screen.getByText(/IBKR/i)).toBeInTheDocument();
    expect(screen.getByText('2.0 bps')).toBeInTheDocument();
    expect(screen.getByText('B 14.0')).toBeInTheDocument();
    expect(screen.getByText('A -11.0')).toBeInTheDocument();
    expect(screen.getByText('Take-Take')).toBeInTheDocument();
    expect(screen.getAllByText(/BLUEOCEAN/i).length).toBeGreaterThan(0);
    expect(screen.getByText('Paused')).toBeInTheDocument();
  });

  it('renders the routed hedge identity and visible hedge latency from the quote snapshot', () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    expect(screen.getByText('IBKR AAPL.BLUEOCEAN')).toBeInTheDocument();
    expect(screen.getByText(/45 ms/i)).toBeInTheDocument();
  });

  it('uses worst-leg age and hides actionable spread and hedge states when quote health is stale', () => {
    const strategy = buildMakerV4Strategy();
    strategy.tradeable = false;
    strategy.blocked = true;
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      quote_snapshot: {
        ...strategy.maker_v4?.quote_snapshot,
        hedge_ready: true,
        hedge_disabled_reason: null,
        ref_leg: {
          ...strategy.maker_v4?.quote_snapshot?.ref_leg,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          reason_code: 'reference_quote_old',
          age_ms: 9_000,
        },
      },
    } as any;

    render(
      <MakerV4SignalTable
        rows={[strategy]}
        nowProvider={() => 1_700_000_010_000}
      />,
    );

    expect(screen.getByRole('button', { name: /Age/i })).toBeInTheDocument();
    expect(screen.getByText('9s')).toBeInTheDocument();
    expect(screen.getByText(/Paused/i)).toBeInTheDocument();
    expect(screen.getByText(/Blocked/i)).toBeInTheDocument();
    expect(screen.getByText(/Quote health/i)).toBeInTheDocument();
    expect(screen.getAllByText(/^stale$/i).length).toBeGreaterThanOrEqual(2);
    expect(screen.queryByText('2.0 bps')).not.toBeInTheDocument();
    expect(screen.queryByText('B 14.0')).not.toBeInTheDocument();
    expect(screen.queryByText('A -11.0')).not.toBeInTheDocument();
  });

  it('shows quote health separately from age and surfaces hedge backlog state', () => {
    const strategy = buildMakerV4Strategy();
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      quote_snapshot: {
        ...strategy.maker_v4?.quote_snapshot,
        maker_leg: {
          ...strategy.maker_v4?.quote_snapshot?.maker_leg,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          reason_code: 'maker_quote_old',
        },
      },
    } as any;
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      operator: {
        ...strategy.maker_v4?.operator,
        hedge_backlog: {
          fill_id: 'take_take:order-1',
          side: 'SELL',
          requested_qty: '1',
          blocked_reason: 'stale_quote',
          fill_ts_ms: 1_700_000_000_450,
          maker_fee_bps: 0.25,
        },
      },
    } as any;

    render(
      <MakerV4SignalTable
        rows={[strategy]}
      />,
    );

    expect(screen.getByText('Feed ok · Quote old')).toBeInTheDocument();
    expect(screen.getByText(/Backlog/i)).toBeInTheDocument();
    expect(screen.getByText(/SELL 1/)).toBeInTheDocument();
  });

  it('sorts stale mid-spread rows after numeric rows', () => {
    const fresh = buildMakerV4Strategy();
    fresh.id = 'fresh_row';
    fresh.maker_v4 = {
      ...fresh.maker_v4,
      quote_snapshot: {
        ...fresh.maker_v4?.quote_snapshot,
        mid_spread_bps: 1.0,
      },
    } as any;

    const stale = buildMakerV4Strategy();
    stale.id = 'stale_row';
    stale.maker_v4 = {
      ...stale.maker_v4,
      quote_snapshot: {
        ...stale.maker_v4?.quote_snapshot,
        maker_leg: {
          ...stale.maker_v4?.quote_snapshot?.maker_leg,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          age_ms: 25_000,
        },
        mid_spread_bps: 99.0,
      },
    } as any;

    render(
      <MakerV4SignalTable
        rows={[stale, fresh]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /Mid Spread/i }));

    const strategyCells = screen.getAllByText(/_row$/i);
    expect(strategyCells.map((node) => node.textContent)).toEqual(['fresh_row', 'stale_row']);
  });

  it('shows fee assumptions when hovering the strategy id', async () => {
    render(
      <MakerV4SignalTable
        rows={[buildMakerV4Strategy()]}
      />,
    );

    const strategyId = screen.getByText('aapl_tradexyz_makerv4');
    expect(strategyId).toHaveAttribute('title', expect.stringContaining('IBKR fee plan: tiered'));
    expect(strategyId).toHaveAttribute(
      'title',
      expect.stringContaining('Maker taker fee: 4.50 bps'),
    );
    expect(strategyId).toHaveAttribute('title', expect.stringContaining('Assumed hedge fee: 1.00 bps'));
  });

  it('reconciles in-place row mutations through liveDataVersion without rebuilding the maker v4 data array', async () => {
    dataTablePropsHistory.length = 0;
    const strategy = buildMakerV4Strategy();
    const rows = [strategy];
    const { rerender } = render(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={1}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('2.0 bps')).toBeInTheDocument();
    });

    const initial = dataTablePropsHistory.at(-1);
    strategy.maker_v4 = {
      ...strategy.maker_v4,
      quote_snapshot: {
        ...strategy.maker_v4?.quote_snapshot,
        mid_spread_bps: 3.0,
      },
    } as any;

    rerender(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={2}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('3.0 bps')).toBeInTheDocument();
    });

    const latest = dataTablePropsHistory.at(-1);
    expect(latest?.data).toBe(initial?.data);
    expect(latest?.liveDataVersion).not.toBe(initial?.liveDataVersion);
  });

  it('reconciles nested in-place state mutations through liveDataVersion without rebuilding the maker v4 data array', async () => {
    dataTablePropsHistory.length = 0;
    const strategy = buildMakerV4Strategy();
    strategy.running = undefined as any;
    strategy.params = {
      ...strategy.params,
      bot_on: '1',
    };
    const rows = [strategy];
    const { rerender } = render(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={1}
        nowProvider={() => 1_700_000_000_500}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Enabled')).toBeInTheDocument();
    });

    const initial = dataTablePropsHistory.at(-1);
    if (!strategy.state) {
      throw new Error('expected strategy state');
    }
    strategy.state.ts_ms = 1_699_999_994_500;

    rerender(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={2}
        nowProvider={() => 1_700_000_000_500}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Pending')).toBeInTheDocument();
    });

    const latest = dataTablePropsHistory.at(-1);
    expect(latest?.data).toBe(initial?.data);
    expect(latest?.liveDataVersion).not.toBe(initial?.liveDataVersion);
  });

  it('recomputes derived trading status for nested in-place maker v4 mutations when liveDataVersion changes without changedRowIds', async () => {
    dataTablePropsHistory.length = 0;
    const strategy = buildMakerV4Strategy();
    strategy.params = {
      ...strategy.params,
      bot_on: '1',
    };
    const rows = [strategy];
    const { rerender } = render(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={1}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Enabled')).toBeInTheDocument();
    });

    const initial = dataTablePropsHistory.at(-1);
    const initialRow = (initial?.data as Array<{ _statusLabel?: { label?: string } }> | undefined)?.[0];
    expect(initialRow?._statusLabel?.label).toBe('Enabled');
    if (!strategy.maker_v4?.quote_snapshot) {
      throw new Error('expected maker_v4 quote snapshot in test fixture');
    }
    strategy.maker_v4.quote_snapshot.hedge_ready = false;

    rerender(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={2}
      />,
    );

    const latest = dataTablePropsHistory.at(-1);
    const latestRow = (latest?.data as Array<{ _statusLabel?: { label?: string } }> | undefined)?.[0];
    await waitFor(() => {
      expect(latestRow?._statusLabel?.label).toBe('Pending');
    });
    expect(latest?.data).toBe(initial?.data);
    expect(latest?.liveDataVersion).not.toBe(initial?.liveDataVersion);
  });

  it('preserves the full-row liveDataVersion fallback across strict-mode double renders', async () => {
    dataTablePropsHistory.length = 0;
    const strategy = buildMakerV4Strategy();
    strategy.params = {
      ...strategy.params,
      bot_on: '1',
    };
    const rows = [strategy];
    const { rerender } = render(
      <StrictMode>
        <MakerV4SignalTable
          rows={rows}
          liveDataVersion={1}
        />
      </StrictMode>,
    );

    await waitFor(() => {
      expect(screen.getByText('Enabled')).toBeInTheDocument();
    });
    const committedRow = (dataTablePropsHistory.at(-1)?.data as Array<{ _statusLabel?: { label?: string } }> | undefined)?.[0];
    expect(committedRow?._statusLabel?.label).toBe('Enabled');

    if (!strategy.maker_v4?.quote_snapshot) {
      throw new Error('expected maker_v4 quote snapshot in test fixture');
    }
    strategy.maker_v4.quote_snapshot.hedge_ready = false;

    rerender(
      <StrictMode>
        <MakerV4SignalTable
          rows={rows}
          liveDataVersion={2}
        />
      </StrictMode>,
    );

    await waitFor(() => {
      const latest = dataTablePropsHistory.at(-1);
      const latestRow = (latest?.data as Array<{ _statusLabel?: { label?: string } }> | undefined)?.[0];
      expect(latestRow?._statusLabel?.label).toBe('Pending');
    });
  });

  it('retries the full-row liveDataVersion fallback after an interrupted suspended render', async () => {
    dataTablePropsHistory.length = 0;
    const strategy = buildMakerV4Strategy();
    strategy.params = {
      ...strategy.params,
      bot_on: '1',
    };
    const rows = [strategy];
    const harnessRef: { current: TransitionHarnessHandle | null } = { current: null };
    render(
      <TransitionHarness
        ref={(value) => {
          harnessRef.current = value;
        }}
        rows={rows}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Enabled')).toBeInTheDocument();
    });
    const committedRow = (dataTablePropsHistory.at(-1)?.data as Array<{ _statusLabel?: { label?: string } }> | undefined)?.[0];
    expect(committedRow?._statusLabel?.label).toBe('Enabled');

    if (!strategy.maker_v4?.quote_snapshot) {
      throw new Error('expected maker_v4 quote snapshot in test fixture');
    }
    strategy.maker_v4.quote_snapshot.hedge_ready = false;

    const gate = createSuspendGate();
    act(() => {
      harnessRef.current?.update({
        liveDataVersion: 2,
        blocker: <gate.Gate />,
      });
    });

    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByText('Enabled')).toBeInTheDocument();
    expect(committedRow?._statusLabel?.label).toBe('Enabled');

    await act(async () => {
      gate.release();
      await Promise.resolve();
      await Promise.resolve();
    });

    await waitFor(() => {
      const latest = dataTablePropsHistory.at(-1);
      const latestRow = (latest?.data as Array<{ _statusLabel?: { label?: string } }> | undefined)?.[0];
      expect(latestRow?._statusLabel?.label).toBe('Pending');
    });
  });

  it('reconciles nested in-place maker v4 mutations when liveDataVersion changes without changedRowIds', async () => {
    dataTablePropsHistory.length = 0;
    const strategy = buildMakerV4Strategy();
    const rows = [strategy];
    const { rerender } = render(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={1}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('2.0 bps')).toBeInTheDocument();
    });

    if (!strategy.maker_v4?.quote_snapshot?.maker_leg) {
      throw new Error('Expected maker leg quote snapshot');
    }
    strategy.maker_v4.quote_snapshot.maker_leg.feed_state = 'stale';

    rerender(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={2}
      />,
    );

    await waitFor(() => {
      expect(screen.getAllByText('stale').length).toBeGreaterThanOrEqual(1);
    });

    const latest = dataTablePropsHistory.at(-1);
    expect(latest?.data).toBe(dataTablePropsHistory[0]?.data);
    expect(latest?.liveDataVersion).toBe(2);
  });

  it('avoids rebuilding untouched maker v4 rows on a single-row live update', async () => {
    renderActualDataTable = false;
    const accessCounts = new Map<string, number>();
    const firstRow = buildTrackedMakerV4Strategy('maker_v4_first', accessCounts);
    const changedRow = buildTrackedMakerV4Strategy('maker_v4_changed', accessCounts);
    const thirdRow = buildTrackedMakerV4Strategy('maker_v4_third', accessCounts);

    const { rerender } = render(
      <MakerV4SignalTable
        rows={[firstRow, changedRow, thirdRow]}
        liveDataVersion={1}
      />,
    );

    await waitFor(() => {
      expect(dataTablePropsHistory.at(-1)?.liveDataVersion).toBe(1);
    });

    const countsBefore = new Map(accessCounts);
    const updatedChangedRow = buildTrackedMakerV4Strategy('maker_v4_changed', accessCounts, {
      maker_v4: {
        ...changedRow.maker_v4,
        quote_snapshot: {
          ...changedRow.maker_v4?.quote_snapshot,
          mid_spread_bps: 9.0,
        },
      } as any,
    });

    rerender(
      <MakerV4SignalTable
        rows={[firstRow, updatedChangedRow, thirdRow]}
        liveDataVersion={2}
        changedRowIds={['maker_v4_changed']}
      />,
    );

    await waitFor(() => {
      expect(dataTablePropsHistory.at(-1)?.liveDataVersion).toBe(2);
    });

    expect(accessCounts.get('maker_v4_first')).toBe(countsBefore.get('maker_v4_first'));
    expect(accessCounts.get('maker_v4_third')).toBe(countsBefore.get('maker_v4_third'));
    expect(accessCounts.get('maker_v4_changed')).toBeGreaterThan(countsBefore.get('maker_v4_changed') ?? 0);
  });

  it('recomputes the mid-spread sort order after a committed liveDataVersion patch', async () => {
    const lowerSpread = buildMakerV4Strategy();
    lowerSpread.id = 'maker_v4_low';
    if (!lowerSpread.maker_v4?.quote_snapshot) {
      throw new Error('expected lower spread maker_v4 quote snapshot');
    }
    lowerSpread.maker_v4.quote_snapshot.mid_spread_bps = 2.0;

    const higherSpread = buildMakerV4Strategy();
    higherSpread.id = 'maker_v4_high';
    if (!higherSpread.maker_v4?.quote_snapshot) {
      throw new Error('expected higher spread maker_v4 quote snapshot');
    }
    higherSpread.maker_v4.quote_snapshot.mid_spread_bps = 5.0;

    const rows = [lowerSpread, higherSpread];
    const { rerender } = render(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={1}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('2.0 bps')).toBeInTheDocument();
      expect(screen.getByText('5.0 bps')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Mid Spread'));
    await waitFor(() => {
      expect(getVisibleMakerV4StrategyOrder()).toEqual(['maker_v4_high', 'maker_v4_low']);
    });

    lowerSpread.maker_v4.quote_snapshot.mid_spread_bps = 9.0;

    rerender(
      <MakerV4SignalTable
        rows={rows}
        liveDataVersion={2}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('9.0 bps')).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(getVisibleMakerV4StrategyOrder()).toEqual(['maker_v4_low', 'maker_v4_high']);
    });
  });

  it('switches the equities signal route to the dedicated maker v4 table while filters stay available', async () => {
    vi.clearAllMocks();
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies: [buildMakerV4Strategy()],
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: [buildMakerV4Strategy()],
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });

    render(
      <MemoryRouter
        initialEntries={['/equities/signal']}
        future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
      >
        <SignalTable />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('maker-v4-signal-table')).toBeInTheDocument();
    });

    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();
    expect(screen.getByText('Mid Spread')).toBeInTheDocument();
    expect(screen.getByText('Arb Spread')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Filters'));
    expect(screen.getByPlaceholderText(/Strategy ID/i)).toBeInTheDocument();
    expect(screen.getByTestId('maker-v4-signal-table')).toBeInTheDocument();
    expect(screen.queryByText('FV market')).not.toBeInTheDocument();
  });

  it('keeps Maker V4 route age, newest-leg recency, and stale color ticking on a quiet feed', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_000_500);

    const strategy = buildMakerV4Strategy();
    (api.getSignalStrategies as any).mockResolvedValue({
      strategies: [strategy],
      server_time: '2024-01-01 12:00:00',
      server_ts_ms: 1_700_000_001_500,
    });
    initSignalState({
      rows: [strategy],
      setRows: vi.fn(),
      mergeStrategy: vi.fn(),
      mergeStrategies: vi.fn(),
    });

    const { container } = render(
      <MemoryRouter
        initialEntries={['/equities/signal']}
        future={{ v7_startTransition: true, v7_relativeSplatPath: true }}
      >
        <SignalTable />
      </MemoryRouter>,
    );

    await flushAsyncRender();
    expect(screen.getByTestId('maker-v4-signal-table')).toBeInTheDocument();

    let ageCell = container.querySelector('tbody tr td:nth-child(9) span') as HTMLElement | null;
    let lastUpdatedCell = container.querySelector('tbody tr td:nth-child(10) span') as HTMLElement | null;
    expect(ageCell).not.toBeNull();
    expect(lastUpdatedCell).not.toBeNull();
    expect(ageCell?.textContent).toContain('3s');
    expect(ageCell).toHaveStyle({ color: colors.text.primary });
    expect(lastUpdatedCell?.textContent).toContain('(1s ago)');

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    await flushAsyncRender();

    ageCell = container.querySelector('tbody tr td:nth-child(9) span') as HTMLElement | null;
    lastUpdatedCell = container.querySelector('tbody tr td:nth-child(10) span') as HTMLElement | null;
    expect(ageCell?.textContent).toContain('4s');
    expect(ageCell).toHaveStyle({ color: colors.semantic.warning.DEFAULT });
    expect(lastUpdatedCell?.textContent).toContain('(2s ago)');
  });

  it('realigns the first post-sync frame to the server clock even when the browser clock was ahead', async () => {
    let currentNow = 1_700_000_002_500;
    const stableNowProvider = () => currentNow;
    const strategy = buildMakerV4Strategy();
    const { rerender, container } = render(
      <MakerV4SignalTable
        rows={[strategy]}
        nowProvider={stableNowProvider}
      />,
    );

    let ageCell = container.querySelector('tbody tr td:nth-child(9) span') as HTMLElement | null;
    let lastUpdatedCell = container.querySelector('tbody tr td:nth-child(10) span') as HTMLElement | null;
    expect(ageCell?.textContent).toContain('3s');
    expect(lastUpdatedCell?.textContent).toContain('(1s ago)');

    currentNow = 1_700_000_001_500;
    rerender(
      <MakerV4SignalTable
        rows={[strategy]}
        nowProvider={stableNowProvider}
        clockAnchorMs={1_700_000_001_500}
      />,
    );

    ageCell = container.querySelector('tbody tr td:nth-child(9) span') as HTMLElement | null;
    lastUpdatedCell = container.querySelector('tbody tr td:nth-child(10) span') as HTMLElement | null;
    expect(ageCell?.textContent).toContain('3s');
    expect(lastUpdatedCell?.textContent).toContain('(1s ago)');
  });

  it('does not reset unchanged maker v4 ages when the server clock advances without row updates', async () => {
    let currentNow = 1_700_000_001_500;
    const stableNowProvider = () => currentNow;
    const strategy = buildMakerV4Strategy();
    const { rerender, container } = render(
      <MakerV4SignalTable
        rows={[strategy]}
        nowProvider={stableNowProvider}
        clockAnchorMs={1_700_000_001_500}
      />,
    );

    let ageCell = container.querySelector('tbody tr td:nth-child(9) span') as HTMLElement | null;
    let lastUpdatedCell = container.querySelector('tbody tr td:nth-child(10) span') as HTMLElement | null;
    expect(ageCell?.textContent).toContain('3s');
    expect(lastUpdatedCell?.textContent).toContain('(1s ago)');

    currentNow = 1_700_000_002_500;
    rerender(
      <MakerV4SignalTable
        rows={[strategy]}
        nowProvider={stableNowProvider}
        clockAnchorMs={1_700_000_002_500}
      />,
    );

    ageCell = container.querySelector('tbody tr td:nth-child(9) span') as HTMLElement | null;
    lastUpdatedCell = container.querySelector('tbody tr td:nth-child(10) span') as HTMLElement | null;
    expect(ageCell?.textContent).toContain('4s');
    expect(lastUpdatedCell?.textContent).toContain('(2s ago)');
  });

  it('keeps maker market tooltip ages ticking from the live age anchor', () => {
    const strategy = buildMakerV4Strategy();
    const makerLeg = strategy.maker_v4?.quote_snapshot?.maker_leg ?? null;

    expect(
      buildLegTooltip(makerLeg, 1_700_000_001_500, 1_700_000_001_500, null),
    ).toContain('Age: 1s');
    expect(
      buildLegTooltip(makerLeg, 1_700_000_001_500, 1_700_000_002_500, null),
    ).toContain('Age: 2s');
  });
});
