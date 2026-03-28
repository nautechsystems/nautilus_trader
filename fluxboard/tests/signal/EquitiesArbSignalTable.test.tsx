import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import EquitiesArbSignalTable from '@/components/domain/signal/EquitiesArbSignalTable';
import { __resetViewportClockRegistryForTests } from '@/hooks/useViewportClock';
import type { SignalStrategy } from '@/types';

function buildEquitiesStrategy(
  id: string,
  strategyFamily: 'equities_maker' | 'equities_taker',
  label: 'Maker' | 'Taker',
): SignalStrategy {
  return {
    id,
    strategy_family: strategyFamily,
    running: true,
    tradeable: true,
    blocked: false,
    balances_ok: true,
    params: { bot_on: '1', qty: '1' },
    meta: {
      chain: 'equities',
      strategy_groups: 'equities',
      strategy_family: strategyFamily,
      class: strategyFamily,
      param_set: strategyFamily,
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
    equities_arb: {
      operator: {
        execution_mode: strategyFamily === 'equities_taker' ? 'take_take' : 'maker_hedge',
        behavior: strategyFamily === 'equities_taker' ? 'take_take' : 'maker',
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
          hl_taker_fee_bps: 4.5,
          hl_maker_fee_bps: 0.25,
          assumed_hedge_fee_bps: 1.0,
        },
      },
      quote_snapshot: {
        ts_ms: 1_700_000_000_500,
        mid_spread_bps: strategyFamily === 'equities_taker' ? 1.5 : 2.0,
        arb_bid_spread_bps: 14.0,
        arb_ask_spread_bps: -11.0,
        effective_spread_bps: 6.5,
        quoted_spread_bps: 8.0,
        expected_maker_fee_bps: 0.25,
        assumed_hedge_fee_bps: 1.0,
        hedge_ready: true,
        hedge_route: 'BLUEOCEAN',
        fee_snapshot_age_s: 9,
        hedge_latency_ms: 45,
        hedge_slippage_bps_vs_mid: 1.5,
        maker_leg: {
          venue: 'HYPERLIQUID',
          instrument_id: 'xyz:AAPL-USD-PERP.HYPERLIQUID',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
        },
        hedge_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.BLUEOCEAN',
          route: 'BLUEOCEAN',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
        },
        ref_leg: {
          venue: 'IBKR',
          instrument_id: 'AAPL.NASDAQ',
          feed_state: 'ok',
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
        },
      },
    },
    state: {
      ts_ms: 1_700_000_000_500,
      state: label.toLowerCase(),
    },
  };
}

describe('EquitiesArbSignalTable', () => {
  beforeEach(() => {
    __resetViewportClockRegistryForTests();
  });

  afterEach(() => {
    vi.useRealTimers();
    __resetViewportClockRegistryForTests();
  });

  it('renders one shared equities table with visible variant labels sorted by symbol then variant', () => {
    render(
      <EquitiesArbSignalTable
        rows={[
          buildEquitiesStrategy('aapl_tradexyz_taker', 'equities_taker', 'Taker'),
          buildEquitiesStrategy('aapl_tradexyz_maker', 'equities_maker', 'Maker'),
        ]}
      />,
    );

    expect(screen.getByTestId('equities-arb-signal-table')).toBeInTheDocument();
    expect(screen.getByText('Variant')).toBeInTheDocument();
    expect(screen.getByText('Maker Market')).toBeInTheDocument();
    expect(screen.getByText('Hedge Market')).toBeInTheDocument();

    const strategyCells = screen.getAllByText(/aapl_tradexyz_(maker|taker)/i);
    expect(strategyCells.map((cell) => cell.textContent)).toEqual([
      'aapl_tradexyz_maker',
      'aapl_tradexyz_taker',
    ]);
    expect(screen.getByText('Maker')).toBeInTheDocument();
    expect(screen.getByText('Taker')).toBeInTheDocument();
  });

  it('preserves the prior equities observability and trading-status semantics on the shared surface', () => {
    const pausedMaker = buildEquitiesStrategy('aapl_tradexyz_maker', 'equities_maker', 'Maker');
    pausedMaker.params.bot_on = '0';

    const pendingTaker = buildEquitiesStrategy('aapl_tradexyz_taker', 'equities_taker', 'Taker');
    pendingTaker.equities_arb = {
      ...pendingTaker.equities_arb,
      quote_snapshot: {
        ...pendingTaker.equities_arb?.quote_snapshot,
        hedge_ready: false,
        hedge_disabled_reason: 'outside_rth_blocked',
      },
    };

    render(<EquitiesArbSignalTable rows={[pausedMaker, pendingTaker]} />);

    expect(screen.getByText('Hedge')).toBeInTheDocument();
    expect(screen.getByText('Last Updated')).toBeInTheDocument();
    expect(screen.getAllByText('SMART · DAY').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Feed ok · Quote fresh').length).toBeGreaterThan(0);
    expect(screen.getByText('Paused')).toBeInTheDocument();
    expect(screen.getByText('Pending')).toBeInTheDocument();
    expect(screen.getByText('aapl_tradexyz_maker')).toHaveAttribute(
      'title',
      expect.stringContaining('Assumed hedge fee: 1.00 bps'),
    );
  });

  it('requires shared equities_arb payloads while still deriving split variants from metadata', () => {
    const makerRow = buildEquitiesStrategy('aapl_zzz_maker', 'equities_maker', 'Maker');
    makerRow.meta = {
      ...makerRow.meta,
      base_asset: 'AAPL',
    };

    const takerBase = buildEquitiesStrategy('aapl_aaa_taker', 'equities_taker', 'Taker');
    const metadataOnlyTaker: SignalStrategy = {
      ...takerBase,
      strategy_family: '' as any,
      meta: {
        ...takerBase.meta,
        strategy_family: undefined,
        class: 'equities_taker',
        param_set: 'equities_taker',
        base_asset: 'AAPL',
      },
    };
    const legacyMakerOnly: SignalStrategy = {
      ...buildEquitiesStrategy('aapl_legacy_makerv4', 'equities_maker', 'Maker'),
      strategy_family: 'maker_v4',
      meta: {
        chain: 'equities',
        strategy_groups: 'equities',
        strategy_family: 'maker_v4',
        class: 'maker_v4',
        param_set: 'makerv4',
        base_asset: 'AAPL',
        quote_asset: 'USD',
      },
      maker_v4: makerRow.equities_arb as any,
      equities_arb: undefined,
    };

    render(<EquitiesArbSignalTable rows={[legacyMakerOnly, metadataOnlyTaker, makerRow]} />);

    const strategyCells = screen.getAllByText(/aapl_(aaa_taker|zzz_maker)/i);
    expect(strategyCells.map((cell) => cell.textContent)).toEqual([
      'aapl_zzz_maker',
      'aapl_aaa_taker',
    ]);
    expect(screen.queryByText('aapl_legacy_makerv4')).not.toBeInTheDocument();
    expect(screen.getByText('Taker')).toBeInTheDocument();
    expect(screen.getAllByText('SMART · DAY').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Feed ok · Quote fresh').length).toBeGreaterThan(0);
  });

  it('uses the top-level strategy family as the authoritative shared equities variant even when legacy metadata is stale', () => {
    const makerRow = buildEquitiesStrategy('aapl_zzz_maker', 'equities_maker', 'Maker');

    const staleMetaTaker: SignalStrategy = {
      ...buildEquitiesStrategy('aapl_aaa_taker', 'equities_taker', 'Taker'),
      strategy_family: 'equities_taker',
      meta: {
        chain: 'equities',
        strategy_groups: 'equities',
        strategy_family: 'maker_v4',
        class: 'maker_v4',
        param_set: 'makerv4',
        base_asset: 'AAPL',
        quote_asset: 'USD',
      },
    };

    render(<EquitiesArbSignalTable rows={[staleMetaTaker, makerRow]} />);

    const strategyCells = screen.getAllByText(/aapl_(aaa_taker|zzz_maker)/i);
    expect(strategyCells.map((cell) => cell.textContent)).toEqual([
      'aapl_zzz_maker',
      'aapl_aaa_taker',
    ]);
    expect(screen.getByText('Taker')).toBeInTheDocument();
  });

  it('uses worst-leg freshness and hides actionable hedge/spread states when quote health is stale', () => {
    const strategy = buildEquitiesStrategy('aapl_tradexyz_maker', 'equities_maker', 'Maker');
    strategy.equities_arb = {
      ...strategy.equities_arb,
      quote_snapshot: {
        ...strategy.equities_arb?.quote_snapshot,
        hedge_ready: true,
        hedge_disabled_reason: null,
        maker_leg: {
          ...strategy.equities_arb?.quote_snapshot?.maker_leg,
          age_ms: 1_000,
          ts_ms: 1_700_000_009_000,
          quote_state: 'fresh',
          pricing_usable: true,
          hedge_usable: true,
        },
        hedge_leg: {
          ...strategy.equities_arb?.quote_snapshot?.hedge_leg,
          age_ms: 9_000,
          ts_ms: 1_700_000_001_000,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          reason_code: 'hedge_quote_old',
        },
        ref_leg: {
          ...strategy.equities_arb?.quote_snapshot?.ref_leg,
          age_ms: 9_000,
          ts_ms: 1_700_000_001_000,
          quote_state: 'old',
          pricing_usable: false,
          hedge_usable: false,
          reason_code: 'reference_quote_old',
        },
      },
    };

    render(
      <EquitiesArbSignalTable
        rows={[strategy]}
        nowProvider={() => 1_700_000_010_000}
      />,
    );

    expect(screen.getByText(/\(9s ago\)/)).toBeInTheDocument();
    expect(screen.getByText(/Blocked/i)).toBeInTheDocument();
    expect(screen.getByText(/Quote health/i)).toBeInTheDocument();
    expect(screen.getAllByText(/^stale$/i).length).toBeGreaterThanOrEqual(2);
    expect(screen.queryByText('2.0 bps')).not.toBeInTheDocument();
    expect(screen.queryByText('B 14.0')).not.toBeInTheDocument();
    expect(screen.queryByText('A -11.0')).not.toBeInTheDocument();
  });

  it('ticks the last-updated age label on the shared equities surface', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_700_000_001_500);

    const strategy = buildEquitiesStrategy('aapl_tradexyz_maker', 'equities_maker', 'Maker');
    strategy.equities_arb = {
      ...strategy.equities_arb,
      quote_snapshot: {
        ...strategy.equities_arb?.quote_snapshot,
        maker_leg: {
          ...strategy.equities_arb?.quote_snapshot?.maker_leg,
          age_ms: 1_000,
          ts_ms: 1_700_000_000_500,
        },
        hedge_leg: {
          ...strategy.equities_arb?.quote_snapshot?.hedge_leg,
          age_ms: 1_000,
          ts_ms: 1_700_000_000_500,
        },
        ref_leg: {
          ...strategy.equities_arb?.quote_snapshot?.ref_leg,
          age_ms: 1_000,
          ts_ms: 1_700_000_000_500,
        },
      },
    };

    render(
      <EquitiesArbSignalTable
        rows={[strategy]}
      />,
    );

    expect(screen.getByText(/\(1s ago\)/)).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(5_000);
    });

    expect(screen.getByText(/\(6s ago\)/)).toBeInTheDocument();
  });
});
